use crate::docker::DockerClient;
use crate::git::GitClient;
use crate::github::GitHub;
use crate::http::{Request, Response};
use crate::json::Json;
use crate::metrics::{collect_processes, processes_to_json, HostMetrics};
use crate::model::{Deployment, Project, KIND_DATABASE, KIND_IMAGE, KIND_REPOSITORY};
use crate::proc::CommandResult;
use crate::store::Store;
use crate::templates::{self, GeneratedResource};
use crate::util::{
    atomic_write, canonical_existing_within, constant_time_eq, json_string, now_unix, random_hex,
    read_env_value, remove_env_key, set_env_value, valid_container_ref, valid_db_identifier,
    valid_display_name, valid_env_key, valid_image_ref, valid_slug,
};
use crate::{net, registry};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};
use std::time::Instant;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/dist/app.js");
const STYLES_CSS: &str = include_str!("../web/dist/app.css");
const FAVICON_SVG: &str = include_str!("../web/favicon.svg");

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub admin_token: String,
    pub data_dir: PathBuf,
    pub allowed_root: PathBuf,
    pub docker_binary: PathBuf,
    pub git_binary: PathBuf,
    pub workers: usize,
    pub max_history: usize,
    pub poll_interval_seconds: u64,
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

        let bind = setting("TDM_BIND").unwrap_or_else(|| "127.0.0.1:8787".to_owned());
        let admin_token = setting("TDM_ADMIN_TOKEN")
            .ok_or_else(|| "TDM_ADMIN_TOKEN es obligatorio".to_owned())?;
        if admin_token.len() < 32 || admin_token.len() > 256 || admin_token.chars().any(char::is_whitespace)
        {
            return Err("TDM_ADMIN_TOKEN debe tener entre 32 y 256 caracteres sin espacios".to_owned());
        }

        let data_dir = PathBuf::from(
            setting("TDM_DATA_DIR")
                .unwrap_or_else(|| "/var/lib/tinkiva-docker-manager".to_owned()),
        );
        let allowed_root = PathBuf::from(
            setting("TDM_ALLOWED_ROOT").unwrap_or_else(|| "/opt/tinkiva/apps".to_owned()),
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
        let poll_interval_seconds = setting("TDM_POLL_INTERVAL_SECONDS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60)
            .clamp(30, 86_400);
        Ok(Self {
            bind,
            admin_token,
            data_dir,
            allowed_root,
            docker_binary: PathBuf::from(
                setting("TDM_DOCKER_BIN").unwrap_or_else(|| "docker".to_owned()),
            ),
            git_binary: PathBuf::from(setting("TDM_GIT_BIN").unwrap_or_else(|| "git".to_owned())),
            workers,
            max_history,
            poll_interval_seconds,
        })
    }
}

/// Herramientas externas detectadas al arrancar. La interfaz las usa para
/// desactivar funciones en lugar de dejar que fallen a mitad de camino.
#[derive(Clone, Copy, Debug)]
struct Capabilities {
    curl: bool,
    openssl: bool,
    git: bool,
}

impl Capabilities {
    fn to_json(self) -> String {
        format!(
            "{{\"curl\":{},\"openssl\":{},\"git\":{}}}",
            self.curl, self.openssl, self.git
        )
    }
}

pub struct App {
    config: Config,
    store: Store,
    docker: DockerClient,
    git: GitClient,
    github: GitHub,
    capabilities: Capabilities,
    deploy_lock: Mutex<()>,
    started_at: u64,
}

impl App {
    pub fn new(config: Config) -> Result<Self, String> {
        let store = Store::load(config.data_dir.join("state.db"), config.max_history)?;
        let github = GitHub::load(config.data_dir.join("github.json"))?;
        let docker = DockerClient::new(config.docker_binary.clone());
        let git = GitClient::new(config.git_binary.clone());
        let capabilities = Capabilities {
            curl: net::curl_available(),
            openssl: net::openssl_available(),
            git: git.available(),
        };

        Ok(Self {
            config,
            store,
            docker,
            git,
            github,
            capabilities,
            deploy_lock: Mutex::new(()),
            started_at: now_unix(),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Ejecuta una ronda secuencial de polling. Solo conserva un proyecto a la
    /// vez y delega HTTP/registry a procesos cortos para mantener baja la RAM.
    pub fn poll_deployments(&self) {
        let projects = match self.store.projects() {
            Ok(projects) => projects,
            Err(error) => {
                eprintln!("watcher: no se pudieron leer proyectos: {error}");
                return;
            }
        };

        for project in projects.into_iter().filter(|project| project.auto_deploy) {
            let changed = if project.kind == KIND_REPOSITORY {
                self.repository_changed(&project)
            } else if project.kind == KIND_IMAGE {
                self.registry_image_changed(&project)
            } else {
                continue;
            };

            match changed {
                Ok(Some(revision)) => {
                    let trigger = if project.kind == KIND_REPOSITORY {
                        "github-poll"
                    } else {
                        "registry-poll"
                    };
                    match self.perform_deploy(
                        project.clone(),
                        None,
                        project.branch.clone(),
                        (trigger == "github-poll").then_some(revision.clone()),
                        trigger,
                    ) {
                        Ok(outcome) if outcome.success && trigger == "registry-poll" => {
                            if let Err(error) = self.store.update_source_revision(&project.slug, revision) {
                                eprintln!("watcher {}: no se guardó el digest: {error}", project.slug);
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            if error.status != 429 {
                                eprintln!("watcher {}: {}", project.slug, error.message);
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("watcher {}: {error}", project.slug),
            }
        }
    }

    fn repository_changed(&self, project: &Project) -> Result<Option<String>, String> {
        let repository = project.repository.as_deref().ok_or("falta el repositorio")?;
        let installation_id = project.installation_id.ok_or("falta installation_id")?;
        let branch = project.branch.as_deref().unwrap_or("main");
        let remote = self
            .github
            .branch_commit(installation_id, repository, branch)?;
        Ok((project.source_revision.as_deref() != Some(remote.as_str())).then_some(remote))
    }

    fn registry_image_changed(&self, project: &Project) -> Result<Option<String>, String> {
        let image = project.current_image.as_deref().ok_or("falta la imagen")?;
        let pull = self.docker.pull_image(image)?;
        if !pull.success {
            return Err(format!("no se pudo consultar {image}: {}", pull.summary()));
        }
        let after = self
            .docker
            .image_revision(image)?
            .ok_or_else(|| format!("Docker no encontró {image} después del pull"))?;
        Ok((project.source_revision.as_deref() != Some(after.as_str())).then_some(after))
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

        // Los retornos del navegador desde GitHub no pueden llevar cabecera
        // Authorization: se validan con un nonce de un solo uso.
        if request.path.starts_with("/github/") {
            return self.route_github_return(method, request);
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
        let segments = path_segments(&request.path);

        match (method, segments.as_slice()) {
            ("GET", ["api", "info"]) => self.info(),
            ("GET", ["api", "catalog"]) => self.catalog(),
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
            ("DELETE", ["api", "projects", slug]) => self.delete_project(slug, request),
            ("GET", ["api", "projects", slug, "logs"]) => self.project_logs(slug, request),
            ("POST", ["api", "projects", slug, "deploy"]) => {
                self.deploy_project(slug, request, "manual")
            }
            ("POST", ["api", "projects", slug, "rollback"]) => self.rollback_project(slug),
            ("GET", ["api", "history"]) => self.history(request),

            ("GET", ["api", "registry", "search"]) => self.registry_search(request),
            ("GET", ["api", "registry", "tags"]) => self.registry_tags(request),

            ("GET", ["api", "github"]) => self.github_status(request),
            ("DELETE", ["api", "github"]) => self.github_disconnect(),
            ("POST", ["api", "github", "manifest"]) => self.github_manifest(request),
            ("POST", ["api", "github", "manual"]) => self.github_manual(request),
            ("POST", ["api", "github", "install"]) => self.github_install_url(),
            ("GET", ["api", "github", "installations"]) => self.github_installations(),
            ("GET", ["api", "github", "repositories"]) => self.github_repositories(request),
            ("GET", ["api", "github", "branches"]) => self.github_branches(request),

            ("POST", ["api", "resources", "database"]) => self.create_database(request, None),
            ("POST", ["api", "resources", "image"]) => self.create_image_service(request),
            ("POST", ["api", "resources", "repository"]) => self.create_repository_service(request),
            // Ruta histórica: equivale a crear una base de datos PostgreSQL.
            ("POST", ["api", "templates", "postgres"]) => {
                self.create_database(request, Some("postgres"))
            }
            _ => json_error(404, "endpoint no encontrado"),
        }
    }

    // ── Autenticación ──────────────────────────────────────────────────────

    fn is_authorized(&self, request: &Request) -> bool {
        bearer_token(request)
            .is_some_and(|token| constant_time_eq(token, &self.config.admin_token))
    }

    /// URL con la que **el navegador** ve el panel, deducida de la cabecera `Host`.
    /// Vale que sea `localhost`: los retornos desde GitHub los hace el navegador.
    fn panel_url(&self, request: &Request) -> String {
        let host = request
            .header("host")
            .filter(|host| valid_host(host))
            .unwrap_or("127.0.0.1:8787");
        let scheme = if request
            .header("x-forwarded-proto")
            .is_some_and(|value| value.eq_ignore_ascii_case("https"))
        {
            "https"
        } else {
            "http"
        };
        format!("{scheme}://{host}")
    }

    // ── Información general ────────────────────────────────────────────────

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
                    "\"capabilities\":{},",
                    "\"github_connected\":{},",
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
                self.capabilities.to_json(),
                self.github.is_connected(),
                docker.to_json(),
            ),
        )
    }

    /// Catálogo estático que alimenta el diálogo «Añadir recurso».
    fn catalog(&self) -> Response {
        Response::json(
            200,
            format!(
                "{{\"engines\":{},\"popular_images\":{},\"capabilities\":{},\"allowed_root\":{}}}",
                templates::engines_json(),
                registry::popular_json(),
                self.capabilities.to_json(),
                json_string(&self.config.allowed_root.to_string_lossy()),
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

    // ── Contenedores ───────────────────────────────────────────────────────

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

    // ── Proyectos ──────────────────────────────────────────────────────────

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
            env_file,
            image_env,
            branch,
            current_image,
            ..Project::compose(
                slug.to_owned(),
                name.trim().to_owned(),
                compose_file,
                webhook_token,
                now_unix(),
            )
        };

        match self.store.add_project(project.clone()) {
            Ok(()) => Response::json(201, project.to_json(true)),
            Err(error) => json_error(409, &error),
        }
    }

    fn delete_project(&self, slug: &str, request: &Request) -> Response {
        if !valid_slug(slug) {
            return json_error(400, "slug inválido");
        }
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "proyecto no encontrado"),
            Err(error) => return json_error(500, &error),
        };

        // `remove` decide cuánto se destruye: por omisión solo se desregistra.
        let remove = request.query.get("remove").map_or("none", String::as_str);
        let mut notes = Vec::new();

        if matches!(remove, "stack" | "all") {
            match self.docker.compose_down(&project, remove == "all") {
                Ok(result) if result.success => notes.push("contenedores detenidos"),
                Ok(_) => notes.push("no se pudieron detener todos los contenedores"),
                Err(_) => notes.push("no se pudo hablar con Docker"),
            }
        }

        if remove == "all" {
            // Solo se borra el directorio que el propio panel generó para el recurso.
            let expected = self.config.allowed_root.join(slug);
            let owned = project
                .directory()
                .and_then(|directory| directory.canonicalize().ok())
                .is_some_and(|directory| directory == expected);
            if owned && expected.exists() {
                match fs::remove_dir_all(&expected) {
                    Ok(()) => notes.push("archivos eliminados"),
                    Err(_) => notes.push("no se pudieron eliminar los archivos"),
                }
            } else {
                notes.push("los archivos quedaron intactos por estar fuera del recurso");
            }
        }

        match self.store.remove_project(slug) {
            Ok(true) => {
                let detail = if notes.is_empty() {
                    "Proyecto desregistrado; no se eliminaron archivos ni contenedores.".to_owned()
                } else {
                    format!("Proyecto desregistrado ({}).", notes.join(", "))
                };
                Response::json(
                    200,
                    format!("{{\"ok\":true,\"message\":{}}}", json_string(&detail)),
                )
            }
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

    // ── Despliegues ────────────────────────────────────────────────────────

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

    /// Deja el clon de git en la punta de la rama antes de reconstruir la imagen.
    fn sync_repository(&self, project: &Project) -> Result<Option<String>, ApiError> {
        let (Some(repository), Some(installation_id)) =
            (project.repository.as_deref(), project.installation_id)
        else {
            return Ok(None);
        };
        let directory = project
            .directory()
            .ok_or_else(|| ApiError::new(500, "el proyecto no tiene directorio"))?
            .join("repo");
        if !directory.is_dir() {
            return Err(ApiError::new(
                422,
                format!("falta el clon de {repository}; vuelve a crear el recurso"),
            ));
        }

        let branch = project.branch.as_deref().unwrap_or("main");
        let token = self
            .github
            .installation_token(installation_id)
            .map_err(|error| ApiError::new(502, error))?;
        let result = self
            .git
            .update_repository(&directory, branch, &token)
            .map_err(|error| ApiError::new(502, error))?;
        if !result.success {
            return Err(ApiError::new(
                502,
                format!(
                    "no se pudo actualizar {repository}: {}",
                    result.redacted_summary(&[&token])
                ),
            ));
        }
        Ok(self.git.head_commit(&directory))
    }

    fn perform_deploy(
        &self,
        project: Project,
        image: Option<String>,
        branch: Option<String>,
        commit: Option<String>,
        trigger: &str,
    ) -> Result<DeployOutcome, ApiError> {
        // Un proyecto atado a una rama solo se despliega desde ella. Si no se
        // indica ninguna se usa la configurada, salvo en webhooks, donde exigimos
        // que el emisor la declare explícitamente.
        let branch = match (branch, project.branch.as_deref()) {
            (Some(supplied), Some(expected)) if supplied != expected => {
                return Err(ApiError::new(
                    403,
                    format!("este proyecto solo acepta la rama {expected}"),
                ));
            }
            (None, Some(expected)) if trigger == "webhook" => {
                return Err(ApiError::new(
                    403,
                    format!("el webhook debe indicar la rama {expected}"),
                ));
            }
            (None, Some(expected)) => Some(expected.to_owned()),
            (supplied, _) => supplied,
        };

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

        let synced_commit = if project.kind == KIND_REPOSITORY {
            self.sync_repository(&project)?
        } else {
            None
        };
        let commit = commit.or(synced_commit);

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
        let command = if trigger == "registry-poll" {
            self.docker.deploy_pulled(&project)
        } else {
            self.docker.deploy(&project)
        };
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
            if project.kind == KIND_REPOSITORY {
                if let Some(revision) = commit.clone() {
                    self.store
                        .update_source_revision(&project.slug, revision)
                        .map_err(|error| {
                            ApiError::new(
                                500,
                                format!("deployment aplicado, pero no se guardó su commit: {error}"),
                            )
                        })?;
                }
            }
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

    // ── Docker Hub ─────────────────────────────────────────────────────────

    fn registry_search(&self, request: &Request) -> Response {
        if !self.capabilities.curl {
            return json_error(503, "curl no está instalado; no se puede consultar Docker Hub");
        }
        let query = request.query.get("q").map_or("", String::as_str);
        match registry::search(query) {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(502, &error),
        }
    }

    fn registry_tags(&self, request: &Request) -> Response {
        if !self.capabilities.curl {
            return json_error(503, "curl no está instalado; no se puede consultar Docker Hub");
        }
        let image = request.query.get("image").map_or("", String::as_str);
        match registry::tags(image) {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(502, &error),
        }
    }

    // ── GitHub App ─────────────────────────────────────────────────────────

    fn github_status(&self, request: &Request) -> Response {
        let panel_url = self.panel_url(request);
        match self.github.status_json(&panel_url) {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(500, &error),
        }
    }

    fn github_disconnect(&self) -> Response {
        match self.github.disconnect() {
            Ok(()) => Response::json(
                200,
                "{\"ok\":true,\"message\":\"GitHub App desconectada del panel. Recuerda borrarla también en GitHub.\"}"
                    .to_owned(),
            ),
            Err(error) => json_error(500, &error),
        }
    }

    /// Devuelve el manifiesto y el destino al que el navegador debe hacer POST.
    fn github_manifest(&self, request: &Request) -> Response {
        if !self.capabilities.curl || !self.capabilities.openssl {
            return json_error(
                503,
                "GitHub necesita curl y openssl instalados en el servidor",
            );
        }

        let panel_url = self.panel_url(request);

        let suffix = match random_hex(3) {
            Ok(suffix) => suffix,
            Err(error) => return json_error(500, &format!("no se pudo generar sufijo: {error}")),
        };
        let state = match self.github.issue_nonce() {
            Ok(state) => state,
            Err(error) => return json_error(500, &error),
        };

        Response::json(
            200,
            format!(
                concat!(
                    "{{\"action\":{},\"manifest\":{},\"state\":{},",
                    "\"panel_url\":{},\"polling\":true}}"
                ),
                json_string(&format!("https://github.com/settings/apps/new?state={state}")),
                json_string(&self.github.manifest(&panel_url, &suffix)),
                json_string(&state),
                json_string(&panel_url),
            ),
        )
    }

    fn github_manual(&self, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let app_id = match field(&fields, "app_id").parse::<u64>() {
            Ok(app_id) => app_id,
            Err(_) => return json_error(422, "App ID inválido"),
        };
        let slug = field(&fields, "slug");
        let private_key = field(&fields, "private_key");
        match self.github.connect_manually(app_id, slug, private_key) {
            Ok(()) => Response::json(201, "{\"ok\":true}".to_owned()),
            Err(error) => json_error(422, &error),
        }
    }

    /// URL de instalación con un nonce fresco para el retorno.
    fn github_install_url(&self) -> Response {
        let status = match self.github.status_json("") {
            Ok(status) => status,
            Err(error) => return json_error(500, &error),
        };
        let Some(install_url) = Json::parse(&status)
            .ok()
            .and_then(|value| value.string("install_url").map(str::to_owned))
        else {
            return json_error(409, "todavía no has conectado una GitHub App");
        };
        let state = match self.github.issue_nonce() {
            Ok(state) => state,
            Err(error) => return json_error(500, &error),
        };
        Response::json(
            200,
            format!(
                "{{\"url\":{}}}",
                json_string(&format!("{install_url}?state={state}"))
            ),
        )
    }

    fn github_installations(&self) -> Response {
        match self.github.installations_json() {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(502, &error),
        }
    }

    fn github_repositories(&self, request: &Request) -> Response {
        let Some(installation_id) = query_u64(request, "installation_id") else {
            return json_error(422, "installation_id inválido");
        };
        match self.github.repositories_json(installation_id) {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(502, &error),
        }
    }

    fn github_branches(&self, request: &Request) -> Response {
        let Some(installation_id) = query_u64(request, "installation_id") else {
            return json_error(422, "installation_id inválido");
        };
        let repository = request.query.get("repository").map_or("", String::as_str);
        match self.github.branches_json(installation_id, repository) {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(502, &error),
        }
    }

    /// Retornos del navegador desde GitHub. No llevan token: se validan por nonce.
    fn route_github_return(&self, method: &str, request: &Request) -> Response {
        if method != "GET" {
            return json_error(405, "método no permitido");
        }
        let segments = path_segments(&request.path);
        let state = request.query.get("state").map_or("", String::as_str);

        match segments.as_slice() {
            ["github", "callback"] => {
                if !self.github.consume_nonce(state) {
                    return Response::redirect("/?github=estado_invalido#/github");
                }
                let code = request.query.get("code").map_or("", String::as_str);
                match self.github.complete_manifest(code) {
                    Ok(()) => Response::redirect("/?github=conectado#/github"),
                    Err(_) => Response::redirect("/?github=error#/github"),
                }
            }
            ["github", "installed"] => {
                // Sin efectos secundarios: las instalaciones se leen en vivo con el JWT.
                let _ = self.github.consume_nonce(state);
                Response::redirect("/?github=instalado#/github")
            }
            _ => json_error(404, "ruta no encontrada"),
        }
    }

    // ── Webhooks ───────────────────────────────────────────────────────────

    fn route_webhook(&self, method: &str, request: &Request) -> Response {
        let segments = path_segments(&request.path);

        match segments.as_slice() {
            ["hooks", "deploy", slug] => {
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
                if !supplied_token
                    .is_some_and(|token| constant_time_eq(token, &project.webhook_token))
                {
                    return json_error(404, "webhook no encontrado");
                }
                self.deploy_project_with_project(project, request, "webhook")
            }
            _ => json_error(404, "webhook no encontrado"),
        }
    }

    // ── Alta de recursos ───────────────────────────────────────────────────

    fn create_database(&self, request: &Request, forced_engine: Option<&str>) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let engine_id = forced_engine.unwrap_or_else(|| field(&fields, "engine"));
        let Some(engine) = templates::engine(engine_id) else {
            return json_error(422, "motor de base de datos no soportado");
        };

        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        if !valid_slug(slug) {
            return json_error(422, "slug inválido");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }

        let database = match optional_field(&fields, "database") {
            Some(value) => value,
            None if engine.needs_database => "app",
            None => "",
        };
        let username = match optional_field(&fields, "username") {
            Some(value) => value,
            None if engine.needs_username => "app",
            None => "",
        };
        if engine.needs_database && !valid_db_identifier(database) {
            return json_error(422, "nombre de base de datos inválido");
        }
        if engine.needs_username && !valid_db_identifier(username) {
            return json_error(422, "nombre de usuario inválido");
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
        let root_password = match random_hex(24) {
            Ok(password) => password,
            Err(error) => return json_error(500, &format!("no se pudo crear contraseña: {error}")),
        };

        let published_port = match parse_port(&fields, "published_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        let memory_mb = optional_field(&fields, "memory_mb")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(engine.default_memory_mb)
            .clamp(64, 16_384);

        let generated = templates::database(&templates::DatabaseRequest {
            engine,
            slug,
            database,
            username,
            password: &password,
            root_password: &root_password,
            published_port,
            memory_mb,
        });

        let directory = match self.prepare_directory(slug) {
            Ok(directory) => directory,
            Err(error) => return json_error(error.status, &error.message),
        };

        self.finish_resource(
            &directory,
            NewResource {
                slug: slug.to_owned(),
                name: name.trim().to_owned(),
                kind: KIND_DATABASE,
                engine: Some(engine.id.to_owned()),
                repository: None,
                installation_id: None,
                branch: None,
                image_env: None,
                current_image: Some(generated.image.clone()),
                auto_deploy: false,
                generated,
            },
            Some(&password),
            published_port,
        )
    }

    fn create_image_service(&self, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        let image = field(&fields, "image");

        if !valid_slug(slug) {
            return json_error(422, "slug inválido");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }
        if !valid_image_ref(image) {
            return json_error(422, "referencia de imagen inválida");
        }

        let container_port = match parse_port(&fields, "container_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        let published_port = match parse_port(&fields, "published_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        if published_port.is_some() && container_port.is_none() {
            return json_error(422, "indica también el puerto interno del contenedor");
        }
        let memory_mb = optional_field(&fields, "memory_mb")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(512)
            .clamp(64, 16_384);
        let volume_path = optional_field(&fields, "volume_path");
        if volume_path.is_some_and(|path| !valid_absolute_path(path)) {
            return json_error(422, "ruta de volumen inválida");
        }
        let environment = match parse_environment(field(&fields, "environment")) {
            Ok(environment) => environment,
            Err(error) => return json_error(422, &error),
        };

        let generated = templates::service(&templates::ServiceRequest {
            slug,
            image,
            container_port,
            published_port,
            memory_mb,
            volume_path,
            environment: &environment,
        });

        let directory = match self.prepare_directory(slug) {
            Ok(directory) => directory,
            Err(error) => return json_error(error.status, &error.message),
        };

        self.finish_resource(
            &directory,
            NewResource {
                slug: slug.to_owned(),
                name: name.trim().to_owned(),
                kind: KIND_IMAGE,
                engine: optional_field(&fields, "icon").map(str::to_owned),
                repository: None,
                installation_id: None,
                branch: None,
                // Con la imagen en `.env` el rollback funciona igual que en Compose.
                image_env: Some("APP_IMAGE".to_owned()),
                current_image: Some(image.to_owned()),
                auto_deploy: field(&fields, "auto_deploy") != "false",
                generated,
            },
            None,
            published_port,
        )
    }

    fn create_repository_service(&self, request: &Request) -> Response {
        if !self.capabilities.git {
            return json_error(503, "git no está instalado en el servidor");
        }
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        let repository = field(&fields, "repository");
        let branch = field(&fields, "branch");

        if !valid_slug(slug) {
            return json_error(422, "slug inválido");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }
        if !crate::github::valid_repository(repository) {
            return json_error(422, "repositorio inválido");
        }
        if !valid_branch(branch) {
            return json_error(422, "rama inválida");
        }
        let Some(installation_id) = field(&fields, "installation_id").parse::<u64>().ok() else {
            return json_error(422, "installation_id inválido");
        };

        let dockerfile = optional_field(&fields, "dockerfile").unwrap_or("Dockerfile");
        let build_context = optional_field(&fields, "build_context").unwrap_or(".");
        if !valid_relative_path(dockerfile) || !valid_relative_path(build_context) {
            return json_error(422, "ruta de Dockerfile o contexto inválida");
        }

        let container_port = match parse_port(&fields, "container_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        let published_port = match parse_port(&fields, "published_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        if published_port.is_some() && container_port.is_none() {
            return json_error(422, "indica también el puerto interno del contenedor");
        }
        let memory_mb = optional_field(&fields, "memory_mb")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(512)
            .clamp(64, 16_384);
        let environment = match parse_environment(field(&fields, "environment")) {
            Ok(environment) => environment,
            Err(error) => return json_error(422, &error),
        };

        let token = match self.github.installation_token(installation_id) {
            Ok(token) => token,
            Err(error) => return json_error(502, &error),
        };

        let directory = match self.prepare_directory(slug) {
            Ok(directory) => directory,
            Err(error) => return json_error(error.status, &error.message),
        };

        let clone = self
            .git
            .clone_repository(repository, branch, &token, &directory.join("repo"));
        match clone {
            Ok(result) if result.success => {}
            Ok(result) => {
                let _ = fs::remove_dir_all(&directory);
                return json_error(
                    502,
                    &format!(
                        "no se pudo clonar {repository}: {}",
                        result.redacted_summary(&[&token])
                    ),
                );
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                return json_error(502, &error);
            }
        }

        let dockerfile_path = directory
            .join("repo")
            .join(build_context.trim_matches('/'))
            .join(dockerfile);
        if !dockerfile_path.is_file() {
            let _ = fs::remove_dir_all(&directory);
            return json_error(
                422,
                &format!("el repositorio no contiene {build_context}/{dockerfile}"),
            );
        }

        let generated = templates::repository(&templates::RepositoryRequest {
            slug,
            repository,
            branch,
            dockerfile,
            build_context,
            container_port,
            published_port,
            memory_mb,
            environment: &environment,
        });

        self.finish_resource(
            &directory,
            NewResource {
                slug: slug.to_owned(),
                name: name.trim().to_owned(),
                kind: KIND_REPOSITORY,
                engine: None,
                repository: Some(repository.to_owned()),
                installation_id: Some(installation_id),
                branch: Some(branch.to_owned()),
                image_env: None,
                current_image: Some(generated.image.clone()),
                auto_deploy: field(&fields, "auto_deploy") != "false",
                generated,
            },
            None,
            published_port,
        )
    }

    /// Crea `<allowed_root>/<slug>` con permisos restringidos.
    fn prepare_directory(&self, slug: &str) -> Result<PathBuf, ApiError> {
        let directory = self.config.allowed_root.join(slug);
        if directory.exists() {
            return Err(ApiError::new(
                409,
                format!("ya existe {}; elige otro slug", directory.display()),
            ));
        }
        fs::create_dir(&directory)
            .map_err(|error| ApiError::new(500, format!("no se pudo crear el directorio: {error}")))?;
        if let Err(error) = fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)) {
            let _ = fs::remove_dir_all(&directory);
            return Err(ApiError::new(
                500,
                format!("no se pudo proteger el directorio: {error}"),
            ));
        }
        Ok(directory)
    }

    /// Escribe los archivos, registra el proyecto y hace el primer despliegue.
    /// Cualquier fallo antes del registro deja el disco como estaba.
    fn finish_resource(
        &self,
        directory: &Path,
        resource: NewResource,
        secret: Option<&str>,
        published_port: Option<u16>,
    ) -> Response {
        let compose_file = directory.join("compose.yaml");
        let env_file = directory.join(".env");

        let abort = |error: String, status: u16| -> Response {
            let _ = fs::remove_dir_all(directory);
            json_error(status, &error)
        };

        if let Err(error) = atomic_write(&compose_file, resource.generated.compose.as_bytes(), 0o640)
        {
            return abort(format!("no se pudo escribir Compose: {error}"), 500);
        }
        if let Err(error) = atomic_write(&env_file, resource.generated.env.as_bytes(), 0o600) {
            return abort(format!("no se pudo escribir .env: {error}"), 500);
        }
        if let Err(error) = self.docker.ensure_network(templates::SHARED_NETWORK) {
            return abort(format!("no se pudo crear la red compartida: {error}"), 502);
        }
        if let Err(error) = self.docker.validate_compose(&compose_file) {
            return abort(format!("el Compose generado no es válido: {error}"), 422);
        }

        let webhook_token = match random_hex(24) {
            Ok(token) => token,
            Err(error) => return abort(format!("no se pudo crear token: {error}"), 500),
        };

        let project = Project {
            slug: resource.slug,
            name: resource.name,
            compose_file,
            env_file: Some(env_file),
            image_env: resource.image_env,
            branch: resource.branch,
            webhook_token,
            current_image: resource.current_image,
            created_at: now_unix(),
            kind: resource.kind.to_owned(),
            engine: resource.engine,
            repository: resource.repository,
            installation_id: resource.installation_id,
            auto_deploy: resource.auto_deploy,
            source_revision: None,
        };
        if let Err(error) = self.store.add_project(project.clone()) {
            return abort(error, 409);
        }

        // A partir de aquí el proyecto ya está registrado: un fallo de despliegue
        // se informa pero no destruye nada, para que el usuario pueda reintentar.
        let outcome = match self.perform_deploy(project.clone(), None, None, None, "resource") {
            Ok(outcome) => outcome,
            Err(error) => {
                return Response::json(
                    error.status,
                    format!(
                        concat!(
                            "{{\"project\":{},\"deployment\":null,\"password\":{},",
                            "\"connection_uri\":{},\"host\":{},\"published_port\":{},\"error\":{}}}"
                        ),
                        project.to_json(true),
                        json_optional_secret(secret),
                        json_string(&resource.generated.connection_uri),
                        json_string(&resource.generated.host),
                        published_port.map_or_else(|| "null".to_owned(), |port| port.to_string()),
                        json_string(&error.message),
                    ),
                );
            }
        };

        if outcome.success && project.kind == KIND_IMAGE {
            if let Some(image) = project.current_image.as_deref() {
                if let Ok(Some(revision)) = self.docker.image_revision(image) {
                    let _ = self.store.update_source_revision(&project.slug, revision);
                }
            }
        }

        Response::json(
            if outcome.success { 201 } else { 502 },
            format!(
                concat!(
                    "{{\"project\":{},\"deployment\":{},\"password\":{},",
                    "\"connection_uri\":{},\"host\":{},\"published_port\":{}}}"
                ),
                project.to_json(true),
                outcome.deployment.to_json(),
                json_optional_secret(secret),
                json_string(&resource.generated.connection_uri),
                json_string(&resource.generated.host),
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

struct NewResource {
    slug: String,
    name: String,
    kind: &'static str,
    engine: Option<String>,
    repository: Option<String>,
    installation_id: Option<u64>,
    branch: Option<String>,
    image_env: Option<String>,
    current_image: Option<String>,
    auto_deploy: bool,
    generated: GeneratedResource,
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

fn optional_field<'a>(fields: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    let value = field(fields, name);
    (!value.is_empty()).then_some(value)
}

fn query_u64(request: &Request, name: &str) -> Option<u64> {
    request
        .query
        .get(name)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn parse_port(fields: &HashMap<String, String>, name: &str) -> Result<Option<u16>, String> {
    match optional_field(fields, name) {
        Some(value) => match value.parse::<u16>() {
            Ok(port) if port > 0 => Ok(Some(port)),
            _ => Err(format!("{name} inválido")),
        },
        None => Ok(None),
    }
}

/// Convierte un bloque `CLAVE=valor` por líneas en pares validados.
fn parse_environment(raw: &str) -> Result<Vec<(String, String)>, String> {
    const RESERVED: [&str; 2] = ["APP_IMAGE", "TDM_MEMORY_LIMIT"];
    let mut pairs = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("línea de entorno sin '=': {line}"));
        };
        let key = key.trim();
        let value = value.trim();

        if !valid_env_key(key) {
            return Err(format!("clave de entorno inválida: {key}"));
        }
        if RESERVED.contains(&key) {
            return Err(format!("{key} lo gestiona el panel; usa otro nombre"));
        }
        if value.len() > 4096 || value.contains(['\r', '\n', '\0']) {
            return Err(format!("valor inválido para {key}"));
        }
        if pairs.len() >= 100 {
            return Err("demasiadas variables de entorno (máximo 100)".to_owned());
        }
        pairs.push((key.to_owned(), value.to_owned()));
    }
    Ok(pairs)
}

fn valid_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
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

/// Ruta relativa dentro del clon: sin `..`, sin raíz absoluta y sin caracteres raros.
fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('/')
        && !value.split('/').any(|segment| segment == "..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/')
        })
}

/// Punto de montaje dentro del contenedor.
fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 256
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/')
        })
}

fn json_optional_secret(secret: Option<&str>) -> String {
    secret.map_or_else(|| "null".to_owned(), json_string)
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
    fn relative_paths_cannot_escape_the_clone() {
        assert!(valid_relative_path("Dockerfile"));
        assert!(valid_relative_path("services/api/Dockerfile"));
        assert!(valid_relative_path("."));
        assert!(!valid_relative_path("/etc/passwd"));
        assert!(!valid_relative_path("../Dockerfile"));
        assert!(!valid_relative_path("a/../../b"));
        assert!(!valid_relative_path(""));
    }

    #[test]
    fn volume_paths_must_be_absolute_and_plain() {
        assert!(valid_absolute_path("/data"));
        assert!(valid_absolute_path("/var/lib/app"));
        assert!(!valid_absolute_path("data"));
        assert!(!valid_absolute_path("/data/../etc"));
        assert!(!valid_absolute_path("/data;rm -rf /"));
    }

    #[test]
    fn environment_blocks_are_parsed_and_guarded() {
        let parsed = parse_environment("# comentario\nLOG_LEVEL=info\n\nPORT=3000\n").unwrap();
        assert_eq!(
            parsed,
            vec![
                ("LOG_LEVEL".to_owned(), "info".to_owned()),
                ("PORT".to_owned(), "3000".to_owned()),
            ]
        );

        assert!(parse_environment("minusculas=1").is_err());
        assert!(parse_environment("SIN_IGUAL").is_err());
        assert!(parse_environment("APP_IMAGE=otra:1").is_err());
        assert!(parse_environment("TDM_MEMORY_LIMIT=99g").is_err());
    }

    #[test]
    fn hosts_used_for_github_callbacks_are_validated() {
        assert!(valid_host("127.0.0.1:8787"));
        assert!(valid_host("panel.example.com"));
        assert!(!valid_host("panel.example.com/evil"));
        assert!(!valid_host("panel example.com"));
        assert!(!valid_host(""));
    }
}
