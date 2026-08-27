use crate::auth::{Auth, LoginError};
use crate::docker::DockerClient;
use crate::http::{Request, Response};
use crate::metrics::HostMetrics;
use crate::util::{constant_time_eq, json_string, now_unix, valid_container_ref};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/dist/app.js");
const STYLES_CSS: &str = include_str!("../web/dist/app.css");
const FAVICON_SVG: &str = include_str!("../web/favicon.svg");

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub admin_token: String,
    pub admin_user: String,
    pub admin_password: String,
    pub data_dir: PathBuf,
    pub docker_binary: PathBuf,
    pub workers: usize,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let file_settings = if env::var("TDM_ADMIN_TOKEN").is_ok() {
            HashMap::new()
        } else {
            crate::setup::read_config_file().unwrap_or_default()
        };
        let setting = |key: &str| {
            env::var(key)
                .ok()
                .or_else(|| file_settings.get(key).cloned())
        };

        let admin_token = setting("TDM_ADMIN_TOKEN")
            .ok_or_else(|| "TDM_ADMIN_TOKEN es obligatorio".to_owned())?;
        if admin_token.len() < 32
            || admin_token.len() > 256
            || admin_token.chars().any(char::is_whitespace)
        {
            return Err(
                "TDM_ADMIN_TOKEN debe tener entre 32 y 256 caracteres sin espacios".to_owned(),
            );
        }

        let data_dir = PathBuf::from(
            setting("TDM_DATA_DIR").unwrap_or_else(|| "/var/lib/tinkiva-docker-manager".to_owned()),
        );
        fs::create_dir_all(&data_dir)
            .map_err(|error| format!("no se pudo crear {}: {error}", data_dir.display()))?;
        fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("no se pudo proteger {}: {error}", data_dir.display()))?;
        let data_dir = data_dir
            .canonicalize()
            .map_err(|error| format!("no se pudo resolver {}: {error}", data_dir.display()))?;

        Ok(Self {
            bind: setting("TDM_BIND").unwrap_or_else(|| "127.0.0.1:8787".to_owned()),
            admin_user: setting("TDM_ADMIN_USER").unwrap_or_else(|| "admin".to_owned()),
            admin_password: setting("TDM_ADMIN_PASSWORD").unwrap_or_else(|| admin_token.clone()),
            admin_token,
            data_dir,
            docker_binary: PathBuf::from(
                setting("TDM_DOCKER_BIN").unwrap_or_else(|| "docker".to_owned()),
            ),
            workers: setting("TDM_WORKERS")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(2)
                .clamp(1, 16),
        })
    }
}

pub struct App {
    config: Config,
    docker: DockerClient,
    auth: Auth,
    started_at: u64,
}

impl App {
    pub fn new(config: Config) -> Result<Self, String> {
        let auth = Auth::load(
            config.data_dir.join("auth.conf"),
            &config.admin_user,
            &config.admin_password,
        )?;
        let docker = DockerClient::new(config.docker_binary.clone());
        Ok(Self {
            config,
            docker,
            auth,
            started_at: now_unix(),
        })
    }

    pub fn handle(&self, request: &Request) -> Response {
        let method = if request.method == "HEAD" {
            "GET"
        } else {
            request.method.as_str()
        };

        match (method, request.path.as_str()) {
            ("GET", "/") | ("GET", "/index.html") => return Response::html(INDEX_HTML),
            ("GET", "/app.js") => return Response::javascript(APP_JS),
            ("GET", "/styles.css") => return Response::css(STYLES_CSS),
            ("GET", "/favicon.svg") => return Response::svg(FAVICON_SVG),
            ("GET", "/healthz") => {
                return Response::json(
                    200,
                    format!(
                        "{{\"ok\":true,\"version\":{},\"edition\":\"createapp\",\"started_at\":{}}}",
                        json_string(env!("CARGO_PKG_VERSION")),
                        self.started_at
                    ),
                );
            }
            _ => {}
        }

        if !request.path.starts_with("/api/") {
            return json_error(404, "ruta no encontrada");
        }
        if method == "POST" && request.path == "/api/auth/login" {
            return self.login(request);
        }
        if method == "POST" && request.path == "/api/auth/change-password" {
            return self.change_password(request);
        }
        if !self.is_authorized(request) {
            return Response::json(
                401,
                "{\"error\":\"sesión inválida o cambio de contraseña pendiente\"}".to_owned(),
            )
            .with_header("WWW-Authenticate", "Bearer");
        }

        let segments = path_segments(&request.path);
        match (method, segments.as_slice()) {
            ("GET", ["api", "info"]) => self.info(),
            ("GET", ["api", "system"]) => self.system_metrics(),
            ("GET", ["api", "containers"]) => self.list_containers(),
            ("GET", ["api", "containers", container, "logs"]) => {
                self.container_logs(container, request)
            }
            _ => json_error(404, "endpoint no encontrado en la edición TinkivaCreateApp"),
        }
    }

    fn login(&self, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        match self.auth.login(
            &request.client_ip(),
            field(&fields, "username"),
            field(&fields, "password"),
        ) {
            Ok(session) => Response::json(
                200,
                format!(
                    "{{\"token\":{},\"must_change_password\":{}}}",
                    json_string(&session.token),
                    session.must_change,
                ),
            ),
            Err(LoginError::Blocked(blocked)) => {
                let message = if blocked.day_lock {
                    "Demasiados intentos fallidos. Acceso bloqueado durante 1 día."
                } else {
                    "Usuario o contraseña incorrectos. Intenta de nuevo en 1 minuto."
                };
                Response::json(
                    429,
                    format!(
                        "{{\"error\":{},\"retry_after_seconds\":{},\"locked_for_day\":{}}}",
                        json_string(message),
                        blocked.retry_after_seconds,
                        blocked.day_lock,
                    ),
                )
                .with_header("Retry-After", blocked.retry_after_seconds.to_string())
            }
            Err(LoginError::Internal(error)) => json_error(500, &error),
        }
    }

    fn change_password(&self, request: &Request) -> Response {
        let Some(token) = bearer_token(request) else {
            return json_error(401, "la sesión ya no es válida");
        };
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        match self.auth.change_password(token, field(&fields, "password")) {
            Ok(new_token) => Response::json(
                200,
                format!(
                    "{{\"token\":{},\"must_change_password\":false}}",
                    json_string(&new_token),
                ),
            ),
            Err(error) if error == "la sesión ya no es válida" => json_error(401, &error),
            Err(error) => json_error(422, &error),
        }
    }

    fn is_authorized(&self, request: &Request) -> bool {
        bearer_token(request).is_some_and(|token| {
            constant_time_eq(token, &self.config.admin_token) || self.auth.authorize(token, false)
        })
    }

    fn info(&self) -> Response {
        Response::json(
            200,
            format!(
                concat!(
                    "{{",
                    "\"name\":\"TinkivaCreateApp Monitor\",",
                    "\"version\":{},",
                    "\"edition\":\"createapp\",",
                    "\"mode\":\"read-only\",",
                    "\"started_at\":{},",
                    "\"data_dir\":{},",
                    "\"workers\":{},",
                    "\"docker\":{}",
                    "}}"
                ),
                json_string(env!("CARGO_PKG_VERSION")),
                self.started_at,
                json_string(&self.config.data_dir.to_string_lossy()),
                self.config.workers,
                self.docker.info().to_json(),
            ),
        )
    }

    fn system_metrics(&self) -> Response {
        match HostMetrics::collect() {
            Ok(metrics) => Response::json(200, metrics.to_json()),
            Err(error) => json_error(500, &error),
        }
    }

    fn list_containers(&self) -> Response {
        match self.docker.containers() {
            Ok(containers) => Response::json(
                200,
                format!(
                    "[{}]",
                    containers
                        .iter()
                        .map(|container| container.to_json())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            Err(error) => json_error(503, &error),
        }
    }

    fn container_logs(&self, container: &str, request: &Request) -> Response {
        if !valid_container_ref(container) {
            return json_error(400, "contenedor inválido");
        }
        let tail = request
            .query
            .get("tail")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(300);
        match self.docker.container_logs(container, tail) {
            Ok(logs) => Response::text(200, logs),
            Err(error) => json_error(502, &error),
        }
    }
}

fn path_segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn bearer_token(request: &Request) -> Option<&str> {
    request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
}

fn field<'a>(fields: &'a HashMap<String, String>, name: &str) -> &'a str {
    fields.get(name).map_or("", |value| value.trim())
}

fn json_error(status: u16, message: &str) -> Response {
    Response::json(status, format!("{{\"error\":{}}}", json_string(message)))
}
