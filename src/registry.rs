//! Búsqueda y etiquetas de Docker Hub.
//!
//! Las respuestas remotas se normalizan aquí a un JSON pequeño y estable para
//! que la interfaz no dependa del formato interno de Docker Hub ni reciba
//! payloads de cientos de kilobytes.

use crate::json::Json;
use crate::net::{fetch, Outbound};
use crate::util::json_string;

const SEARCH_RESULTS: usize = 12;
const TAG_RESULTS: usize = 30;

/// Sugerencias que la interfaz muestra antes de que el usuario escriba nada.
const POPULAR: [(&str, &str, &str); 12] = [
    ("nginx", "nginx", "Servidor web y proxy inverso"),
    ("traefik", "traefikproxy", "Proxy inverso con TLS automático"),
    ("redis", "redis", "Caché y colas en memoria"),
    ("n8nio/n8n", "n8n", "Automatizaciones y workflows"),
    ("minio/minio", "minio", "Almacenamiento compatible con S3"),
    ("grafana/grafana", "grafana", "Dashboards y observabilidad"),
    ("louislam/uptime-kuma", "uptimekuma", "Monitor de uptime"),
    ("portainer/portainer-ce", "portainer", "Gestión visual de Docker"),
    ("metabase/metabase", "metabase", "Business intelligence"),
    ("rabbitmq", "rabbitmq", "Broker de mensajería AMQP"),
    ("getmeili/meilisearch", "meilisearch", "Buscador full-text"),
    ("vaultwarden/server", "bitwarden", "Gestor de contraseñas"),
];

pub fn popular_json() -> String {
    let entries: Vec<String> = POPULAR
        .iter()
        .map(|(image, icon, description)| {
            format!(
                "{{\"name\":{},\"icon\":{},\"description\":{},\"official\":{}}}",
                json_string(image),
                json_string(icon),
                json_string(description),
                !image.contains('/')
            )
        })
        .collect();
    format!("[{}]", entries.join(","))
}

/// `nginx` → `("library", "nginx")`; `bitnami/redis` → `("bitnami", "redis")`.
pub fn split_repository(image: &str) -> Result<(String, String), String> {
    let reference = image.trim().trim_start_matches("docker.io/");
    if reference.is_empty() {
        return Err("indica una imagen".to_owned());
    }
    // Descarta referencias con registro propio: Docker Hub no las conoce.
    if reference.split('/').count() > 2 {
        return Err("solo se pueden explorar imágenes de Docker Hub".to_owned());
    }
    let name_only = reference.split(['@', ':']).next().unwrap_or(reference);
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.'))
    };

    match name_only.split_once('/') {
        Some((namespace, name)) if valid(namespace) && valid(name) => {
            Ok((namespace.to_owned(), name.to_owned()))
        }
        None if valid(name_only) => Ok(("library".to_owned(), name_only.to_owned())),
        _ => Err("nombre de imagen inválido".to_owned()),
    }
}

pub fn search(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.len() < 2 {
        return Ok("[]".to_owned());
    }
    if query.len() > 64
        || !query.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b' ')
        })
    {
        return Err("búsqueda inválida".to_owned());
    }

    let url = format!(
        "https://hub.docker.com/v2/search/repositories/?page_size={SEARCH_RESULTS}&query={}",
        crate::util::encode_field(query)
    );
    let response = fetch(&Outbound::get(url).max_bytes(256 * 1024))?;
    if !response.is_success() {
        return Err(response.error_summary("Docker Hub"));
    }

    let document = Json::parse(&response.body)
        .map_err(|error| format!("respuesta inesperada de Docker Hub: {error}"))?;
    let results = document.array("results").unwrap_or_default();

    let entries: Vec<String> = results
        .iter()
        .take(SEARCH_RESULTS)
        .filter_map(|result| {
            let name = result.string("repo_name")?;
            Some(format!(
                concat!(
                    "{{\"name\":{},\"description\":{},",
                    "\"stars\":{},\"pulls\":{},\"official\":{}}}"
                ),
                json_string(name),
                json_string(
                    result
                        .string("short_description")
                        .unwrap_or_default()
                        .trim()
                ),
                result.number("star_count").unwrap_or(0),
                result.number("pull_count").unwrap_or(0),
                result
                    .get("is_official")
                    .and_then(Json::as_bool)
                    .unwrap_or(false),
            ))
        })
        .collect();

    Ok(format!("[{}]", entries.join(",")))
}

pub fn tags(image: &str) -> Result<String, String> {
    let (namespace, name) = split_repository(image)?;
    let url = format!(
        "https://hub.docker.com/v2/repositories/{namespace}/{name}/tags/?page_size={TAG_RESULTS}&ordering=last_updated"
    );
    let response = fetch(&Outbound::get(url).max_bytes(512 * 1024))?;
    if response.status == 404 {
        return Err(format!("Docker Hub no conoce la imagen {image}"));
    }
    if !response.is_success() {
        return Err(response.error_summary("Docker Hub"));
    }

    let document = Json::parse(&response.body)
        .map_err(|error| format!("respuesta inesperada de Docker Hub: {error}"))?;
    let results = document.array("results").unwrap_or_default();

    let entries: Vec<String> = results
        .iter()
        .take(TAG_RESULTS)
        .filter_map(|result| {
            let tag = result.string("name")?;
            Some(format!(
                "{{\"name\":{},\"size\":{},\"updated\":{},\"digest\":{}}}",
                json_string(tag),
                result.number("full_size").unwrap_or(0),
                json_string(result.string("last_updated").unwrap_or_default()),
                json_string(result.string("digest").unwrap_or_default()),
            ))
        })
        .collect();

    Ok(format!(
        "{{\"image\":{},\"tags\":[{}]}}",
        json_string(&format!(
            "{}{name}",
            if namespace == "library" {
                String::new()
            } else {
                format!("{namespace}/")
            }
        )),
        entries.join(",")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_images_resolve_to_the_library_namespace() {
        assert_eq!(
            split_repository("nginx").unwrap(),
            ("library".to_owned(), "nginx".to_owned())
        );
        assert_eq!(
            split_repository("postgres:17-alpine").unwrap(),
            ("library".to_owned(), "postgres".to_owned())
        );
        assert_eq!(
            split_repository("docker.io/bitnami/redis").unwrap(),
            ("bitnami".to_owned(), "redis".to_owned())
        );
    }

    #[test]
    fn foreign_registries_are_rejected() {
        assert!(split_repository("ghcr.io/isaul19/app:1").is_err());
        assert!(split_repository("").is_err());
        assert!(split_repository("MAYUS/app").is_err());
        assert!(split_repository("../etc/passwd").is_err());
    }

    #[test]
    fn short_queries_do_not_reach_the_network() {
        assert_eq!(search("a").unwrap(), "[]");
        assert!(search("nginx; rm -rf /").is_err());
    }

    #[test]
    fn popular_catalog_is_valid_json() {
        let parsed = Json::parse(&popular_json()).unwrap();
        assert_eq!(parsed.as_array().map(<[_]>::len), Some(POPULAR.len()));
    }
}
