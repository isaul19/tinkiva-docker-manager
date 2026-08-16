//! Firma SigV4 y credenciales de Amazon ECR.
//!
//! AWS obliga a firmar cada petición con HMAC-SHA256 encadenado. El panel ya
//! traía esas primitivas en `crypto` (las usaba el JWT de GitHub), así que la
//! integración no añade ninguna dependencia ni exige el CLI de `aws` en el
//! servidor: se firma aquí y se envía con el mismo `curl` que el resto.
//!
//! El token que devuelve ECR dura doce horas y se guarda solo en memoria; en
//! disco vive únicamente la clave de acceso, con permisos 0600.

use crate::crypto::{hex, hmac_sha256, sha256};
use crate::json::Json;
use crate::net::{fetch, Outbound};
use crate::util::{atomic_write, decode_field, encode_field, json_string, now_unix};
use std::path::PathBuf;
use std::sync::Mutex;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const SERVICE: &str = "ecr";
const API_PREFIX: &str = "AmazonEC2ContainerRegistry_V20150921";
const CONTENT_TYPE: &str = "application/x-amz-json-1.1";
/// Los tokens de ECR duran 12 h; se renuevan antes para no pillar el borde.
const TOKEN_MARGIN_SECONDS: u64 = 600;
/// Techos deliberados: el panel presume de memoria constante, así que ni las
/// respuestas de AWS ni las listas que se mandan al navegador crecen sin freno.
const MAX_REPOSITORIES: usize = 100;
const MAX_TAGS: usize = 30;

#[derive(Clone, Debug, Default)]
pub struct EcrCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    /// Cuenta de AWS dueña del registro. Se deduce del token si no se indica.
    pub registry_id: String,
    pub connected_at: u64,
}

impl EcrCredentials {
    /// `<cuenta>.dkr.ecr.<region>.amazonaws.com`, el host contra el que se
    /// autentica Docker y con el que empiezan las imágenes.
    pub fn registry_host(&self) -> String {
        format!("{}.dkr.ecr.{}.amazonaws.com", self.registry_id, self.region)
    }
}

/// Sesión abierta contra ECR: usuario, contraseña temporal y caducidad.
#[derive(Clone, Debug)]
pub struct EcrToken {
    pub username: String,
    pub password: String,
    pub registry: String,
    pub expires_at: u64,
}

pub struct Ecr {
    path: PathBuf,
    credentials: Mutex<Option<EcrCredentials>>,
    token: Mutex<Option<EcrToken>>,
}

impl Ecr {
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
            token: Mutex::new(None),
        })
    }

    pub fn credentials(&self) -> Result<Option<EcrCredentials>, String> {
        self.credentials
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| "el estado de ECR quedó bloqueado".to_owned())
    }

    /// Indica si una imagen apunta al registro conectado. Sirve para no intentar
    /// autenticarse cuando el despliegue no tiene nada que ver con ECR.
    pub fn owns_image(&self, image: &str) -> bool {
        self.credentials()
            .ok()
            .flatten()
            .is_some_and(|credentials| image.starts_with(&credentials.registry_host()))
    }

    pub fn connect(&self, credentials: EcrCredentials) -> Result<EcrToken, String> {
        let token = request_token(&credentials)?;
        // El registro real lo dice ECR: así el usuario no tiene que saberse el
        // número de cuenta de memoria.
        let mut credentials = credentials;
        if credentials.registry_id.is_empty() {
            credentials.registry_id = registry_id_from(&token.registry);
        }
        credentials.connected_at = now_unix();

        atomic_write(&self.path, serialize(&credentials).as_bytes(), 0o600)
            .map_err(|error| format!("no se pudo guardar las credenciales: {error}"))?;
        *self
            .credentials
            .lock()
            .map_err(|_| "el estado de ECR quedó bloqueado".to_owned())? = Some(credentials);
        self.remember(token.clone())?;
        Ok(token)
    }

    pub fn disconnect(&self) -> Result<(), String> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .map_err(|error| format!("no se pudo borrar las credenciales: {error}"))?;
        }
        *self
            .credentials
            .lock()
            .map_err(|_| "el estado de ECR quedó bloqueado".to_owned())? = None;
        *self
            .token
            .lock()
            .map_err(|_| "el estado de ECR quedó bloqueado".to_owned())? = None;
        Ok(())
    }

    /// Token vigente, pidiendo uno nuevo solo si el anterior está por caducar.
    pub fn token(&self) -> Result<EcrToken, String> {
        let credentials = self.require_credentials()?;

        if let Ok(guard) = self.token.lock() {
            if let Some(token) = guard.as_ref() {
                if token.expires_at > now_unix() + TOKEN_MARGIN_SECONDS {
                    return Ok(token.clone());
                }
            }
        }

        let token = request_token(&credentials)?;
        self.remember(token.clone())?;
        Ok(token)
    }

    fn remember(&self, token: EcrToken) -> Result<(), String> {
        *self
            .token
            .lock()
            .map_err(|_| "el estado de ECR quedó bloqueado".to_owned())? = Some(token);
        Ok(())
    }

    /// Repositorios del registro, para poblar el desplegable del formulario.
    pub fn repositories(&self) -> Result<Vec<String>, String> {
        request_repositories(&self.require_credentials()?)
    }

    /// Etiquetas de un repositorio. Se piden solo al elegirlo: consultarlas
    /// todas de golpe serían tantas llamadas a AWS como repositorios haya.
    pub fn tags(&self, repository: &str) -> Result<Vec<EcrTag>, String> {
        if !valid_repository_name(repository) {
            return Err("nombre de repositorio inválido".to_owned());
        }
        request_tags(&self.require_credentials()?, repository)
    }

    fn require_credentials(&self) -> Result<EcrCredentials, String> {
        self.credentials()?
            .ok_or_else(|| "todavía no has conectado ECR".to_owned())
    }

    pub fn status_json(&self) -> Result<String, String> {
        let Some(credentials) = self.credentials()? else {
            return Ok("{\"connected\":false}".to_owned());
        };
        let expires_at = self
            .token
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|token| token.expires_at))
            .unwrap_or(0);
        Ok(format!(
            concat!(
                "{{\"connected\":true,\"region\":{},\"registry_id\":{},",
                "\"registry\":{},\"access_key_id\":{},\"connected_at\":{},",
                "\"token_expires_at\":{}}}"
            ),
            json_string(&credentials.region),
            json_string(&credentials.registry_id),
            json_string(&credentials.registry_host()),
            json_string(&mask_key(&credentials.access_key_id)),
            credentials.connected_at,
            expires_at,
        ))
    }
}

/// Solo se enseñan los últimos cuatro caracteres: basta para reconocer la clave
/// sin volver a exponerla en la interfaz.
fn mask_key(value: &str) -> String {
    if value.len() <= 4 {
        return "****".to_owned();
    }
    format!("****{}", &value[value.len() - 4..])
}

fn registry_id_from(endpoint: &str) -> String {
    endpoint
        .trim_start_matches("https://")
        .split('.')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Firma y envía una operación de la API de ECR, devolviendo el cuerpo crudo.
fn call(credentials: &EcrCredentials, operation: &str, body: &str) -> Result<String, String> {
    if !valid_region(&credentials.region) {
        return Err("región de AWS inválida".to_owned());
    }
    if credentials.access_key_id.trim().is_empty() || credentials.secret_access_key.is_empty() {
        return Err("faltan el access key id y su secret".to_owned());
    }

    let target = format!("{API_PREFIX}.{operation}");
    let host = format!("api.ecr.{}.amazonaws.com", credentials.region);
    let timestamp = now_unix();
    let amz_date = amz_date(timestamp);
    let date = amz_date[..8].to_owned();

    let signed_headers = "content-type;host;x-amz-date;x-amz-target";
    let canonical_headers = format!(
        "content-type:{CONTENT_TYPE}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-target:{target}\n"
    );
    let canonical_request = format!(
        "POST\n/\n\n{canonical_headers}\n{signed_headers}\n{}",
        hex(&sha256(body.as_bytes()))
    );
    let scope = format!("{date}/{}/{SERVICE}/aws4_request", credentials.region);
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex(&sha256(canonical_request.as_bytes()))
    );

    let signature = hex(&sign(credentials, &date, string_to_sign.as_bytes()));
    let authorization = format!(
        "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    );

    let response = fetch(
        &Outbound::post(format!("https://{host}/"), body)
            .header(format!("Content-Type: {CONTENT_TYPE}"))
            .header(format!("X-Amz-Target: {target}"))
            .header(format!("X-Amz-Date: {amz_date}"))
            .header(format!("Authorization: {authorization}"))
            .max_bytes(256 * 1024),
    )?;

    if response.status != 200 {
        return Err(describe_error(&response.body, response.status));
    }
    Ok(response.body)
}

/// Pide a ECR un token de acceso de doce horas.
fn request_token(credentials: &EcrCredentials) -> Result<EcrToken, String> {
    let body = call(credentials, "GetAuthorizationToken", "{}")?;
    let parsed = Json::parse(&body).map_err(|_| "ECR devolvió un JSON inválido".to_owned())?;
    let entry = parsed
        .get("authorizationData")
        .and_then(Json::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| "ECR no devolvió credenciales".to_owned())?;
    let encoded = entry
        .string("authorizationToken")
        .ok_or_else(|| "ECR no devolvió el token".to_owned())?;
    let registry = entry.string("proxyEndpoint").unwrap_or_default();
    // `expiresAt` llega en milisegundos desde epoch.
    let expires_at = entry
        .number("expiresAt")
        .map_or_else(|| now_unix() + 43_200, |value| value / 1000);

    let decoded = base64_decode(encoded).ok_or_else(|| "token de ECR ilegible".to_owned())?;
    let decoded = String::from_utf8(decoded).map_err(|_| "token de ECR ilegible".to_owned())?;
    let (username, password) = decoded
        .split_once(':')
        .ok_or_else(|| "token de ECR con formato inesperado".to_owned())?;

    Ok(EcrToken {
        username: username.to_owned(),
        password: password.to_owned(),
        registry: registry.trim_start_matches("https://").to_owned(),
        expires_at,
    })
}

/// Una etiqueta publicada en un repositorio, con lo justo para elegirla.
#[derive(Clone, Debug, PartialEq)]
pub struct EcrTag {
    pub tag: String,
    pub pushed_at: u64,
    pub size_bytes: u64,
}

/// Nombres de los repositorios del registro conectado, en orden alfabético.
fn request_repositories(credentials: &EcrCredentials) -> Result<Vec<String>, String> {
    let body = call(
        credentials,
        "DescribeRepositories",
        &format!("{{\"maxResults\":{MAX_REPOSITORIES}}}"),
    )?;
    parse_repositories(&body)
}

fn parse_repositories(body: &str) -> Result<Vec<String>, String> {
    let parsed = Json::parse(body).map_err(|_| "ECR devolvió un JSON inválido".to_owned())?;
    let mut names: Vec<String> = parsed
        .array("repositories")
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry.string("repositoryName"))
        .map(str::to_owned)
        .collect();
    names.sort();
    Ok(names)
}

/// Etiquetas de un repositorio, de la más reciente a la más antigua.
///
/// `DescribeImages` devuelve una entrada por *imagen*, no por etiqueta: una
/// imagen puede llevar varias (`latest` y el sha del commit, por ejemplo) y las
/// que ya no tienen ninguna —las que el CI dejó huérfanas— llegan sin lista.
fn request_tags(credentials: &EcrCredentials, repository: &str) -> Result<Vec<EcrTag>, String> {
    let body = call(
        credentials,
        "DescribeImages",
        &format!(
            "{{\"repositoryName\":{},\"maxResults\":100}}",
            json_string(repository)
        ),
    )?;
    parse_tags(&body)
}

fn parse_tags(body: &str) -> Result<Vec<EcrTag>, String> {
    let parsed = Json::parse(body).map_err(|_| "ECR devolvió un JSON inválido".to_owned())?;

    let mut tags = Vec::new();
    for detail in parsed.array("imageDetails").unwrap_or_default() {
        // `imagePushedAt` llega como epoch en segundos y con decimales, así que
        // `as_u64` (que exige un entero exacto) lo descartaría.
        let pushed_at = match detail.get("imagePushedAt") {
            Some(Json::Number(value)) if *value >= 0.0 => *value as u64,
            _ => 0,
        };
        let size_bytes = detail.number("imageSizeInBytes").unwrap_or(0);
        for tag in detail.array("imageTags").unwrap_or_default() {
            if let Some(tag) = tag.as_str() {
                tags.push(EcrTag {
                    tag: tag.to_owned(),
                    pushed_at,
                    size_bytes,
                });
            }
        }
    }
    tags.sort_by(|left, right| right.pushed_at.cmp(&left.pushed_at).then(left.tag.cmp(&right.tag)));
    tags.truncate(MAX_TAGS);
    Ok(tags)
}

/// Traduce los errores típicos de AWS a algo accionable.
fn describe_error(body: &str, status: u16) -> String {
    let message = Json::parse(body).ok().map_or_else(String::new, |parsed| {
        parsed
            .string("message")
            .or_else(|| parsed.string("Message"))
            .unwrap_or_default()
            .to_owned()
    });
    let hint = match status {
        400 if message.contains("security token") => " Revisa el access key id.",
        403 => " La clave existe pero le falta el permiso ecr:GetAuthorizationToken.",
        _ => "",
    };
    if message.is_empty() {
        format!("ECR respondió {status}.{hint}")
    } else {
        format!("ECR respondió {status}: {message}.{hint}")
    }
}

/// Cadena de derivación de SigV4: fecha → región → servicio → petición.
fn sign(credentials: &EcrCredentials, date: &str, message: &[u8]) -> [u8; 32] {
    let initial = format!("AWS4{}", credentials.secret_access_key);
    let by_date = hmac_sha256(initial.as_bytes(), date.as_bytes());
    let by_region = hmac_sha256(&by_date, credentials.region.as_bytes());
    let by_service = hmac_sha256(&by_region, SERVICE.as_bytes());
    let signing = hmac_sha256(&by_service, b"aws4_request");
    hmac_sha256(&signing, message)
}

/// `20260816T101530Z` en UTC a partir de un epoch. Sin dependencias de fechas:
/// se calcula con el algoritmo civil-from-days de Howard Hinnant.
pub fn amz_date(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let seconds = timestamp % 86_400;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Base64 estándar con relleno, que es como ECR entrega el token.
fn base64_decode(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    for byte in value.bytes() {
        let sextet = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | sextet;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(output)
}

pub fn valid_region(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 24
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn valid_access_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub fn valid_registry_id(value: &str) -> bool {
    value.is_empty() || (value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Nombre de repositorio tal y como lo acepta ECR: minúsculas, dígitos y los
/// separadores `. _ - /`, siempre empezando y terminando en alfanumérico. Se
/// valida antes de firmar porque acaba dentro del cuerpo de la petición.
pub fn valid_repository_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=256).contains(&value.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && !value.contains("..")
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-' | b'/')
        })
}

fn serialize(credentials: &EcrCredentials) -> String {
    format!(
        "access_key_id={}\nsecret_access_key={}\nregion={}\nregistry_id={}\nconnected_at={}\n",
        encode_field(&credentials.access_key_id),
        encode_field(&credentials.secret_access_key),
        encode_field(&credentials.region),
        encode_field(&credentials.registry_id),
        credentials.connected_at,
    )
}

fn parse_credentials(contents: &str) -> Result<EcrCredentials, String> {
    let mut credentials = EcrCredentials::default();
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = decode_field(value)?;
        match key {
            "access_key_id" => credentials.access_key_id = value,
            "secret_access_key" => credentials.secret_access_key = value,
            "region" => credentials.region = value,
            "registry_id" => credentials.registry_id = value,
            "connected_at" => credentials.connected_at = value.parse().unwrap_or_default(),
            _ => {}
        }
    }
    if credentials.access_key_id.is_empty() || credentials.secret_access_key.is_empty() {
        return Err("el archivo de credenciales de ECR está incompleto".to_owned());
    }
    Ok(credentials)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_amazon_dates_in_utc() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_755_302_400), "20250816T000000Z");
        // Un año bisiesto justo en el salto de febrero.
        assert_eq!(amz_date(1_709_164_800), "20240229T000000Z");
    }

    #[test]
    fn derives_the_signature_from_the_aws_test_vector() {
        // Vector oficial de la documentación de SigV4 (servicio y región de la
        // guía), que valida la cadena completa de derivación.
        let credentials = EcrCredentials {
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
            region: "us-east-1".to_owned(),
            ..EcrCredentials::default()
        };
        let key = {
            let initial = format!("AWS4{}", credentials.secret_access_key);
            let by_date = hmac_sha256(initial.as_bytes(), b"20150830");
            let by_region = hmac_sha256(&by_date, b"us-east-1");
            let by_service = hmac_sha256(&by_region, b"iam");
            hmac_sha256(&by_service, b"aws4_request")
        };
        assert_eq!(
            hex(&key),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }

    #[test]
    fn decodes_the_base64_token_from_ecr() {
        assert_eq!(base64_decode("QVdTOnNlY3JldA==").unwrap(), b"AWS:secret");
        assert_eq!(base64_decode("QVdTOmE=").unwrap(), b"AWS:a");
        assert!(base64_decode("no válido").is_none());
    }

    #[test]
    fn masks_the_access_key() {
        assert_eq!(mask_key("AKIAIOSFODNN7EXAMPLE"), "****MPLE");
        assert_eq!(mask_key("abc"), "****");
    }

    #[test]
    fn lists_repositories_in_alphabetical_order() {
        let body = r#"{"repositories":[
            {"repositoryName":"web","repositoryUri":"1.dkr.ecr.us-east-1.amazonaws.com/web"},
            {"repositoryName":"api","repositoryUri":"1.dkr.ecr.us-east-1.amazonaws.com/api"}
        ]}"#;
        assert_eq!(parse_repositories(body).unwrap(), vec!["api", "web"]);
        assert!(parse_repositories("{}").unwrap().is_empty());
    }

    #[test]
    fn expands_every_tag_of_an_image_and_orders_by_push_date() {
        // Una imagen con dos etiquetas, otra más vieja y una huérfana sin
        // ninguna: así es como responde ECR cuando el CI reetiqueta `latest`.
        let body = r#"{"imageDetails":[
            {"imageTags":["latest","sha-b2"],"imagePushedAt":1750000000.25,"imageSizeInBytes":1048576},
            {"imageTags":["sha-a1"],"imagePushedAt":1740000000.0,"imageSizeInBytes":1000},
            {"imagePushedAt":1730000000.0,"imageSizeInBytes":500}
        ]}"#;

        let tags = parse_tags(body).unwrap();
        let names: Vec<&str> = tags.iter().map(|entry| entry.tag.as_str()).collect();
        assert_eq!(names, vec!["latest", "sha-b2", "sha-a1"]);
        assert_eq!(tags[0].pushed_at, 1_750_000_000, "el decimal se trunca");
        assert_eq!(tags[0].size_bytes, 1_048_576);
    }

    #[test]
    fn validates_repository_names() {
        assert!(valid_repository_name("api"));
        assert!(valid_repository_name("equipo/calculator-back"));
        assert!(valid_repository_name("app_1.2"));
        assert!(!valid_repository_name("a"), "ECR exige al menos dos caracteres");
        assert!(!valid_repository_name("API"), "solo minúsculas");
        assert!(!valid_repository_name("/api"));
        assert!(!valid_repository_name("api/"));
        assert!(!valid_repository_name("../etc/passwd"));
        assert!(!valid_repository_name("api\",\"maxResults\":1"));
    }

    #[test]
    fn validates_aws_identifiers() {
        assert!(valid_region("us-east-1"));
        assert!(!valid_region("US-EAST-1"));
        assert!(!valid_region("us east 1"));
        assert!(valid_access_key("AKIAIOSFODNN7EXAMPLE"));
        assert!(!valid_access_key("AKIA;rm -rf /"));
        assert!(valid_registry_id(""));
        assert!(valid_registry_id("123456789012"));
        assert!(!valid_registry_id("no-numerico"));
    }

    #[test]
    fn credentials_round_trip_through_disk_format() {
        let original = EcrCredentials {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
            region: "us-east-1".to_owned(),
            registry_id: "123456789012".to_owned(),
            connected_at: 1_700_000_000,
        };
        let parsed = parse_credentials(&serialize(&original)).unwrap();
        assert_eq!(parsed.secret_access_key, original.secret_access_key);
        assert_eq!(parsed.registry_host(), "123456789012.dkr.ecr.us-east-1.amazonaws.com");
    }
}
