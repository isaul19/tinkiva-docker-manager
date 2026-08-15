//! Integración con GitHub App.
//!
//! Reproduce el flujo «un clic» de Coolify/Vercel:
//!   1. el panel genera un *manifest* y envía al navegador a
//!      `github.com/settings/apps/new`, donde GitHub crea la App por ti;
//!   2. GitHub vuelve a `/github/callback?code=…` y el panel canjea ese código
//!      por las credenciales definitivas (App ID y clave privada);
//!   3. el panel te lleva a `github.com/apps/<slug>/installations/new` para elegir
//!      todos los repositorios o solo algunos;
//!   4. a partir de ahí el panel firma JWT RS256 y pide tokens de instalación
//!      para listar repositorios, clonar y consultar commits por polling.
//!
//! Los redirect vienen del navegador y por tanto no llevan la cabecera
//! `Authorization`; se validan con un *nonce* de un solo uso emitido por el panel.

use crate::crypto::{base64_url, constant_time_eq_bytes, rs256_sign};
use crate::json::Json;
use crate::net::{fetch, Outbound};
use crate::util::{atomic_write, json_string, now_unix, random_hex};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

const NONCE_TTL_SECONDS: u64 = 15 * 60;
const MAX_PENDING_NONCES: usize = 16;
const TOKEN_MARGIN_SECONDS: u64 = 300;
const ACCEPT: &str = "Accept: application/vnd.github+json";
const API_VERSION: &str = "X-GitHub-Api-Version: 2022-11-28";

#[derive(Clone, Debug, Default)]
pub struct AppCredentials {
    pub app_id: u64,
    pub slug: String,
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub webhook_secret: String,
    pub private_key: String,
    pub html_url: String,
    pub connected_at: u64,
}

pub struct GitHub {
    path: PathBuf,
    credentials: Mutex<Option<AppCredentials>>,
    nonces: Mutex<Vec<(String, u64)>>,
    tokens: Mutex<HashMap<u64, (String, u64)>>,
}

impl GitHub {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let credentials = if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .map_err(|error| format!("no se pudo leer {}: {error}", path.display()))?;
            Some(parse_credentials(&contents)?)
        } else {
            None
        };

        Ok(Self {
            path,
            credentials: Mutex::new(credentials),
            nonces: Mutex::new(Vec::new()),
            tokens: Mutex::new(HashMap::new()),
        })
    }

    fn credentials(&self) -> Result<Option<AppCredentials>, String> {
        self.credentials
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| "el estado de GitHub quedó bloqueado".to_owned())
    }

    fn require(&self) -> Result<AppCredentials, String> {
        self.credentials()?
            .ok_or_else(|| "todavía no has conectado una GitHub App".to_owned())
    }

    pub fn is_connected(&self) -> bool {
        self.credentials().ok().flatten().is_some()
    }

    fn persist(&self, credentials: AppCredentials) -> Result<(), String> {
        atomic_write(&self.path, serialize(&credentials).as_bytes(), 0o600)
            .map_err(|error| format!("no se pudo guardar la GitHub App: {error}"))?;
        let mut guard = self
            .credentials
            .lock()
            .map_err(|_| "el estado de GitHub quedó bloqueado".to_owned())?;
        *guard = Some(credentials);
        self.forget_tokens();
        Ok(())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .map_err(|error| format!("no se pudo borrar la configuración: {error}"))?;
        }
        let mut guard = self
            .credentials
            .lock()
            .map_err(|_| "el estado de GitHub quedó bloqueado".to_owned())?;
        *guard = None;
        drop(guard);
        self.forget_tokens();
        Ok(())
    }

    fn forget_tokens(&self) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.clear();
        }
    }

    // ── Nonces de un solo uso para los redirect del navegador ──────────────

    pub fn issue_nonce(&self) -> Result<String, String> {
        let nonce = random_hex(24).map_err(|error| format!("no se pudo generar estado: {error}"))?;
        let mut nonces = self
            .nonces
            .lock()
            .map_err(|_| "el estado de GitHub quedó bloqueado".to_owned())?;
        let now = now_unix();
        nonces.retain(|(_, issued)| now.saturating_sub(*issued) < NONCE_TTL_SECONDS);
        if nonces.len() >= MAX_PENDING_NONCES {
            nonces.remove(0);
        }
        nonces.push((nonce.clone(), now));
        Ok(nonce)
    }

    pub fn consume_nonce(&self, candidate: &str) -> bool {
        let Ok(mut nonces) = self.nonces.lock() else {
            return false;
        };
        let now = now_unix();
        nonces.retain(|(_, issued)| now.saturating_sub(*issued) < NONCE_TTL_SECONDS);
        let Some(index) = nonces.iter().position(|(nonce, _)| {
            constant_time_eq_bytes(nonce.as_bytes(), candidate.as_bytes())
        }) else {
            return false;
        };
        nonces.remove(index);
        true
    }

    // ── Alta de la App ─────────────────────────────────────────────────────

    /// Manifiesto que el navegador envía por POST a `github.com/settings/apps/new`.
    ///
    /// `panel_url` es la dirección con la que **el navegador** ve el panel, así que
    /// puede ser `localhost` sin problema: los redirect los hace el propio navegador.
    /// No se solicita webhook ni eventos: el watcher consulta la API desde el servidor.
    pub fn manifest(&self, panel_url: &str, suffix: &str) -> String {
        format!(
            concat!(
                "{{",
                "\"name\":{},",
                "\"url\":\"https://github.com/isaul19/tinkiva-docker-manager\",",
                "\"redirect_url\":{},",
                "\"setup_url\":{},",
                "\"callback_urls\":[{}],",
                "\"setup_on_update\":true,",
                "\"public\":false,",
                "\"default_permissions\":{{",
                "\"contents\":\"read\",\"metadata\":\"read\"",
                "}}",
                "}}"
            ),
            json_string(&format!("Tinkiva DM {suffix}")),
            json_string(&format!("{panel_url}/github/callback")),
            json_string(&format!("{panel_url}/github/installed")),
            json_string(&format!("{panel_url}/github/callback")),
        )
    }

    /// Canjea el `code` del manifiesto por las credenciales definitivas.
    pub fn complete_manifest(&self, code: &str) -> Result<(), String> {
        if code.is_empty() || code.len() > 128 || !code.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err("código de manifiesto inválido".to_owned());
        }

        let response = fetch(
            &Outbound::post(
                format!("https://api.github.com/app-manifests/{code}/conversions"),
                String::new(),
            )
            .header(ACCEPT)
            .header(API_VERSION),
        )?;
        if !response.is_success() {
            return Err(response.error_summary("GitHub"));
        }

        let document = Json::parse(&response.body)
            .map_err(|error| format!("respuesta inesperada de GitHub: {error}"))?;
        let credentials = AppCredentials {
            app_id: document
                .number("id")
                .ok_or_else(|| "GitHub no devolvió el App ID".to_owned())?,
            slug: document.string("slug").unwrap_or_default().to_owned(),
            name: document.string("name").unwrap_or_default().to_owned(),
            client_id: document.string("client_id").unwrap_or_default().to_owned(),
            client_secret: document
                .string("client_secret")
                .unwrap_or_default()
                .to_owned(),
            webhook_secret: document
                .string("webhook_secret")
                .unwrap_or_default()
                .to_owned(),
            private_key: document
                .string("pem")
                .ok_or_else(|| "GitHub no devolvió la clave privada".to_owned())?
                .to_owned(),
            html_url: document.string("html_url").unwrap_or_default().to_owned(),
            connected_at: now_unix(),
        };
        self.persist(credentials)
    }

    /// Alta manual para quien ya tiene una GitHub App creada.
    pub fn connect_manually(
        &self,
        app_id: u64,
        slug: &str,
        private_key: &str,
    ) -> Result<(), String> {
        if app_id == 0 {
            return Err("App ID inválido".to_owned());
        }
        if !valid_slug(slug) {
            return Err("slug de la App inválido".to_owned());
        }
        if !private_key.contains("PRIVATE KEY") {
            return Err("la clave privada debe estar en formato PEM".to_owned());
        }

        self.persist(AppCredentials {
            app_id,
            slug: slug.to_owned(),
            name: slug.to_owned(),
            client_id: String::new(),
            client_secret: String::new(),
            webhook_secret: String::new(),
            private_key: private_key.to_owned(),
            html_url: format!("https://github.com/apps/{slug}"),
            connected_at: now_unix(),
        })
    }

    // ── Autenticación con GitHub ───────────────────────────────────────────

    fn jwt(&self) -> Result<String, String> {
        let credentials = self.require()?;
        let now = now_unix();
        let header = base64_url(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = base64_url(
            format!(
                "{{\"iat\":{},\"exp\":{},\"iss\":\"{}\"}}",
                now.saturating_sub(60),
                now + 540,
                credentials.app_id
            )
            .as_bytes(),
        );
        let signing_input = format!("{header}.{payload}");
        let signature = rs256_sign(&credentials.private_key, signing_input.as_bytes())?;
        Ok(format!("{signing_input}.{}", base64_url(&signature)))
    }

    pub fn installation_token(&self, installation_id: u64) -> Result<String, String> {
        if let Ok(tokens) = self.tokens.lock() {
            if let Some((token, expires_at)) = tokens.get(&installation_id) {
                if *expires_at > now_unix() + TOKEN_MARGIN_SECONDS {
                    return Ok(token.clone());
                }
            }
        }

        let jwt = self.jwt()?;
        let response = fetch(
            &Outbound::post(
                format!("https://api.github.com/app/installations/{installation_id}/access_tokens"),
                String::new(),
            )
            .header(format!("Authorization: Bearer {jwt}"))
            .header(ACCEPT)
            .header(API_VERSION),
        )?;
        if !response.is_success() {
            return Err(response.error_summary("GitHub"));
        }

        let document = Json::parse(&response.body)
            .map_err(|error| format!("respuesta inesperada de GitHub: {error}"))?;
        let token = document
            .string("token")
            .ok_or_else(|| "GitHub no devolvió token de instalación".to_owned())?
            .to_owned();

        if let Ok(mut tokens) = self.tokens.lock() {
            // Los tokens de instalación duran una hora; renovamos algo antes.
            tokens.insert(installation_id, (token.clone(), now_unix() + 3000));
        }
        Ok(token)
    }

    fn api_get(&self, url: String, token: &str) -> Result<Json, String> {
        let response = fetch(
            &Outbound::get(url)
                .header(format!("Authorization: Bearer {token}"))
                .header(ACCEPT)
                .header(API_VERSION)
                .max_bytes(1024 * 1024),
        )?;
        if !response.is_success() {
            return Err(response.error_summary("GitHub"));
        }
        Json::parse(&response.body)
            .map_err(|error| format!("respuesta inesperada de GitHub: {error}"))
    }

    // ── Consultas que alimentan la interfaz ────────────────────────────────

    pub fn installations_json(&self) -> Result<String, String> {
        let jwt = self.jwt()?;
        let document = self.api_get(
            "https://api.github.com/app/installations?per_page=100".to_owned(),
            &jwt,
        )?;
        let installations = document.as_array().unwrap_or_default();

        let entries: Vec<String> = installations
            .iter()
            .filter_map(|installation| {
                let id = installation.number("id")?;
                let account = installation.get("account");
                Some(format!(
                    concat!(
                        "{{\"id\":{},\"login\":{},\"avatar_url\":{},\"type\":{},",
                        "\"repository_selection\":{},\"html_url\":{}}}"
                    ),
                    id,
                    json_string(
                        account
                            .and_then(|account| account.string("login"))
                            .unwrap_or_default()
                    ),
                    json_string(
                        account
                            .and_then(|account| account.string("avatar_url"))
                            .unwrap_or_default()
                    ),
                    json_string(
                        account
                            .and_then(|account| account.string("type"))
                            .unwrap_or("User")
                    ),
                    json_string(
                        installation
                            .string("repository_selection")
                            .unwrap_or("selected")
                    ),
                    json_string(installation.string("html_url").unwrap_or_default()),
                ))
            })
            .collect();

        Ok(format!("[{}]", entries.join(",")))
    }

    pub fn repositories_json(&self, installation_id: u64) -> Result<String, String> {
        let token = self.installation_token(installation_id)?;
        let document = self.api_get(
            "https://api.github.com/installation/repositories?per_page=100".to_owned(),
            &token,
        )?;
        let repositories = document.array("repositories").unwrap_or_default();

        let entries: Vec<String> = repositories
            .iter()
            .filter_map(|repository| {
                let full_name = repository.string("full_name")?;
                Some(format!(
                    concat!(
                        "{{\"full_name\":{},\"private\":{},\"default_branch\":{},",
                        "\"description\":{},\"language\":{},\"updated_at\":{},\"html_url\":{}}}"
                    ),
                    json_string(full_name),
                    repository
                        .get("private")
                        .and_then(Json::as_bool)
                        .unwrap_or(true),
                    json_string(repository.string("default_branch").unwrap_or("main")),
                    json_string(repository.string("description").unwrap_or_default()),
                    json_string(repository.string("language").unwrap_or_default()),
                    json_string(repository.string("updated_at").unwrap_or_default()),
                    json_string(repository.string("html_url").unwrap_or_default()),
                ))
            })
            .collect();

        Ok(format!("[{}]", entries.join(",")))
    }

    pub fn branches_json(&self, installation_id: u64, repository: &str) -> Result<String, String> {
        if !valid_repository(repository) {
            return Err("repositorio inválido".to_owned());
        }
        let token = self.installation_token(installation_id)?;
        let document = self.api_get(
            format!("https://api.github.com/repos/{repository}/branches?per_page=100"),
            &token,
        )?;

        let entries: Vec<String> = document
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(|branch| branch.string("name").map(json_string))
            .collect();
        Ok(format!("[{}]", entries.join(",")))
    }

    /// Devuelve la revisión actual de una rama para el watcher saliente.
    pub fn branch_commit(
        &self,
        installation_id: u64,
        repository: &str,
        branch: &str,
    ) -> Result<String, String> {
        if !valid_repository(repository)
            || branch.is_empty()
            || branch.len() > 255
            || branch.chars().any(char::is_whitespace)
        {
            return Err("repositorio o rama inválidos".to_owned());
        }
        let token = self.installation_token(installation_id)?;
        let document = self.api_get(
            format!("https://api.github.com/repos/{repository}/commits/{branch}"),
            &token,
        )?;
        document
            .string("sha")
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(str::to_owned)
            .ok_or_else(|| "GitHub no devolvió un commit válido".to_owned())
    }

    // ── Webhooks ───────────────────────────────────────────────────────────

    /// Estado que consume la interfaz. Nunca incluye secretos.
    pub fn status_json(&self, panel_url: &str) -> Result<String, String> {
        let credentials = self.credentials()?;

        Ok(match credentials {
            Some(credentials) => format!(
                concat!(
                    "{{\"connected\":true,\"app_id\":{},\"slug\":{},\"name\":{},",
                    "\"html_url\":{},\"install_url\":{},\"connected_at\":{},",
                    "\"panel_url\":{},\"polling\":true,\"settings_url\":{}}}"
                ),
                credentials.app_id,
                json_string(&credentials.slug),
                json_string(&credentials.name),
                json_string(&credentials.html_url),
                json_string(&format!(
                    "https://github.com/apps/{}/installations/new",
                    credentials.slug
                )),
                credentials.connected_at,
                json_string(panel_url),
                json_string(&format!(
                    "https://github.com/settings/apps/{}",
                    credentials.slug
                )),
            ),
            None => format!(
                "{{\"connected\":false,\"panel_url\":{},\"polling\":true}}",
                json_string(panel_url)
            ),
        })
    }
}

pub fn valid_repository(value: &str) -> bool {
    let Some((owner, name)) = value.split_once('/') else {
        return false;
    };
    let segment = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    };
    segment(owner) && segment(name) && !name.contains("..")
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn serialize(credentials: &AppCredentials) -> String {
    format!(
        concat!(
            "{{\"app_id\":{},\"slug\":{},\"name\":{},\"client_id\":{},",
            "\"client_secret\":{},\"webhook_secret\":{},\"private_key\":{},",
            "\"html_url\":{},\"connected_at\":{}}}\n"
        ),
        credentials.app_id,
        json_string(&credentials.slug),
        json_string(&credentials.name),
        json_string(&credentials.client_id),
        json_string(&credentials.client_secret),
        json_string(&credentials.webhook_secret),
        json_string(&credentials.private_key),
        json_string(&credentials.html_url),
        credentials.connected_at,
    )
}

fn parse_credentials(contents: &str) -> Result<AppCredentials, String> {
    let document = Json::parse(contents.trim())
        .map_err(|error| format!("configuración de GitHub corrupta: {error}"))?;
    Ok(AppCredentials {
        app_id: document
            .number("app_id")
            .ok_or_else(|| "falta app_id en la configuración de GitHub".to_owned())?,
        slug: document.string("slug").unwrap_or_default().to_owned(),
        name: document.string("name").unwrap_or_default().to_owned(),
        client_id: document.string("client_id").unwrap_or_default().to_owned(),
        client_secret: document
            .string("client_secret")
            .unwrap_or_default()
            .to_owned(),
        webhook_secret: document
            .string("webhook_secret")
            .unwrap_or_default()
            .to_owned(),
        private_key: document.string("private_key").unwrap_or_default().to_owned(),
        html_url: document.string("html_url").unwrap_or_default().to_owned(),
        connected_at: document.number("connected_at").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance() -> GitHub {
        GitHub::load(std::env::temp_dir().join(format!(
            "tdm-github-test-{}.json",
            crate::util::unique_suffix()
        )))
        .unwrap()
    }

    #[test]
    fn nonces_are_single_use_and_unknown_values_are_rejected() {
        let github = instance();
        let nonce = github.issue_nonce().unwrap();
        assert!(github.consume_nonce(&nonce));
        assert!(!github.consume_nonce(&nonce));
        assert!(!github.consume_nonce("no-emitido"));
        assert!(!github.consume_nonce(""));
    }

    #[test]
    fn manifest_points_every_callback_at_the_panel() {
        let github = instance();
        let manifest = github.manifest("https://panel.example.com", "abc123");
        let parsed = Json::parse(&manifest).unwrap();
        assert_eq!(parsed.string("name"), Some("Tinkiva DM abc123"));
        assert_eq!(
            parsed.string("redirect_url"),
            Some("https://panel.example.com/github/callback")
        );
        assert_eq!(
            parsed.string("setup_url"),
            Some("https://panel.example.com/github/installed")
        );
        assert!(parsed.get("hook_attributes").is_none());
        assert!(parsed.get("default_events").is_none());
        assert_eq!(parsed.get("public").and_then(Json::as_bool), Some(false));
    }

    #[test]
    fn manifest_omits_the_hook_when_the_panel_is_not_public() {
        // Los callbacks vuelven mediante el navegador, por eso localhost es válido.
        let github = instance();
        let manifest = github.manifest("http://localhost:8787", "abc123");
        let parsed = Json::parse(&manifest).unwrap();

        assert!(parsed.get("hook_attributes").is_none());
        assert_eq!(
            parsed.string("redirect_url"),
            Some("http://localhost:8787/github/callback")
        );
        assert_eq!(
            parsed.string("setup_url"),
            Some("http://localhost:8787/github/installed")
        );
    }

    #[test]
    fn credentials_round_trip_including_the_pem() {
        let credentials = AppCredentials {
            app_id: 12345,
            slug: "tinkiva-dm-abc".to_owned(),
            name: "Tinkiva DM abc".to_owned(),
            client_id: "Iv1.abc".to_owned(),
            client_secret: "secret".to_owned(),
            webhook_secret: "hook".to_owned(),
            private_key: "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----\n"
                .to_owned(),
            html_url: "https://github.com/apps/tinkiva-dm-abc".to_owned(),
            connected_at: 1_700_000_000,
        };
        let decoded = parse_credentials(&serialize(&credentials)).unwrap();
        assert_eq!(decoded.app_id, credentials.app_id);
        assert_eq!(decoded.private_key, credentials.private_key);
        assert_eq!(decoded.webhook_secret, credentials.webhook_secret);
    }

    #[test]
    fn status_never_leaks_secrets() {
        let github = instance();
        github
            .persist(AppCredentials {
                app_id: 1,
                slug: "demo".to_owned(),
                webhook_secret: "super-secreto".to_owned(),
                private_key: "-----BEGIN RSA PRIVATE KEY-----".to_owned(),
                client_secret: "otro-secreto".to_owned(),
                ..AppCredentials::default()
            })
            .unwrap();

        let status = github.status_json("http://127.0.0.1:8787").unwrap();
        assert!(status.contains("\"connected\":true"));
        assert!(status.contains("https://github.com/apps/demo/installations/new"));
        assert!(!status.contains("super-secreto"));
        assert!(!status.contains("otro-secreto"));
        assert!(!status.contains("PRIVATE KEY"));
        github.disconnect().unwrap();
    }

    #[test]
    fn repository_names_are_validated() {
        assert!(valid_repository("isaul19/tinkiva-docker-manager"));
        assert!(valid_repository("a/b.c"));
        assert!(!valid_repository("sin-barra"));
        assert!(!valid_repository("a/../b"));
        assert!(!valid_repository("a/b/c"));
        assert!(!valid_repository("a/"));
        assert!(!valid_repository("a/.hidden"));
    }
}
