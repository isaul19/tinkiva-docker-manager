//! Cliente HTTPS saliente mínimo, delegado en `curl`.
//!
//! El panel no enlaza ninguna pila TLS: invoca `curl` como subproceso igual que
//! ya hacía el auto-actualizador. Así el binario sigue sin dependencias y el
//! consumo en reposo no cambia (el proceso `curl` vive milisegundos y muere).
//!
//! Reglas de seguridad aplicadas a cada petición:
//!   * solo `https://` y solo hacia hosts de la lista blanca,
//!   * sin seguir redirecciones (evita salir de la lista blanca),
//!   * las cabeceras y el cuerpo viajan por stdin, nunca por `argv`,
//!     para que los secretos no aparezcan en `ps`.

use crate::util::unique_suffix;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

const USER_AGENT: &str = concat!("tinkiva-docker-manager/", env!("CARGO_PKG_VERSION"));

/// Hosts a los que el panel puede hablar. Cualquier otro se rechaza antes de
/// ejecutar `curl`.
const ALLOWED_HOSTS: [&str; 4] = [
    "api.github.com",
    "github.com",
    "hub.docker.com",
    "registry.hub.docker.com",
];

pub struct Outbound {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<String>,
    pub body: Option<String>,
    pub timeout_secs: u64,
    pub max_bytes: usize,
}

impl Outbound {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET",
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout_secs: 20,
            max_bytes: 512 * 1024,
        }
    }

    pub fn post(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: "POST",
            url: url.into(),
            headers: Vec::new(),
            body: Some(body.into()),
            timeout_secs: 30,
            max_bytes: 512 * 1024,
        }
    }

    #[must_use]
    pub fn header(mut self, header: impl Into<String>) -> Self {
        self.headers.push(header.into());
        self
    }

    #[must_use]
    pub fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

pub struct Fetched {
    pub status: u16,
    pub body: String,
}

impl Fetched {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Mensaje corto y legible para devolver al panel cuando la API remota falla.
    pub fn error_summary(&self, service: &str) -> String {
        let detail = crate::json::Json::parse(&self.body)
            .ok()
            .and_then(|value| {
                value
                    .string("message")
                    .or_else(|| value.string("error_description"))
                    .or_else(|| value.string("error"))
                    .or_else(|| value.string("detail"))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| crate::util::truncate_text(self.body.trim(), 300));

        if detail.is_empty() {
            format!("{service} respondió {}", self.status)
        } else {
            format!("{service} respondió {}: {detail}", self.status)
        }
    }
}

pub fn fetch(request: &Outbound) -> Result<Fetched, String> {
    validate_url(&request.url)?;
    for header in &request.headers {
        if header.contains('\r') || header.contains('\n') || !header.contains(':') {
            return Err("cabecera saliente inválida".to_owned());
        }
    }

    let body_path = std::env::temp_dir().join(format!("tdm-net-{}.body", unique_suffix()));
    let mut command = Command::new("curl");
    command
        .arg("--config")
        .arg("-")
        .arg("--silent")
        .arg("--show-error")
        .arg("--proto")
        .arg("=https")
        .arg("--tlsv1.2")
        .arg("--no-location")
        .arg("--max-time")
        .arg(request.timeout_secs.clamp(5, 120).to_string())
        .arg("--user-agent")
        .arg(USER_AGENT)
        .arg("--request")
        .arg(request.method)
        .arg("--output")
        .arg(&body_path)
        .arg("--write-out")
        .arg("%{http_code}")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|error| {
        let _ = fs::remove_file(&body_path);
        format!("no se pudo ejecutar curl (requerido para GitHub y Docker Hub): {error}")
    })?;

    let configuration = build_configuration(request);
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(configuration.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&body_path);
            return Err(format!("no se pudo enviar la configuración a curl: {error}"));
        }
    }

    let output = child.wait_with_output().map_err(|error| {
        let _ = fs::remove_file(&body_path);
        format!("no se pudo esperar a curl: {error}")
    })?;

    let raw_body = fs::read(&body_path).unwrap_or_default();
    let _ = fs::remove_file(&body_path);

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            "curl no pudo completar la petición".to_owned()
        } else {
            format!("curl: {}", crate::util::truncate_text(detail, 300))
        });
    }

    let status = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .unwrap_or(0);
    if status == 0 {
        return Err("curl no devolvió un código HTTP válido".to_owned());
    }

    let truncated = if raw_body.len() > request.max_bytes {
        &raw_body[..request.max_bytes]
    } else {
        raw_body.as_slice()
    };

    Ok(Fetched {
        status,
        body: String::from_utf8_lossy(truncated).into_owned(),
    })
}

fn build_configuration(request: &Outbound) -> String {
    let mut configuration = String::with_capacity(256);
    configuration.push_str(&format!("url = {}\n", quote(&request.url)));
    for header in &request.headers {
        configuration.push_str(&format!("header = {}\n", quote(header)));
    }
    if let Some(body) = &request.body {
        configuration.push_str(&format!("data = {}\n", quote(body)));
    }
    configuration
}

/// Codifica un valor para el formato de archivo de configuración de curl, donde
/// dentro de comillas dobles solo `\` y `"` necesitan escape.
fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn validate_url(url: &str) -> Result<(), String> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err("solo se permiten URLs https".to_owned());
    };
    if url.len() > 2048 || url.chars().any(|character| character.is_control()) {
        return Err("URL saliente inválida".to_owned());
    }

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        return Err("no se permiten credenciales en la URL".to_owned());
    }
    // Descarta un puerto explícito: solo hablamos por el 443 implícito.
    if authority.contains(':') {
        return Err("no se permite especificar puerto".to_owned());
    }
    if !ALLOWED_HOSTS.contains(&authority) {
        return Err(format!("host no permitido: {authority}"));
    }
    Ok(())
}

/// Indica si `curl` está disponible; el panel lo usa para avisar en la interfaz
/// antes de ofrecer funciones que dependen de la red.
pub fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Indica si `openssl` está disponible (necesario para firmar los JWT de GitHub App).
pub fn openssl_available() -> bool {
    Command::new("openssl")
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allows_whitelisted_https_hosts() {
        assert!(validate_url("https://api.github.com/app/installations").is_ok());
        assert!(validate_url("https://hub.docker.com/v2/search/repositories/?query=a").is_ok());
        assert!(validate_url("http://api.github.com/x").is_err());
        assert!(validate_url("https://evil.example.com/x").is_err());
        assert!(validate_url("https://api.github.com.evil.com/x").is_err());
        assert!(validate_url("https://user:pass@api.github.com/x").is_err());
        assert!(validate_url("https://api.github.com:8443/x").is_err());
    }

    #[test]
    fn configuration_quotes_secrets_safely() {
        let request = Outbound::post("https://github.com/login/oauth/access_token", "a=\"b\"\\c")
            .header("Authorization: Bearer tok\"en");
        let configuration = build_configuration(&request);
        assert!(configuration.contains(r#"header = "Authorization: Bearer tok\"en""#));
        assert!(configuration.contains(r#"data = "a=\"b\"\\c""#));
        assert!(!configuration.contains('\u{0}'));
    }

    #[test]
    fn rejects_header_injection() {
        let request = Outbound::get("https://api.github.com/user")
            .header("X-Bad: value\r\nAuthorization: Bearer stolen");
        assert!(fetch(&request).is_err());
    }
}
