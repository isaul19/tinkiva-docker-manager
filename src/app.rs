use crate::docker::{CommandResult, DockerClient};
use crate::http::{Request, Response};
use crate::metrics::{
    collect_processes, processes_to_json, HostMetrics,
};
use crate::model::{Deployment, Project};
use crate::store::Store;
use crate::util::{
    atomic_write, canonical_existing_within, constant_time_eq, json_string, now_unix,
    random_hex, read_env_value, remove_env_key, set_env_value, valid_container_ref,
    valid_db_identifier, valid_display_name, valid_env_key, valid_image_ref, valid_slug,
};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};
use std::time::Instant;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLES_CSS: &str = include_str!("../web/styles.css");
const FAVICON_SVG: &str = include_str!("../web/favicon.svg");

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub admin_token: String,
    pub data_dir: PathBuf,
    pub allowed_root: PathBuf,
    pub docker_binary: PathBuf,
    pub workers: usize,
    pub max_history: usize,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let file_settings = if env::var("TDM_ADMIN_TOKEN").is_ok() {
            HashMap::new()
        } else {
            match crate::setup::read_config_file() {
                Some(settings) => settings,
                None => crate::setup::run_wizard()?,
            }
        };
        let setting = |key: &str| {
            env::var(key)
                .ok()
                .or_else(|| file_settings.get(key).cloned())
        };

        let bind = setting("TDM_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_owned());
        let admin_token = setting("TDM_ADMIN_TOKEN")
            .ok_or_else(|| "TDM_ADMIN_TOKEN es obligatorio".to_owned())?;
        if admin_token.len() < 32 || admin_token.len() > 256 || admin_token.chars().any(char::is_whitespace)
        {
            return Err("TDM_ADMIN_TOKEN debe tener entre 32 y 256 caracteres sin espacios".to_owned());
        }

        let data_dir = PathBuf::from(
            setting("TDM_DATA_DIR")
                .unwrap_or_else(|_| "/var/lib/tinkiva-docker-manager".to_owned()),
        );
        let allowed_root = PathBuf::from(
            setting("TDM_ALLOWED_ROOT").unwrap_or_else(|_| "/opt/tinkiva/apps".to_owned()),
        );
        fs::create_dir_all(&data_dir)
            .map_err(|error| format!("no se pudo crear {}: {error}", data_dir.display()))?;
        fs::create_dir_all(&allowed_root)
            .map_err(|error| format!("no se pudo crear {}: {error}", allowed_root.display()))?;
        fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("no se pudo proteger {}: {error}", data_dir.display()))?;

        let allowed_root = allowed_root
            .canonicalize()
            .map_err(|error| format!("no se pudo resolver {}: {error}", allowed_root.display()))?;
        let data_dir = data_dir
            .canonicalize()
            .map_err(|error| format!("no se pudo resolver {}: {error}", data_dir.display()))?;

        let workers = setting("TDM_WORKERS")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2)
            .clamp(1, 16);
        let max_history = setting("TDM_MAX_HISTORY")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(200)
            .clamp(10, 10_000);

        Ok(Self {
            bind,
            admin_token,
            data_dir,
            allowed_root,
            docker_binary: PathBuf::from(
                setting("TDM_DOCKER_BIN").unwrap_or_else(|_| "docker".to_owned()),
            ),
            workers,
            max_history,
        })
    }
}

pub struct App {
    config: Config,
    store: Store,
    docker: DockerClient,
    deploy_lock: Mutex<()>,
    started_at: u64,
}

impl App {
    pub fn new(config: Config) -> Result<Self, String> {
        let store = Store::load(config.data_dir.join("state.db"), config.max_history)?;
        let docker = DockerClient::new(config.docker_binary.clone());
        Ok(Self {
            config,
            store,
            docker,
            deploy_lock: Mutex::new(()),
            started_at: now_unix(),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
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
                        "{{\"ok\":true,\"version\":{},\"started_at\":{}}}",
                        json_string(env!("CARGO_PKG_VERSION")),
                        self.started_at
                    ),
                );
            }
            _ => {}
        }

        if request.path.starts_with("/hooks/") {
            return self.route_webhook(method, request);
        }

        if !request.path.starts_with("/api/") {
            return json_error(404, "ruta no encontrada");
        }
        if !self.is_authorized(request) {
            return Response::json(401, "{\"error\":\"token inválido\"}".to_owned())
                .with_header("WWW-Authenticate", "Bearer");
        }

        self.route_api(method, request)
    }

    fn route_api(&self, method: &str, request: &Request) -> Response {
        let segments: Vec<&str> = request
            .path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        match (method, segments.as_slice()) {
            ("GET", ["api", "info"]) => self.info(),
            ("GET", ["api", "system"]) => self.system_metrics(),
            ("GET", ["api", "processes"]) => self.list_processes(),
            ("GET", ["api", "containers"]) => self.list_containers(),
            ("GET", ["api", "containers", container, "logs"]) => {
                self.container_logs(container, request)
            }
            ("POST", ["api", "containers", container, action]) => {
                self.container_action(container, action)
            }
            ("GET", ["api", "projects"]) => self.list_projects(),
            ("POST", ["api", "projects"]) => self.create_project(request),
            ("DELETE", ["api", "projects", slug]) => self.delete_project(slug),
            ("GET", ["api", "projects", slug, "logs"]) => {
                self.project_logs(slug, request)
            }
            ("POST", ["api", "projects", slug, "deploy"]) => {
                self.deploy_project(slug, request, "manual")
            }
            ("POST", ["api", "projects", slug, "rollback"]) => {
                self.rollback_project(slug)
            }
            ("GET", ["api", "history"]) => self.history(request),
            ("POST", ["api", "templates", "postgres"]) => {
                self.create_postgres_template(request)
            }
            _ => json_error(404, "endpoint no encontrado"),
        }
    }

    fn route_webhook(&self, method: &str, request: &Request) -> Response {
        let segments: Vec<&str> = request
            .path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let ["hooks", "deploy", slug] = segments.as_slice() else {
            return json_error(404, "webhook no encontrado");
        };
        if method != "POST" {
            return json_error(405, "el webhook requiere POST");
        }

        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "webhook no encontrado"),
            Err(error) => return json_error(500, &error),
        };
        let supplied_token = request
            .header("x-tinkiva-token")
            .or_else(|| bearer_token(request));
        if !supplied_token.is_some_and(|token| constant_time_eq(token, &project.webhook_token)) {
            return json_error(404, "webhook no encontrado");
        }

        self.deploy_project_with_project(project, request, "webhook")
    }

    fn is_authorized(&self, request: &Request) -> bool {
        bearer_token(request)
            .is_some_and(|token| constant_time_eq(token, &self.config.admin_token))
    }

    fn info(&self) -> Response {
        let (projects, deployments) = match self.store.counts() {
            Ok(counts) => counts,
            Err(error) => return json_error(500, &error),
        };
        let docker = self.docker.info();
        Response::json(
            200,
            format!(
                concat!(
                    "{{",
                    "\"name\":\"Tinkiva Docker Manager\",",
                    "\"version\":{},",
                    "\"started_at\":{},",
                    "\"allowed_root\":{},",
                    "\"data_dir\":{},",
                    "\"workers\":{},",
                    "\"projects\":{},",
                    "\"deployments\":{},",
                    "\"docker\":{}",
                    "}}"
                ),
                json_string(env!("CARGO_PKG_VERSION")),
                self.started_at,
                json_string(&self.config.allowed_root.to_string_lossy()),
                json_string(&self.config.data_dir.to_string_lossy()),
                self.config.workers,
                projects,
                deployments,
                docker.to_json(),
            ),
        )
    }

    fn system_metrics(&self) -> Response {
        match HostMetrics::collect() {
            Ok(metrics) => Response::json(200, metrics.to_json()),
            Err(error) => json_error(500, &error),
        }
    }

    fn list_processes(&self) -> Response {
        match collect_processes() {
            Ok(entries) => Response::json(200, processes_to_json(&entries)),
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

    fn container_action(&self, container: &str, action: &str) -> Response {
        match self.docker.container_action(container, action) {
            Ok(result) if result.success => Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"message\":{}}}",
                    json_string(&result.summary())
                ),
            ),
            Ok(result) => json_error(502, &result.summary()),
            Err(error) => json_error(400, &error),
        }
    }

    fn list_projects(&self) -> Response {
        match self.store.projects() {
            Ok(projects) => Response::json(
                200,
                format!(
                    "[{}]",
                    projects
                        .iter()
                        .map(|project| project.to_json(true))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            Err(error) => json_error(500, &error),
        }
    }

    fn create_project(&self, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        let compose_input = field(&fields, "compose_file");

        if !valid_slug(slug) {
            return json_error(422, "slug inválido; usa minúsculas, números y guiones");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }
        if compose_input.is_empty() {
            return json_error(422, "compose_file es obligatorio");
        }

        let compose_file = match self.resolve_existing_path(compose_input) {
            Ok(path) if path.is_file() => path,
            Ok(_) => return json_error(422, "compose_file no es un archivo"),
            Err(error) => return json_error(422, &error),
        };
        let env_file = match optional_field(&fields, "env_file") {
            Some(value) => match self.resolve_existing_path(value) {
                Ok(path) if path.is_file() => Some(path),
                Ok(_) => return json_error(422, "env_file no es un archivo"),
                Err(error) => return json_error(422, &error),
            },
            None => None,
        };
        let image_env = optional_field(&fields, "image_env").map(str::to_owned);
        if image_env.as_deref().is_some_and(|value| !valid_env_key(value)) {
            return json_error(422, "image_env debe parecerse a APP_IMAGE");
        }
        if image_env.is_some() && env_file.is_none() {
            return json_error(422, "image_env requiere env_file");
        }
        let branch = optional_field(&fields, "branch").map(str::to_owned);
        if branch.as_deref().is_some_and(|value| !valid_branch(value)) {
            return json_error(422, "rama inválida");
        }

        let webhook_token = match optional_field(&fields, "webhook_token") {
            Some(token) if valid_webhook_token(token) => token.to_owned(),
            Some(_) => return json_error(422, "webhook_token inválido"),
            None => match random_hex(24) {
                Ok(token) => token,
                Err(error) => return json_error(500, &format!("no se pudo crear token: {error}")),
            },
        };

        if let Err(error) = self.docker.validate_compose(&compose_file) {
            return json_error(422, &format!("Compose inválido: {error}"));
        }

        let current_image = match (&env_file, &image_env) {
            (Some(path), Some(key)) => read_env_value(path, key).unwrap_or_default(),
            _ => None,
        };
        let project = Project {
            slug: slug.to_owned(),
            name: name.trim().to_owned(),
            compose_file,
            env_file,
            image_env,
            branch,
            webhook_token,
            current_image,
            created_at: now_unix(),
        };

        match self.store.add_project(project.clone()) {
            Ok(()) => Response::json(201, project.to_json(true)),
            Err(error) => json_error(409, &error),
        }
    }

    fn delete_project(&self, slug: &str) -> Response {
        if !valid_slug(slug) {
            return json_error(400, "slug inválido");
        }
        match self.store.remove_project(slug) {
            Ok(true) => Response::json(
                200,
                "{\"ok\":true,\"message\":\"Proyecto desregistrado; no se eliminaron archivos ni contenedores.\"}".to_owned(),
            ),
            Ok(false) => json_error(404, "proyecto no encontrado"),
            Err(error) => json_error(500, &error),
        }
    }

    fn project_logs(&self, slug: &str, request: &Request) -> Response {
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "proyecto no encontrado"),
            Err(error) => return json_error(500, &error),
        };
        let tail = request
            .query
            .get("tail")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(300);
        match self.docker.compose_logs(&project, tail) {
            Ok(logs) => Response::text(200, logs),
            Err(error) => json_error(502, &error),
        }
    }

    fn deploy_project(&self, slug: &str, request: &Request, trigger: &str) -> Response {
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "proyecto no encontrado"),
            Err(error) => return json_error(500, &error),
        };
        self.deploy_project_with_project(project, request, trigger)
    }

    fn deploy_project_with_project(
        &self,
        project: Project,
        request: &Request,
        trigger: &str,
    ) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let image = optional_field(&fields, "image").map(str::to_owned);
        if image.as_deref().is_some_and(|value| !valid_image_ref(value)) {
            return json_error(422, "referencia de imagen inválida");
        }
        let branch = optional_field(&fields, "branch").map(str::to_owned);
        if branch.as_deref().is_some_and(|value| !valid_branch(value)) {
            return json_error(422, "rama inválida");
        }
        let commit = optional_field(&fields, "commit").map(str::to_owned);
        if commit.as_deref().is_some_and(|value| !valid_commit(value)) {
            return json_error(422, "commit inválido");
        }

        match self.perform_deploy(project, image, branch, commit, trigger) {
            Ok(outcome) => Response::json(
                if outcome.success { 200 } else { 502 },
                outcome.deployment.to_json(),
            ),
            Err(error) => json_error(error.status, &error.message),
        }
    }

    fn rollback_project(&self, slug: &str) -> Response {
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "proyecto no encontrado"),
            Err(error) => return json_error(500, &error),
        };
        let target = match self.store.rollback_target(slug) {
            Ok(Some(image)) => image,
            Ok(None) => return json_error(409, "no existe una imagen anterior para rollback"),
            Err(error) => return json_error(500, &error),
        };

        match self.perform_deploy(
            project.clone(),
            Some(target),
            project.branch.clone(),
            None,
            "rollback",
        ) {
            Ok(outcome) => Response::json(
                if outcome.success { 200 } else { 502 },
                outcome.deployment.to_json(),
            ),
            Err(error) => json_error(error.status, &error.message),
        }
    }

    fn perform_deploy(
        &self,
        project: Project,
        image: Option<String>,
        branch: Option<String>,
        commit: Option<String>,
        trigger: &str,
    ) -> Result<DeployOutcome, ApiError> {
        if let Some(expected_branch) = &project.branch {
            if branch.as_deref() != Some(expected_branch.as_str()) {
                return Err(ApiError::new(
                    403,
                    format!("este proyecto solo acepta la rama {expected_branch}"),
                ));
            }
        }

        let _deployment_guard = match self.deploy_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => {
                return Err(ApiError::new(
                    429,
                    "ya hay otro deployment en ejecución; vuelve a intentarlo",
                ));
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(ApiError::new(500, "el bloqueo de deployments falló"));
            }
        };

        let started = Instant::now();
        let mut previous_image = project.current_image.clone();
        let mut env_target: Option<(PathBuf, String)> = None;

        if let Some(new_image) = &image {
            let (Some(env_file), Some(image_env)) = (&project.env_file, &project.image_env) else {
                return Err(ApiError::new(
                    422,
                    "el proyecto no tiene env_file e image_env configurados",
                ));
            };
            let env_file = canonical_existing_within(&self.config.allowed_root, env_file)
                .map_err(|error| ApiError::new(422, error))?;
            previous_image = read_env_value(&env_file, image_env)
                .map_err(|error| ApiError::new(500, format!("no se pudo leer .env: {error}")))?
                .or(previous_image);
            set_env_value(&env_file, image_env, new_image)
                .map_err(|error| ApiError::new(500, format!("no se pudo actualizar .env: {error}")))?;
            env_target = Some((env_file, image_env.clone()));
        }

        let effective_image = image.clone().or_else(|| previous_image.clone());
        let command = self.docker.deploy(&project);
        let (success, mut message, command_duration) = match command {
            Ok(result) => (result.success, deployment_message(&result), result.duration_ms),
            Err(error) => (false, error, started.elapsed().as_millis()),
        };

        if success {
            self.store
                .update_current_image(&project.slug, effective_image.clone())
                .map_err(|error| {
                    ApiError::new(
                        500,
                        format!("deployment aplicado, pero no se guardó su estado: {error}"),
                    )
                })?;
        } else if let Some((env_file, image_env)) = &env_target {
            let restored = match &previous_image {
                Some(previous) => set_env_value(env_file, image_env, previous),
                None => remove_env_key(env_file, image_env),
            };
            match restored {
                Ok(()) => match self.docker.deploy(&project) {
                    Ok(rollback) if rollback.success => {
                        message.push_str(" | Restauración automática completada.");
                        let _ = self
                            .store
                            .update_current_image(&project.slug, previous_image.clone());
                    }
                    Ok(rollback) => {
                        message.push_str(" | La restauración automática también falló: ");
                        message.push_str(&rollback.summary());
                    }
                    Err(error) => {
                        message.push_str(" | No se pudo ejecutar la restauración automática: ");
                        message.push_str(&error);
                    }
                },
                Err(error) => {
                    message.push_str(" | No se pudo restaurar el archivo .env: ");
                    message.push_str(&error.to_string());
                }
            }
        }

        let deployment = Deployment {
            id: 0,
            project: project.slug,
            created_at: now_unix(),
            status: if success { "success" } else { "failed" }.to_owned(),
            branch,
            commit,
            image: effective_image,
            previous_image,
            message,
            duration_ms: command_duration.max(started.elapsed().as_millis()),
            trigger: trigger.to_owned(),
        };
        let deployment = self
            .store
            .append_deployment(deployment)
            .map_err(|error| ApiError::new(500, error))?;

        Ok(DeployOutcome {
            success,
            deployment,
        })
    }

    fn history(&self, request: &Request) -> Response {
        let project = request.query.get("project").map(String::as_str);
        let limit = request
            .query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(100);
        match self.store.history(project, limit) {
            Ok(deployments) => Response::json(
                200,
                format!(
                    "[{}]",
                    deployments
                        .iter()
                        .map(Deployment::to_json)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            Err(error) => json_error(500, &error),
        }
    }

    fn create_postgres_template(&self, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        let database = field(&fields, "database");
        let username = field(&fields, "username");

        if !valid_slug(slug) {
            return json_error(422, "slug inválido");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }
        if !valid_db_identifier(database) || !valid_db_identifier(username) {
            return json_error(422, "database o username inválido");
        }

        let password = match optional_field(&fields, "password") {
            Some(password) if valid_database_password(password) => password.to_owned(),
            Some(_) => {
                return json_error(
                    422,
                    "la contraseña debe tener 12-128 caracteres seguros sin espacios",
                );
            }
            None => match random_hex(24) {
                Ok(password) => password,
                Err(error) => return json_error(500, &format!("no se pudo crear contraseña: {error}")),
            },
        };
        let published_port = match optional_field(&fields, "published_port") {
            Some(value) => match value.parse::<u16>() {
                Ok(port) if port > 0 => Some(port),
                _ => return json_error(422, "published_port inválido"),
            },
            None => None,
        };
        let memory_mb = optional_field(&fields, "memory_mb")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(512)
            .clamp(128, 8192);

        let directory = self.config.allowed_root.join(slug);
        if directory.exists() {
            return json_error(409, "el directorio del template ya existe");
        }
        if let Err(error) = fs::create_dir(&directory) {
            return json_error(500, &format!("no se pudo crear el directorio: {error}"));
        }
        if let Err(error) = fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)) {
            let _ = fs::remove_dir(&directory);
            return json_error(500, &format!("no se pudo proteger el directorio: {error}"));
        }

        let compose_file = directory.join("compose.yaml");
        let env_file = directory.join(".env");
        let compose = postgres_compose(slug, published_port);
        let env_contents = format!(
            "POSTGRES_DB={database}\nPOSTGRES_USER={username}\nPOSTGRES_PASSWORD={password}\nPOSTGRES_MEMORY_LIMIT={memory_mb}m\n"
        );
        if let Err(error) = atomic_write(&compose_file, compose.as_bytes(), 0o640) {
            let _ = fs::remove_dir_all(&directory);
            return json_error(500, &format!("no se pudo escribir Compose: {error}"));
        }
        if let Err(error) = atomic_write(&env_file, env_contents.as_bytes(), 0o600) {
            let _ = fs::remove_dir_all(&directory);
            return json_error(500, &format!("no se pudo escribir .env: {error}"));
        }
        if let Err(error) = self.docker.ensure_network("tinkiva") {
            let _ = fs::remove_dir_all(&directory);
            return json_error(502, &format!("no se pudo crear la red tinkiva: {error}"));
        }
        if let Err(error) = self.docker.validate_compose(&compose_file) {
            let _ = fs::remove_dir_all(&directory);
            return json_error(422, &format!("el template Compose no es válido: {error}"));
        }

        let project = Project {
            slug: slug.to_owned(),
            name: name.trim().to_owned(),
            compose_file,
            env_file: Some(env_file),
            image_env: None,
            branch: None,
            webhook_token: match random_hex(24) {
                Ok(token) => token,
                Err(error) => {
                    let _ = fs::remove_dir_all(&directory);
                    return json_error(500, &format!("no se pudo crear token: {error}"));
                }
            },
            current_image: Some("postgres:17-alpine".to_owned()),
            created_at: now_unix(),
        };
        if let Err(error) = self.store.add_project(project.clone()) {
            let _ = fs::remove_dir_all(&directory);
            return json_error(409, &error);
        }

        let outcome = match self.perform_deploy(project.clone(), None, None, None, "template") {
            Ok(outcome) => outcome,
            Err(error) => {
                return json_error(
                    error.status,
                    &format!(
                        "PostgreSQL fue registrado, pero no se pudo desplegar: {}. Contraseña generada: {}",
                        error.message, password
                    ),
                );
            }
        };
        let connection_host = format!("{slug}-postgres");
        let connection_uri = format!(
            "postgresql://{username}:{password}@{connection_host}:5432/{database}"
        );
        Response::json(
            if outcome.success { 201 } else { 502 },
            format!(
                concat!(
                    "{{",
                    "\"project\":{},",
                    "\"deployment\":{},",
                    "\"password\":{},",
                    "\"connection_uri\":{},",
                    "\"published_port\":{}",
                    "}}"
                ),
                project.to_json(true),
                outcome.deployment.to_json(),
                json_string(&password),
                json_string(&connection_uri),
                published_port.map_or_else(|| "null".to_owned(), |port| port.to_string()),
            ),
        )
    }

    fn resolve_existing_path(&self, input: &str) -> Result<PathBuf, String> {
        let path = Path::new(input);
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.config.allowed_root.join(path)
        };
        canonical_existing_within(&self.config.allowed_root, &candidate)
    }
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

fn optional_field<'a>(fields: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    let value = field(fields, name);
    (!value.is_empty()).then_some(value)
}

fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/')
        })
}

fn valid_commit(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_webhook_token(value: &str) -> bool {
    (24..=256).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_database_password(value: &str) -> bool {
    (12..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
        })
}

fn deployment_message(result: &CommandResult) -> String {
    if result.success {
        let details = result.summary();
        if details == "el comando no devolvió detalles" {
            "Deployment completado.".to_owned()
        } else {
            format!("Deployment completado. {details}")
        }
    } else {
        format!("Deployment fallido: {}", result.summary())
    }
}

fn postgres_compose(slug: &str, published_port: Option<u16>) -> String {
    let ports = published_port.map_or_else(String::new, |port| {
        format!("    ports:\n      - \"127.0.0.1:{port}:5432\"\n")
    });
    format!(
        concat!(
            "services:\n",
            "  postgres:\n",
            "    image: postgres:17-alpine\n",
            "    container_name: {slug}-postgres\n",
            "    restart: unless-stopped\n",
            "    environment:\n",
            "      POSTGRES_DB: ${{POSTGRES_DB}}\n",
            "      POSTGRES_USER: ${{POSTGRES_USER}}\n",
            "      POSTGRES_PASSWORD: ${{POSTGRES_PASSWORD}}\n",
            "    env_file:\n",
            "      - .env\n",
            "{ports}",
            "    volumes:\n",
            "      - postgres_data:/var/lib/postgresql/data\n",
            "    networks:\n",
            "      - tinkiva\n",
            "    mem_limit: ${{POSTGRES_MEMORY_LIMIT:-512m}}\n",
            "    shm_size: 128m\n",
            "    healthcheck:\n",
            "      test: [\"CMD-SHELL\", \"pg_isready -U $$POSTGRES_USER -d $$POSTGRES_DB\"]\n",
            "      interval: 10s\n",
            "      timeout: 5s\n",
            "      retries: 5\n",
            "      start_period: 20s\n",
            "    security_opt:\n",
            "      - no-new-privileges:true\n",
            "\n",
            "volumes:\n",
            "  postgres_data:\n",
            "\n",
            "networks:\n",
            "  tinkiva:\n",
            "    external: true\n"
        ),
        slug = slug,
        ports = ports,
    )
}

fn json_error(status: u16, message: &str) -> Response {
    Response::json(status, format!("{{\"error\":{}}}", json_string(message)))
}

struct DeployOutcome {
    success: bool,
    deployment: Deployment,
}

struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_are_conservative() {
        assert!(valid_branch("main"));
        assert!(valid_branch("release/uat-1"));
        assert!(!valid_branch("../main"));
        assert!(!valid_branch("feature con espacios"));
    }

    #[test]
    fn postgres_template_does_not_publish_without_port() {
        let compose = postgres_compose("demo", None);
        assert!(!compose.contains("ports:"));
        assert!(compose.contains("demo-postgres"));
    }
}
