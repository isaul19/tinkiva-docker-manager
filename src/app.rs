use crate::docker::{self, DockerClient};
use crate::git::GitClient;
use crate::aws::{self, Ecr, EcrCredentials};
use crate::github::GitHub;
use crate::http::{Request, Response};
use crate::json::Json;
use crate::metrics::{collect_processes, processes_to_json, HostMetrics};
use crate::model::{
    Deployment, Project, KIND_COMPOSE, KIND_DATABASE, KIND_IMAGE, KIND_REPOSITORY,
};
use crate::proc::CommandResult;
use crate::store::Store;
use crate::templates::{self, GeneratedResource};
use crate::util::{
    atomic_write, canonical_existing_within, constant_time_eq, json_string, json_string_array,
    now_unix, random_hex, truncate_text,
    read_env_value, remove_env_key, set_env_value, unique_suffix, valid_container_ref,
    valid_db_identifier, valid_display_name, valid_env_key, valid_image_ref, valid_schema_name,
    valid_slug,
};
use crate::{buildpack, net};
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
    /// Builds de repositorio que se conservan por proyecto. 0 desactiva la
    /// limpieza automática.
    pub image_retention: usize,
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
        // Dos: la que corre y la anterior. Son exactamente las que el panel
        // sabe alcanzar, porque «Rollback» solo retrocede un paso.
        let image_retention = setting("TDM_IMAGE_RETENTION")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2)
            .min(100);
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
            image_retention,
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
    ecr: Ecr,
    capabilities: Capabilities,
    deploy_lock: Mutex<()>,
    started_at: u64,
}

impl App {
    pub fn new(config: Config) -> Result<Self, String> {
        let store = Store::load(config.data_dir.join("state.db"), config.max_history)?;
        let github = GitHub::load(config.data_dir.join("github.json"))?;
        let ecr = Ecr::load(config.data_dir.join("ecr.conf"))?;
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
            ecr,
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
            // Un recurso Compose se vigila si declara qué imagen observar: es lo
            // que permite auto-desplegar lo que GitLab acaba de subir a ECR.
            let watches_registry = project.kind == KIND_IMAGE
                || (project.kind == KIND_COMPOSE && project.current_image.is_some());
            let changed = if project.kind == KIND_REPOSITORY {
                self.repository_changed(&project)
            } else if watches_registry {
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
        self.ensure_registry_login(Some(image));
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
            ("GET", ["api", "containers", container, "console"]) => {
                self.container_console_info(container)
            }
            ("POST", ["api", "containers", container, "console"]) => {
                self.container_console(container, request)
            }
            ("GET", ["api", "containers", container, "export"]) => {
                self.container_sql_info(container)
            }
            ("POST", ["api", "containers", container, "export"]) => {
                self.container_export(container, request)
            }
            ("GET", ["api", "containers", container, "import"]) => {
                self.container_sql_info(container)
            }
            ("POST", ["api", "containers", container, "import"]) => {
                self.container_import(container, request)
            }
            ("POST", ["api", "containers", container, action]) => {
                self.container_action(container, action)
            }
            ("GET", ["api", "images"]) => self.list_images(),
            ("DELETE", ["api", "images"]) => self.delete_image(request),
            ("POST", ["api", "images", "prune"]) => self.prune_images(),
            ("GET", ["api", "projects"]) => self.list_projects(),
            ("POST", ["api", "projects"]) => self.create_project(request),
            ("DELETE", ["api", "projects", slug]) => self.delete_project(slug, request),
            ("GET", ["api", "projects", slug, "logs"]) => self.project_logs(slug, request),
            ("GET", ["api", "projects", slug, "compose"]) => self.project_compose(slug),
            ("POST", ["api", "projects", slug, "compose"]) => {
                self.update_project_compose(slug, request)
            }
            ("GET", ["api", "projects", slug, "environment"]) => self.project_environment(slug),
            ("POST", ["api", "projects", slug, "environment"]) => self.update_project_environment(slug, request),
            ("POST", ["api", "projects", slug, "deploy"]) => {
                self.deploy_project(slug, request, "manual")
            }
            ("POST", ["api", "projects", slug, "rollback"]) => self.rollback_project(slug),
            ("GET", ["api", "history"]) => self.history(request),
            ("GET", ["api", "history", "page"]) => self.history_page(request),

            ("GET", ["api", "ecr"]) => self.ecr_status(),
            ("POST", ["api", "ecr"]) => self.ecr_connect(request),
            ("DELETE", ["api", "ecr"]) => self.ecr_disconnect(),
            ("GET", ["api", "ecr", "repositories"]) => self.ecr_repositories(request),

            ("GET", ["api", "github"]) => self.github_status(request),
            ("DELETE", ["api", "github"]) => self.github_disconnect(),
            ("POST", ["api", "github", "manifest"]) => self.github_manifest(request),
            ("POST", ["api", "github", "manual"]) => self.github_manual(request),
            ("POST", ["api", "github", "install"]) => self.github_install_url(),
            ("GET", ["api", "github", "installations"]) => self.github_installations(),
            ("GET", ["api", "github", "repositories"]) => self.github_repositories(request),
            ("GET", ["api", "github", "branches"]) => self.github_branches(request),

            ("POST", ["api", "resources", "database"]) => self.create_database(request, None),
            ("POST", ["api", "resources", "repository"]) => self.create_repository_service(request),
            ("POST", ["api", "resources", "compose"]) => self.create_compose_text_resource(request),
            ("POST", ["api", "resources", "ecr"]) => self.create_ecr_resource(request),
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
                    "\"ecr_registry\":{},",
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
                // Vacío mientras no haya credenciales: sirve de bandera y, cuando
                // las hay, ahorra al usuario teclear el host del registro.
                json_string(&self.ecr_registry_host()),
                docker.to_json(),
            ),
        )
    }

    fn ecr_registry_host(&self) -> String {
        self.ecr
            .credentials()
            .ok()
            .flatten()
            .map(|credentials| credentials.registry_host())
            .unwrap_or_default()
    }

    /// Catálogo estático que alimenta el diálogo «Añadir recurso».
    fn catalog(&self) -> Response {
        Response::json(
            200,
            format!(
                "{{\"engines\":{},\"capabilities\":{},\"allowed_root\":{}}}",
                templates::engines_json(),
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

    /// Imágenes que hay que conservar aunque ningún contenedor las use: son el
    /// destino de «Rollback» de algún recurso. Un despliegue anterior deja de
    /// estar en uso en cuanto se sustituye, así que sin esta protección la
    /// limpieza se llevaría justo lo que permite volver atrás.
    fn rollback_images(&self) -> HashMap<String, String> {
        let mut protected = HashMap::new();
        let Ok(projects) = self.store.projects() else {
            return protected;
        };
        for project in projects {
            if let Some(image) = project.current_image.clone() {
                protected.insert(image, project.slug.clone());
            }
            if let Ok(Some(image)) = self.store.rollback_target(&project.slug) {
                protected.insert(image, project.slug.clone());
            }
        }
        protected
    }

    /// Imágenes del host con la marca de rollback ya aplicada.
    fn images_with_protection(&self) -> Result<Vec<crate::docker::ImageInfo>, String> {
        let mut images = self.docker.images()?;
        let protected = self.rollback_images();
        for image in &mut images {
            image.protected_by = protected.get(&image.reference).cloned();
        }
        Ok(images)
    }

    fn list_images(&self) -> Response {
        match self.images_with_protection() {
            Ok(images) => Response::json(
                200,
                format!(
                    "[{}]",
                    images
                        .iter()
                        .map(|image| image.to_json())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            Err(error) => json_error(503, &error),
        }
    }

    /// Borra una imagen local, pero solo si ningún contenedor la usa. La
    /// comprobación se rehace aquí con datos frescos: entre que el panel pintó
    /// la lista y el usuario pulsó borrar puede haber arrancado algo.
    fn delete_image(&self, request: &Request) -> Response {
        let Some(reference) = request.query.get("reference").map(String::as_str) else {
            return json_error(400, "indica la imagen a borrar");
        };
        let images = match self.images_with_protection() {
            Ok(images) => images,
            Err(error) => return json_error(503, &error),
        };
        let Some(image) = images
            .iter()
            .find(|image| image.reference == reference || image.id == reference)
        else {
            return json_error(404, "la imagen no existe en este host");
        };
        if !image.containers.is_empty() {
            return json_error(
                409,
                &format!(
                    "la imagen está en uso por {}. Elimina primero esos contenedores.",
                    image.containers.join(", ")
                ),
            );
        }

        match self.docker.remove_image(&image.reference) {
            Ok(result) if result.success => Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"message\":{}}}",
                    json_string(&format!("Imagen {} eliminada.", image.reference))
                ),
            ),
            Ok(result) => json_error(502, &result.summary()),
            Err(error) => json_error(502, &error),
        }
    }

    /// Borra de una vez todas las imágenes que ningún contenedor usa, salvo las
    /// que siguen siendo el rollback de algún recurso. No es `docker image
    /// prune -a`: ese se llevaría también esas versiones anteriores.
    fn prune_images(&self) -> Response {
        let images = match self.images_with_protection() {
            Ok(images) => images,
            Err(error) => return json_error(503, &error),
        };

        let mut removed = Vec::new();
        let mut failed = Vec::new();
        let mut freed_ids: HashMap<String, u64> = HashMap::new();
        let mut kept = 0_usize;

        for image in images.iter().filter(|image| image.containers.is_empty()) {
            if image.protected_by.is_some() {
                kept += 1;
                continue;
            }
            match self.docker.remove_image(&image.reference) {
                Ok(result) if result.success => {
                    freed_ids.insert(image.id.clone(), image.size_bytes);
                    removed.push(image.reference.clone());
                }
                Ok(result) => failed.push(format!("{}: {}", image.reference, result.summary())),
                Err(error) => failed.push(format!("{}: {error}", image.reference)),
            }
        }

        // El espacio se cuenta por id: dos etiquetas de la misma imagen no
        // liberan el doble.
        let freed: u64 = freed_ids.values().sum();
        let message = if removed.is_empty() {
            "No había imágenes sin usar que borrar.".to_owned()
        } else {
            format!("{} imagen(es) eliminada(s).", removed.len())
        };
        Response::json(
            200,
            format!(
                "{{\"ok\":true,\"removed\":{},\"kept\":{kept},\"freed_bytes\":{freed},\"failed\":[{}],\"message\":{}}}",
                removed.len(),
                failed
                    .iter()
                    .map(|error| json_string(error))
                    .collect::<Vec<_>>()
                    .join(","),
                json_string(&message),
            ),
        )
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
                        .map(|project| {
                            let runtime_status = self
                                .docker
                                .project_status(project)
                                .unwrap_or("error");
                            let rollback_target = self
                                .store
                                .rollback_target(&project.slug)
                                .ok()
                                .flatten();
                            let rollback_configured =
                                project.env_file.is_some() && project.image_env.is_some();
                            let has_previous_image =
                                rollback_target.as_deref() != project.current_image.as_deref();
                            let can_rollback =
                                rollback_configured && rollback_target.is_some() && has_previous_image;
                            let rollback_reason = if !rollback_configured {
                                "Este recurso no usa una imagen configurable mediante archivo .env."
                            } else if rollback_target.is_none() || !has_previous_image {
                                "Todavía no hay una imagen anterior disponible."
                            } else {
                                ""
                            };
                            let last_deployment = self
                                .store
                                .last_deployment(&project.slug)
                                .ok()
                                .flatten()
                                .map_or_else(
                                    || "null".to_owned(),
                                    |deployment| {
                                        format!(
                                            "{{\"created_at\":{},\"status\":{}}}",
                                            deployment.created_at,
                                            json_string(&deployment.status)
                                        )
                                    },
                                );
                            let project_json = project.to_json(true);
                            format!(
                                concat!(
                                    "{},\"runtime_status\":{},\"can_rollback\":{},",
                                    "\"rollback_reason\":{},\"last_deployment\":{}}}"
                                ),
                                project_json.trim_end_matches('}'),
                                json_string(runtime_status),
                                can_rollback,
                                json_string(rollback_reason),
                                last_deployment,
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            Err(error) => json_error(500, &error),
        }
    }

    fn create_project(&self, request: &Request) -> Response {
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
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

    fn container_console_info(&self, container: &str) -> Response {
        match self.docker.container_console_info(container) {
            Ok(info) => Response::json(
                200,
                format!(
                    "{{\"database\":{},\"database_label\":{},\"user\":{}}}",
                    info.database.map_or_else(|| "null".to_owned(), json_string),
                    info.database
                        .map(database_label)
                        .map_or_else(|| "null".to_owned(), json_string),
                    json_string(&info.user),
                ),
            ),
            Err(error) => json_error(400, &error),
        }
    }

    /// Ejecuta una consulta en bases de datos detectadas o un comando no
    /// interactivo en los demás contenedores. Nunca se ejecuta nada en el host.
    fn container_console(&self, container: &str, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let input = fields.get("input").map_or("", String::as_str);
        let info = match self.docker.container_console_info(container) {
            Ok(info) => info,
            Err(error) => return json_error(400, &error),
        };
        let result = match info.database {
            Some(database) => self.docker.database_query(container, database, input),
            None => self.docker.container_exec(container, input),
        };
        match result {
            Ok(result) => Response::json(
                200,
                format!(
                    "{{\"ok\":{},\"output\":{}}}",
                    result.success,
                    json_string(&console_output(&result))
                ),
            ),
            Err(error) => json_error(400, &error),
        }
    }

    /// Motor detectado y bases de datos con las que trabajan la exportación y la
    /// importación. Se consulta al abrir cada diálogo, no en el listado: cada
    /// llamada ejecuta comandos dentro del contenedor.
    fn container_sql_info(&self, container: &str) -> Response {
        let engine = match self.exportable_database(container) {
            Ok(engine) => engine,
            Err(response) => return response,
        };
        let schemas = match self.docker.database_schemas(container, engine) {
            Ok(schemas) => schemas,
            Err(error) => return json_error(502, &error),
        };
        let list = schemas
            .iter()
            .map(|schema| json_string(schema))
            .collect::<Vec<_>>()
            .join(",");
        Response::json(
            200,
            format!(
                "{{\"database\":{},\"database_label\":{},\"schemas\":[{list}]}}",
                json_string(engine),
                json_string(database_label(engine)),
            ),
        )
    }

    /// Genera el volcado y lo entrega como descarga. El `.sql` se escribe en un
    /// archivo temporal y se transmite por trozos: el panel no guarda el
    /// volcado en memoria ni deja rastro en disco después de enviarlo.
    fn container_export(&self, container: &str, request: &Request) -> Response {
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let mode = fields.get("mode").map_or("all", String::as_str);
        if !matches!(mode, "all" | "structure" | "data") {
            return json_error(400, "modo de exportación no válido");
        }
        let schemas: Vec<String> = fields
            .get("schemas")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|schema| !schema.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        if schemas.is_empty() {
            return json_error(400, "selecciona al menos una base de datos");
        }
        if !schemas.iter().all(|schema| valid_schema_name(schema)) {
            return json_error(400, "nombre de base de datos inválido");
        }

        let engine = match self.exportable_database(container) {
            Ok(engine) => engine,
            Err(response) => return response,
        };

        let destination = env::temp_dir().join(format!("tdm-dump-{}.sql", unique_suffix()));
        let result = self
            .docker
            .database_dump(container, engine, mode, &schemas, &destination);
        match result {
            Ok(dump) if dump.success => Response::temporary_file_download(
                destination,
                dump.bytes,
                "application/sql; charset=utf-8",
                &format!("{container}.sql"),
            ),
            Ok(dump) => {
                let _ = fs::remove_file(&destination);
                json_error(502, &format!("la exportación falló: {}", dump.summary()))
            }
            Err(error) => {
                let _ = fs::remove_file(&destination);
                json_error(502, &error)
            }
        }
    }

    /// Restaura dentro del contenedor el `.sql` que subió el navegador.
    ///
    /// El archivo no viaja en el JSON ni en un formulario: llega como cuerpo
    /// crudo y `http::read_request` lo escribe en un temporal según entra, así
    /// que un volcado de cientos de megabytes nunca pasa por la memoria del
    /// panel. De aquí va directo a la entrada estándar del cliente.
    fn container_import(&self, container: &str, request: &Request) -> Response {
        let Some(source) = request.upload.as_ref() else {
            return json_error(400, "adjunta un archivo .sql");
        };
        let bytes = fs::metadata(source).map(|data| data.len()).unwrap_or(0);
        if bytes == 0 {
            return json_error(400, "el archivo está vacío");
        }

        let schema = request.query.get("schema").map_or("", String::as_str).trim();
        if !valid_schema_name(schema) {
            return json_error(400, "selecciona la base de datos de destino");
        }

        let engine = match self.exportable_database(container) {
            Ok(engine) => engine,
            Err(response) => return response,
        };

        match self.docker.database_restore(container, engine, schema, source) {
            Ok(result) if result.success => Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"bytes\":{bytes},\"output\":{}}}",
                    json_string(&console_output(&result))
                ),
            ),
            Ok(result) => json_error(
                502,
                &format!("la importación falló: {}", console_output(&result)),
            ),
            Err(error) => json_error(502, &error),
        }
    }

    /// Comprueba que el contenedor sea una base de datos con volcado SQL.
    fn exportable_database(&self, container: &str) -> Result<&'static str, Response> {
        let info = match self.docker.container_console_info(container) {
            Ok(info) => info,
            Err(error) => return Err(json_error(400, &error)),
        };
        match info.database {
            Some(engine) if docker::exportable_engine(engine) => Ok(engine),
            Some(engine) => Err(json_error(
                422,
                &format!(
                    "{} no genera volcados SQL; solo PostgreSQL, MySQL y MariaDB.",
                    database_label(engine)
                ),
            )),
            None => Err(json_error(
                422,
                "el contenedor no parece una base de datos SQL",
            )),
        }
    }

    /// Devuelve el Compose como texto editable para recursos Compose. Los recursos
    /// generados por los asistentes tienen configuración adicional que no debe
    /// editarse a ciegas desde este editor.
    fn project_compose(&self, slug: &str) -> Response {
        let project = match self.editable_compose_project(slug) {
            Ok(project) => project,
            Err(response) => return response,
        };
        match fs::read_to_string(&project.compose_file) {
            Ok(compose) => Response::json(200, format!("{{\"compose\":{}}}", json_string(&compose))),
            Err(error) => json_error(500, &format!("no se pudo leer compose.yaml: {error}")),
        }
    }

    /// Guarda el YAML de un recurso Compose después de validarlo con Docker.
    /// No despliega automáticamente: el usuario puede revisar y usar
    /// «Desplegar» cuando esté listo.
    fn update_project_compose(&self, slug: &str, request: &Request) -> Response {
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
        let project = match self.editable_compose_project(slug) {
            Ok(project) => project,
            Err(response) => return response,
        };
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let compose = fields.get("compose").map_or("", String::as_str);
        if let Err(error) = validate_compose_text(compose) {
            return json_error(422, &error);
        }
        let original = match fs::read_to_string(&project.compose_file) {
            Ok(contents) => contents,
            Err(error) => return json_error(500, &format!("no se pudo leer compose.yaml: {error}")),
        };
        if compose == original {
            return Response::json(200, "{\"ok\":true,\"changed\":false,\"message\":\"No había cambios que guardar.\"}".to_owned());
        }
        if let Err(error) = atomic_write(&project.compose_file, compose.as_bytes(), 0o640) {
            return json_error(500, &format!("no se pudo actualizar compose.yaml: {error}"));
        }
        if let Err(error) = self.docker.validate_compose(&project.compose_file) {
            let _ = atomic_write(&project.compose_file, original.as_bytes(), 0o640);
            return json_error(422, &format!("Compose inválido: {error}"));
        }
        Response::json(200, "{\"ok\":true,\"changed\":true,\"message\":\"Compose guardado. Despliega el recurso para aplicar los cambios.\"}".to_owned())
    }

    fn project_environment(&self, slug: &str) -> Response {
        if !valid_slug(slug) {
            return json_error(400, "slug inválido");
        }
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "proyecto no encontrado"),
            Err(error) => return json_error(500, &error),
        };
        let Some(env_file) = project.env_file.as_ref() else {
            return json_error(409, "este recurso no tiene archivo .env gestionado por el panel");
        };
        let env_file = match canonical_existing_within(&self.config.allowed_root, env_file) {
            Ok(path) if path.is_file() => path,
            Ok(_) => return json_error(409, "el archivo .env del recurso ya no existe"),
            Err(error) => return json_error(422, &error),
        };
        let contents = match fs::read_to_string(&env_file) {
            Ok(contents) => contents,
            Err(error) => return json_error(500, &format!("no se pudo leer .env: {error}")),
        };
        let managed = managed_environment_keys(&project);
        let environment = editable_environment(&contents, &managed);
        let managed_json = managed
            .iter()
            .map(|key| json_string(key))
            .collect::<Vec<_>>()
            .join(",");
        Response::json(
            200,
            format!(
                "{{\"environment\":{},\"managed_keys\":[{}]}}",
                json_string(&environment),
                managed_json
            ),
        )
    }

    fn update_project_environment(&self, slug: &str, request: &Request) -> Response {
        if !valid_slug(slug) {
            return json_error(400, "slug inválido");
        }
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return json_error(404, "proyecto no encontrado"),
            Err(error) => return json_error(500, &error),
        };
        let Some(env_file) = project.env_file.as_ref() else {
            return json_error(409, "este recurso no tiene archivo .env gestionado por el panel");
        };
        let env_file = match canonical_existing_within(&self.config.allowed_root, env_file) {
            Ok(path) if path.is_file() => path,
            Ok(_) => return json_error(409, "el archivo .env del recurso ya no existe"),
            Err(error) => return json_error(422, &error),
        };
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let raw = fields.get("environment").map_or("", String::as_str);
        let managed = managed_environment_keys(&project);
        let managed_refs = managed.iter().map(String::as_str).collect::<Vec<_>>();
        let environment = match parse_environment_with_reserved(raw, &managed_refs) {
            Ok(environment) => environment,
            Err(error) => return json_error(422, &error),
        };
        let original = match fs::read_to_string(&env_file) {
            Ok(contents) => contents,
            Err(error) => return json_error(500, &format!("no se pudo leer .env: {error}")),
        };
        let updated = replace_editable_environment(&original, &environment, &managed);
        if updated == original {
            return Response::json(
                200,
                "{\"ok\":true,\"changed\":false,\"message\":\"No había cambios que aplicar.\"}"
                    .to_owned(),
            );
        }
        if let Err(error) = atomic_write(&env_file, updated.as_bytes(), 0o600) {
            return json_error(500, &format!("no se pudo actualizar .env: {error}"));
        }
        if let Err(error) = self.docker.validate_compose(&project.compose_file) {
            let _ = atomic_write(&env_file, original.as_bytes(), 0o600);
            return json_error(
                422,
                &format!("las variables dejan el Compose inválido: {error}"),
            );
        }
        let started = Instant::now();
        match self.docker.deploy_pulled(&project) {
            Ok(result) if result.success => {
                let deployment = Deployment {
                    id: 0,
                    project: project.slug.clone(),
                    created_at: now_unix(),
                    status: "success".to_owned(),
                    branch: project.branch.clone(),
                    commit: None,
                    image: project.current_image.clone(),
                    previous_image: project.current_image.clone(),
                    message: "Variables de entorno actualizadas; Docker recreó el servicio si era necesario."
                        .to_owned(),
                    duration_ms: result.duration_ms.max(started.elapsed().as_millis()),
                    trigger: "environment".to_owned(),
                };
                if let Err(error) = self.store.append_deployment(deployment) {
                    return json_error(
                        500,
                        &format!("variables aplicadas, pero no se pudo registrar el cambio: {error}"),
                    );
                }
                Response::json(
                    200,
                    "{\"ok\":true,\"changed\":true,\"message\":\"Variables actualizadas y aplicadas al recurso.\"}"
                        .to_owned(),
                )
            }
            Ok(result) => {
                let _ = atomic_write(&env_file, original.as_bytes(), 0o600);
                let restored = self
                    .docker
                    .deploy_pulled(&project)
                    .map(|restore| restore.success)
                    .unwrap_or(false);
                let suffix = if restored {
                    " Se restauró el .env anterior."
                } else {
                    " No se pudo confirmar la restauración automática."
                };
                json_error(
                    502,
                    &format!("Docker no pudo aplicar las variables: {}{suffix}", result.summary()),
                )
            }
            Err(error) => {
                let _ = atomic_write(&env_file, original.as_bytes(), 0o600);
                let _ = self.docker.deploy_pulled(&project);
                json_error(502, &format!("no se pudieron aplicar las variables: {error}"))
            }
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
        if project.env_file.is_none() || project.image_env.is_none() {
            return json_error(
                409,
                "rollback no disponible: este recurso no usa una imagen configurable mediante archivo .env",
            );
        }
        let target = match self.store.rollback_target(slug) {
            Ok(Some(image)) => image,
            Ok(None) => return json_error(409, "no existe una imagen anterior para rollback"),
            Err(error) => return json_error(500, &error),
        };
        if project.current_image.as_deref() == Some(target.as_str()) {
            return json_error(409, "no existe una imagen anterior distinta para rollback");
        }

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
        self.restore_generated_dockerfile(project)?;
        Ok(self.git.head_commit(&directory))
    }

    fn restore_generated_dockerfile(&self, project: &Project) -> Result<(), ApiError> {
        let Some(directory) = project.directory() else {
            return Ok(());
        };
        let blueprint = directory.join(buildpack::BLUEPRINT_FILE);
        if !blueprint.is_file() {
            return Ok(());
        }
        let context = fs::read_to_string(directory.join(buildpack::CONTEXT_FILE))
            .map_err(|error| ApiError::new(500, format!("no se pudo leer el contexto generado: {error}")))?;
        let context = context.trim();
        if !valid_relative_path(context) {
            return Err(ApiError::new(500, "el contexto generado guardado es inválido"));
        }
        let repository = directory.join("repo").canonicalize()
            .map_err(|error| ApiError::new(500, format!("no se pudo resolver el clon: {error}")))?;
        let target = repository.join(context.trim_matches('/')).canonicalize()
            .map_err(|error| ApiError::new(500, format!("no se pudo resolver el contexto: {error}")))?;
        if !target.starts_with(&repository) || !target.is_dir() {
            return Err(ApiError::new(500, "el contexto generado sale del repositorio"));
        }
        let contents = fs::read(&blueprint)
            .map_err(|error| ApiError::new(500, format!("no se pudo leer el Dockerfile generado: {error}")))?;
        atomic_write(&target.join(buildpack::GENERATED_DOCKERFILE), &contents, 0o640)
            .map_err(|error| ApiError::new(500, format!("no se pudo restaurar el Dockerfile generado: {error}")))
    }

    /// Pasa un recurso de repositorio creado antes del pin por commit: el
    /// Compose pasa a resolver la imagen desde `APP_IMAGE`, el `.env` arranca
    /// con la etiqueta histórica y el proyecto queda con rollback habilitado.
    /// Es idempotente: si ya migró, no hace nada.
    fn migrate_repository_image(&self, project: &Project) -> Result<(), ApiError> {
        if project.image_env.is_some() {
            return Ok(());
        }
        let Some(env_path) = &project.env_file else {
            return Err(ApiError::new(
                500,
                "el recurso de repositorio no tiene .env; vuelve a crearlo",
            ));
        };
        let env_file = canonical_existing_within(&self.config.allowed_root, env_path)
            .map_err(|error| ApiError::new(422, error))?;

        let legacy_line = format!("    image: tinkiva/{}:latest\n", project.slug);
        if let Ok(compose) = fs::read_to_string(&project.compose_file) {
            if compose.contains(&legacy_line) {
                let patched = compose.replace(&legacy_line, "    image: ${APP_IMAGE}\n");
                atomic_write(&project.compose_file, patched.as_bytes(), 0o640).map_err(|error| {
                    ApiError::new(500, format!("no se pudo actualizar el Compose: {error}"))
                })?;
            }
        }

        let tag = format!("tinkiva/{}:latest", project.slug);
        set_env_value(&env_file, "APP_IMAGE", &tag)
            .map_err(|error| ApiError::new(500, format!("no se pudo actualizar .env: {error}")))?;
        self.store
            .set_image_env(&project.slug, "APP_IMAGE".to_owned())
            .map_err(|error| ApiError::new(500, format!("no se pudo registrar APP_IMAGE: {error}")))?;
        Ok(())
    }

    fn perform_deploy(
        &self,
        mut project: Project,
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
        // Antes de que Compose intente el pull: si la imagen es de ECR hay que
        // llevar el `docker login` al día.
        self.ensure_registry_login(
            image.as_deref().or(project.current_image.as_deref()),
        );
        let requested_image = image.clone();
        let mut previous_image = project.current_image.clone();
        let mut env_target: Option<(PathBuf, String)> = None;

        let synced_commit = if project.kind == KIND_REPOSITORY {
            // Un despliegue con imagen fija (rollback) no toca el clon: la
            // imagen anterior ya está construida en el Docker local.
            if image.is_none() {
                self.migrate_repository_image(&project)?;
                project.image_env = Some("APP_IMAGE".to_owned());
                self.sync_repository(&project)?
            } else {
                None
            }
        } else {
            None
        };
        let commit = commit.or(synced_commit);

        // Los builds de repositorio se etiquetan por commit: cada versión queda
        // en el Docker local y el rollback vuelve a ella sin reconstruir.
        let mut image = image;
        if project.kind == KIND_REPOSITORY && image.is_none() {
            if let Some(sha) = commit.as_deref() {
                let tag = format!("tinkiva/{}:{}", project.slug, &sha[..sha.len().min(12)]);
                if valid_image_ref(&tag) {
                    image = Some(tag);
                }
            }
        }

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
        // Con imagen explícita (rollback o pin manual) no se reconstruye:
        // `up -d` aplica la etiqueta que ya vive en el Docker local.
        let command = if trigger == "registry-poll" || requested_image.is_some() {
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
                Ok(()) => {
                    // En repositorios la imagen anterior ya está construida;
                    // sin --build la restauración es inmediata.
                    let restore = if project.kind == KIND_REPOSITORY {
                        self.docker.deploy_pulled(&project)
                    } else {
                        self.docker.deploy(&project)
                    };
                    match restore {
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
                    }
                }
                Err(error) => {
                    message.push_str(" | No se pudo restaurar el archivo .env: ");
                    message.push_str(&error.to_string());
                }
            }
        }

        let slug = project.slug.clone();
        let kind = project.kind.clone();
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

        // El historial ya está escrito, así que aquí `rollback_target` ve el
        // estado definitivo y la limpieza sabe qué imagen debe respetar.
        if success && kind == KIND_REPOSITORY {
            self.trim_build_images(&slug);
        }

        Ok(DeployOutcome {
            success,
            deployment,
        })
    }

    /// Borra las imágenes viejas que el panel construyó para un repositorio.
    ///
    /// Cada push deja una etiqueta `tinkiva/<slug>:<commit>` y sin esto se
    /// acumulan indefinidamente. Se conservan las `image_retention` más
    /// recientes y, pase lo que pase, la imagen desplegada y el destino de
    /// «Rollback».
    ///
    /// Por omisión son dos, que son justo las que el panel sabe alcanzar:
    /// `rollback_target` retrocede un único paso, así que una tercera imagen no
    /// tendría desde dónde recuperarse y solo ocuparía disco.
    fn trim_build_images(&self, slug: &str) {
        let retention = self.config.image_retention;
        if retention == 0 {
            return;
        }
        let repository = format!("tinkiva/{slug}");
        let builds = match self.docker.repository_images(&repository) {
            Ok(builds) => builds,
            Err(error) => {
                eprintln!("limpieza {slug}: no se pudieron listar las imágenes: {error}");
                return;
            }
        };

        let mut pinned = Vec::new();
        if let Ok(Some(project)) = self.store.project(slug) {
            pinned.extend(project.current_image);
        }
        if let Ok(Some(target)) = self.store.rollback_target(slug) {
            pinned.push(target);
        }

        for stale in stale_builds(&builds, retention, &pinned) {
            match self.docker.remove_image(&stale) {
                Ok(result) if result.success => {}
                Ok(result) => eprintln!("limpieza {slug}: {} sigue ahí: {}", stale, result.summary()),
                Err(error) => eprintln!("limpieza {slug}: {stale}: {error}"),
            }
        }
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

    fn history_page(&self, request: &Request) -> Response {
        let project = request.query.get("project").map(String::as_str);
        let offset = request
            .query
            .get("offset")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = request
            .query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(10)
            .clamp(1, 50);
        match self.store.history_page(project, offset, limit) {
            Ok((deployments, total)) => Response::json(
                200,
                format!(
                    "{{\"items\":[{}],\"total\":{},\"offset\":{},\"limit\":{}}}",
                    deployments
                        .iter()
                        .map(Deployment::to_json)
                        .collect::<Vec<_>>()
                        .join(","),
                    total,
                    offset,
                    limit
                ),
            ),
            Err(error) => json_error(500, &error),
        }
    }

    // ── Amazon ECR ─────────────────────────────────────────────────────────

    fn ecr_status(&self) -> Response {
        match self.ecr.status_json() {
            Ok(body) => Response::json(200, body),
            Err(error) => json_error(500, &error),
        }
    }

    /// Lista el registro conectado. Sin `?repository=` devuelve los nombres de
    /// los repositorios; con él, las etiquetas de ese repositorio.
    ///
    /// Se piden en dos pasos a propósito: pedir las etiquetas de todos al abrir
    /// el formulario serían tantas llamadas firmadas a AWS como repositorios
    /// tenga la cuenta.
    fn ecr_repositories(&self, request: &Request) -> Response {
        if !self.capabilities.curl {
            return json_error(503, "curl no está instalado; no se puede hablar con AWS");
        }
        match request.query.get("repository").map(String::as_str) {
            None | Some("") => match self.ecr.repositories() {
                Ok(names) => Response::json(
                    200,
                    format!("{{\"repositories\":{}}}", json_string_array(&names)),
                ),
                Err(error) => json_error(502, &error),
            },
            Some(repository) => match self.ecr.tags(repository) {
                Ok(tags) => {
                    let registry = self.ecr_registry_host();
                    let items: Vec<String> = tags
                        .iter()
                        .map(|entry| {
                            format!(
                                "{{\"tag\":{},\"image\":{},\"pushed_at\":{},\"size_bytes\":{}}}",
                                json_string(&entry.tag),
                                json_string(&format!("{registry}/{repository}:{}", entry.tag)),
                                entry.pushed_at,
                                entry.size_bytes,
                            )
                        })
                        .collect();
                    Response::json(200, format!("{{\"tags\":[{}]}}", items.join(",")))
                }
                Err(error) => json_error(502, &error),
            },
        }
    }

    /// Guarda las credenciales solo si sirven: primero pide el token a AWS y
    /// hace el `docker login`. Así el usuario se entera del error de permisos
    /// aquí y no meses después, en mitad de un despliegue.
    fn ecr_connect(&self, request: &Request) -> Response {
        if !self.capabilities.curl {
            return json_error(503, "curl no está instalado; no se puede hablar con AWS");
        }
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let access_key_id = field(&fields, "access_key_id");
        let secret_access_key = field(&fields, "secret_access_key");
        let region = field(&fields, "region");
        let registry_id = field(&fields, "registry_id");

        if !aws::valid_access_key(access_key_id) {
            return json_error(422, "access key id inválido");
        }
        if secret_access_key.is_empty() || secret_access_key.len() > 256 {
            return json_error(422, "secret access key inválido");
        }
        if !aws::valid_region(region) {
            return json_error(422, "región inválida, por ejemplo us-east-1");
        }
        if !aws::valid_registry_id(registry_id) {
            return json_error(422, "el id de registro son los 12 dígitos de la cuenta");
        }

        let credentials = EcrCredentials {
            access_key_id: access_key_id.to_owned(),
            secret_access_key: secret_access_key.to_owned(),
            region: region.to_owned(),
            registry_id: registry_id.to_owned(),
            connected_at: 0,
        };
        let token = match self.ecr.connect(credentials) {
            Ok(token) => token,
            Err(error) => return json_error(502, &error),
        };
        if let Err(error) = self
            .docker
            .login(&token.registry, &token.username, &token.password)
        {
            return json_error(502, &format!("AWS respondió, pero docker login falló: {error}"));
        }
        self.ecr_status()
    }

    fn ecr_disconnect(&self) -> Response {
        if let Ok(Some(credentials)) = self.ecr.credentials() {
            let _ = self.docker.logout(&credentials.registry_host());
        }
        match self.ecr.disconnect() {
            Ok(()) => Response::json(200, "{\"connected\":false}".to_owned()),
            Err(error) => json_error(500, &error),
        }
    }

    /// Renueva el `docker login` si la imagen vive en el ECR conectado. El token
    /// de AWS dura doce horas, así que sin esto los despliegues empezarían a
    /// fallar solos a mitad del día.
    fn ensure_registry_login(&self, image: Option<&str>) {
        let Some(image) = image else { return };
        if !self.ecr.owns_image(image) {
            return;
        }
        match self.ecr.token() {
            Ok(token) => {
                if let Err(error) = self
                    .docker
                    .login(&token.registry, &token.username, &token.password)
                {
                    eprintln!("ecr: no se pudo renovar el acceso: {error}");
                }
            }
            Err(error) => eprintln!("ecr: no se pudo pedir el token: {error}"),
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
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
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
        let external_access = field(&fields, "external_access") == "true";
        let memory_mb = parse_memory_mb(&fields, engine.default_memory_mb);

        let generated = templates::database(&templates::DatabaseRequest {
            engine,
            slug,
            database,
            username,
            password: &password,
            root_password: &root_password,
            published_port,
            external_access,
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
            external_access,
        )
    }

    fn create_repository_service(&self, request: &Request) -> Response {
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
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
        let build_mode = optional_field(&fields, "build_mode").unwrap_or("auto");
        if !matches!(build_mode, "auto" | "dockerfile") {
            return json_error(422, "modo de build inválido");
        }
        let build_context = optional_field(&fields, "build_context").unwrap_or(".");
        if !valid_relative_path(dockerfile) || !valid_relative_path(build_context) {
            return json_error(422, "ruta de Dockerfile o contexto inválida");
        }

        let mut container_port = match parse_port(&fields, "container_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        let mut published_port = match parse_port(&fields, "published_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        let memory_mb = parse_memory_mb(&fields, 512);
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

        let repository_root = match directory.join("repo").canonicalize() {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                return json_error(500, &format!("no se pudo resolver el clon: {error}"));
            }
        };
        let context = match repository_root.join(build_context.trim_matches('/')).canonicalize() {
            Ok(path) if path.is_dir() && path.starts_with(&repository_root) => path,
            _ => {
                let _ = fs::remove_dir_all(&directory);
                return json_error(422, "el contexto de build no existe o sale del repositorio");
            }
        };

        let plan = if build_mode == "dockerfile" {
            if !context.join(dockerfile).is_file() {
                let _ = fs::remove_dir_all(&directory);
                return json_error(422, &format!("el repositorio no contiene {build_context}/{dockerfile}"));
            }
            buildpack::BuildPlan {
                dockerfile_name: dockerfile.to_owned(),
                dockerfile: None,
                runtime: "docker",
                default_port: None,
            }
        } else {
            match buildpack::detect(&context, dockerfile) {
                Ok(plan) => plan,
                Err(error) => {
                    let _ = fs::remove_dir_all(&directory);
                    return json_error(422, &format!("no se pudo detectar la aplicación: {error}"));
                }
            }
        };

        if container_port.is_none() {
            container_port = plan.default_port;
        }
        // En modo automático el puerto puede conocerse recién después de detectar
        // el runtime. Si el puerto del VPS quedó vacío, usamos el mismo puerto.
        if published_port.is_none() && container_port.is_some() {
            published_port = container_port;
        }
        if published_port.is_some() && container_port.is_none() {
            let _ = fs::remove_dir_all(&directory);
            return json_error(422, "indica el puerto interno para publicar el servicio");
        }
        let external_access = field(&fields, "external_access") == "true";
        if let Some(contents) = &plan.dockerfile {
            if let Err(error) = atomic_write(&directory.join(buildpack::BLUEPRINT_FILE), contents.as_bytes(), 0o640)
                .and_then(|_| atomic_write(&directory.join(buildpack::CONTEXT_FILE), build_context.as_bytes(), 0o640))
                .and_then(|_| atomic_write(&context.join(buildpack::GENERATED_DOCKERFILE), contents.as_bytes(), 0o640))
            {
                let _ = fs::remove_dir_all(&directory);
                return json_error(500, &format!("no se pudo guardar el Dockerfile generado: {error}"));
            }
        }

        let generated = templates::repository(&templates::RepositoryRequest {
            slug,
            repository,
            branch,
            dockerfile: &plan.dockerfile_name,
            build_context,
            container_port,
            published_port,
            external_access,
            memory_mb,
            environment: &environment,
        });

        self.finish_resource(
            &directory,
            NewResource {
                slug: slug.to_owned(),
                name: name.trim().to_owned(),
                kind: KIND_REPOSITORY,
                engine: Some(plan.runtime.to_owned()),
                repository: Some(repository.to_owned()),
                installation_id: Some(installation_id),
                branch: Some(branch.to_owned()),
                image_env: Some("APP_IMAGE".to_owned()),
                current_image: Some(generated.image.clone()),
                auto_deploy: field(&fields, "auto_deploy") != "false",
                generated,
            },
            None,
            published_port,
            external_access,
        )
    }

    /// Crea un recurso a partir de una imagen del ECR conectado.
    ///
    /// El Compose lo genera el panel, no el usuario: así la imagen privada llega
    /// con el mismo endurecimiento que los demás recursos (red propia,
    /// `no-new-privileges`, puerto en loopback salvo que se pida lo contrario) y
    /// con `auto_deploy`, que es el motivo de conectar ECR.
    fn create_ecr_resource(&self, request: &Request) -> Response {
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        let image = field(&fields, "image");
        if !valid_slug(slug) {
            return json_error(422, "slug inválido; usa minúsculas, números y guiones");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }
        if !valid_image_ref(image) {
            return json_error(422, "referencia de imagen inválida");
        }
        // Solo imágenes del registro conectado: para cualquier otra no sabríamos
        // autenticarnos y el despliegue fallaría en el primer pull.
        if !self.ecr.owns_image(image) {
            return json_error(422, "esa imagen no pertenece al registro de ECR conectado");
        }

        let container_port = match parse_port(&fields, "container_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        let mut published_port = match parse_port(&fields, "published_port") {
            Ok(port) => port,
            Err(error) => return json_error(422, &error),
        };
        if published_port.is_none() {
            published_port = container_port;
        }
        if published_port.is_some() && container_port.is_none() {
            return json_error(422, "indica el puerto interno para publicar el servicio");
        }
        let memory_mb = parse_memory_mb(&fields, 512);
        let environment = match parse_environment(field(&fields, "environment")) {
            Ok(environment) => environment,
            Err(error) => return json_error(422, &error),
        };
        let external_access = field(&fields, "external_access") == "true";

        // El login se renueva antes de crear nada: si las credenciales ya no
        // valen, es mejor fallar aquí que dejar un recurso a medio desplegar.
        self.ensure_registry_login(Some(image));

        let directory = match self.prepare_directory(slug) {
            Ok(directory) => directory,
            Err(error) => return json_error(error.status, &error.message),
        };
        let generated = templates::registry_image(&templates::RegistryImageRequest {
            slug,
            image,
            container_port,
            published_port,
            external_access,
            memory_mb,
            environment: &environment,
        });

        self.finish_resource(
            &directory,
            NewResource {
                slug: slug.to_owned(),
                name: name.trim().to_owned(),
                kind: KIND_IMAGE,
                engine: None,
                repository: None,
                installation_id: None,
                branch: None,
                image_env: Some("APP_IMAGE".to_owned()),
                current_image: Some(image.to_owned()),
                auto_deploy: field(&fields, "auto_deploy") != "false",
                generated,
            },
            None,
            published_port,
            external_access,
        )
    }

    /// Crea un recurso Compose a partir de YAML pegado en la interfaz.
    fn create_compose_text_resource(&self, request: &Request) -> Response {
        if let Err(error) = self.require_docker_compose() {
            return json_error(503, &error);
        }
        let fields = match request.form() {
            Ok(fields) => fields,
            Err(error) => return json_error(400, &error),
        };
        let slug = field(&fields, "slug");
        let name = field(&fields, "name");
        let compose = fields.get("compose").map_or("", String::as_str);
        if !valid_slug(slug) {
            return json_error(422, "slug inválido; usa minúsculas, números y guiones");
        }
        if !valid_display_name(name) {
            return json_error(422, "nombre inválido");
        }
        if let Err(error) = validate_compose_text(compose) {
            return json_error(422, &error);
        }
        let directory = match self.prepare_directory(slug) {
            Ok(directory) => directory,
            Err(error) => return json_error(error.status, &error.message),
        };
        let compose_file = directory.join("compose.yaml");
        let abort = |message: String, status: u16| {
            let _ = fs::remove_dir_all(&directory);
            json_error(status, &message)
        };
        if let Err(error) = atomic_write(&compose_file, compose.as_bytes(), 0o640) {
            return abort(format!("no se pudo escribir Compose: {error}"), 500);
        }
        if let Err(error) = self.docker.validate_compose(&compose_file) {
            return abort(format!("Compose inválido: {error}"), 422);
        }
        let webhook_token = match random_hex(24) {
            Ok(token) => token,
            Err(error) => return abort(format!("no se pudo crear token: {error}"), 500),
        };
        let mut project = Project::compose(slug.to_owned(), name.trim().to_owned(), compose_file, webhook_token, now_unix());
        // Declarar qué imagen vigilar es lo que convierte un Compose en un
        // recurso con auto-deploy: el watcher comprueba su digest y redespliega
        // cuando el registro publica una versión nueva.
        if let Some(image) = optional_field(&fields, "watch_image") {
            if !valid_image_ref(image) {
                return abort("referencia de imagen inválida".to_owned(), 422);
            }
            project.current_image = Some(image.to_owned());
            project.auto_deploy = field(&fields, "auto_deploy") != "false";
        }
        match self.store.add_project(project.clone()) {
            Ok(()) => Response::json(201, project.to_json(true)),
            Err(error) => abort(error, 409),
        }
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
        external_access: bool,
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
                            "\"connection_uri\":{},\"host\":{},\"published_port\":{},",
                            "\"external_access\":{},\"error\":{}}}"
                        ),
                        project.to_json(true),
                        json_optional_secret(secret),
                        json_string(&resource.generated.connection_uri),
                        json_string(&resource.generated.host),
                        published_port.map_or_else(|| "null".to_owned(), |port| port.to_string()),
                        external_access,
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
                    "\"connection_uri\":{},\"host\":{},\"published_port\":{},",
                    "\"external_access\":{}}}"
                ),
                project.to_json(true),
                outcome.deployment.to_json(),
                json_optional_secret(secret),
                json_string(&resource.generated.connection_uri),
                json_string(&resource.generated.host),
                published_port.map_or_else(|| "null".to_owned(), |port| port.to_string()),
                external_access,
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

    fn editable_compose_project(&self, slug: &str) -> Result<Project, Response> {
        if !valid_slug(slug) {
            return Err(json_error(400, "slug inválido"));
        }
        let project = match self.store.project(slug) {
            Ok(Some(project)) => project,
            Ok(None) => return Err(json_error(404, "proyecto no encontrado")),
            Err(error) => return Err(json_error(500, &error)),
        };
        if project.kind != crate::model::KIND_COMPOSE {
            return Err(json_error(409, "solo los recursos Compose se editan como texto"));
        }
        match canonical_existing_within(&self.config.allowed_root, &project.compose_file) {
            Ok(path) if path.is_file() => {
                let mut project = project;
                project.compose_file = path;
                Ok(project)
            }
            Ok(_) => Err(json_error(409, "el archivo Compose del recurso ya no existe")),
            Err(error) => Err(json_error(422, &error)),
        }
    }

    fn require_docker_compose(&self) -> Result<(), String> {
        let info = self.docker.info();
        if !info.available {
            return Err(info.error.unwrap_or_else(|| "Docker no está disponible".to_owned()));
        }
        if info.compose_version.is_none() {
            return Err(info
                .compose_error
                .unwrap_or_else(|| "Docker Compose no está instalado".to_owned()));
        }
        Ok(())
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

/// Salida legible de un comando: stdout y stderr juntos y recortados. La usan
/// la consola y el error de una importación fallida, donde el motivo suele
/// llegar por stderr mientras stdout va vacío.
fn console_output(result: &CommandResult) -> String {
    let mut output = result.stdout.clone();
    if !result.stderr.trim().is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&result.stderr);
    }
    truncate_text(output.trim_end(), 16_000)
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

/// Límite de RAM del recurso en MB. `memory_unlimited=true` devuelve 0, que las
/// plantillas traducen a un Compose sin `mem_limit`.
fn parse_memory_mb(fields: &HashMap<String, String>, default_mb: u32) -> u32 {
    if field(fields, "memory_unlimited") == "true" {
        return 0;
    }
    optional_field(fields, "memory_mb")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default_mb)
        .clamp(64, 16_384)
}

fn validate_compose_text(compose: &str) -> Result<(), String> {
    if compose.trim().is_empty() {
        return Err("pega el contenido de docker-compose.yml".to_owned());
    }
    // Con formularios URL-encoded, el peor caso ocupa tres veces más bytes.
    // 128 KiB mantiene la solicitud por debajo del límite HTTP de 512 KiB.
    if compose.len() > 128 * 1024 {
        return Err("el Compose supera el máximo de 128 KiB".to_owned());
    }
    if compose.contains('\0') {
        return Err("el Compose contiene caracteres no válidos".to_owned());
    }
    Ok(())
}

fn database_label(database: &str) -> &'static str {
    match database {
        "postgres" => "PostgreSQL",
        "mysql" => "MySQL",
        "mariadb" => "MariaDB",
        "mongodb" => "MongoDB",
        "redis" => "Redis",
        _ => "Base de datos",
    }
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
    parse_environment_with_reserved(raw, &["APP_IMAGE", "TDM_MEMORY_LIMIT"])
}

fn parse_environment_with_reserved(
    raw: &str,
    reserved: &[&str],
) -> Result<Vec<(String, String)>, String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
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
        if reserved.contains(&key) {
            return Err(format!("{key} lo gestiona el panel; usa otro nombre"));
        }
        if pairs.iter().any(|(existing, _)| existing == key) {
            return Err(format!("clave de entorno repetida: {key}"));
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

/// Variables del `.env` que gestiona el panel y el editor nunca debe tocar.
fn managed_environment_keys(project: &Project) -> Vec<String> {
    let mut keys = vec!["TDM_MEMORY_LIMIT".to_owned()];
    if let Some(image_env) = project.image_env.as_ref() {
        if !keys.iter().any(|key| key == image_env) {
            keys.push(image_env.clone());
        }
    }
    keys
}

/// Bloque `CLAVE=valor` editable extraído del `.env` actual.
fn editable_environment(contents: &str, managed: &[String]) -> String {
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            let key = key.trim();
            if !valid_env_key(key) || managed.iter().any(|reserved| reserved == key) {
                return None;
            }
            Some(format!("{key}={}", value.trim()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sustituye las líneas editables del `.env` por las nuevas, conservando
/// comentarios y variables gestionadas por el panel en su sitio.
fn replace_editable_environment(
    original: &str,
    environment: &[(String, String)],
    managed: &[String],
) -> String {
    let mut lines = Vec::new();
    for line in original.lines() {
        let trimmed = line.trim_start();
        let editable_assignment = !trimmed.starts_with('#')
            && trimmed.split_once('=').is_some_and(|(key, _)| {
                let key = key.trim();
                valid_env_key(key) && !managed.iter().any(|reserved| reserved == key)
            });
        if !editable_assignment {
            lines.push(line.to_owned());
        }
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if !lines.is_empty() && !environment.is_empty() {
        lines.push(String::new());
    }
    for (key, value) in environment {
        lines.push(format!("{key}={value}"));
    }
    let mut updated = lines.join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated
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

/// Builds que sobran, dado el listado de más reciente a más antiguo. Conserva
/// las `retention` primeras y nunca devuelve una imagen fijada, aunque sea vieja:
/// la desplegada y la de rollback tienen que sobrevivir a cualquier retención.
fn stale_builds(builds: &[String], retention: usize, pinned: &[String]) -> Vec<String> {
    if retention == 0 {
        return Vec::new();
    }
    builds
        .iter()
        .skip(retention)
        .filter(|build| !pinned.contains(build))
        .cloned()
        .collect()
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
    fn build_retention_keeps_the_newest_and_never_drops_the_rollback() {
        let builds: Vec<String> = ["app:e5", "app:d4", "app:c3", "app:b2", "app:a1"]
            .iter()
            .map(|value| (*value).to_owned())
            .collect();

        // Por omisión sobreviven la desplegada y la anterior; el resto sobra.
        assert_eq!(
            stale_builds(&builds, 2, &[]),
            vec!["app:c3".to_owned(), "app:b2".to_owned(), "app:a1".to_owned()]
        );

        // Salvo que alguna siga siendo el destino de rollback.
        assert_eq!(
            stale_builds(&builds, 2, &["app:a1".to_owned()]),
            vec!["app:c3".to_owned(), "app:b2".to_owned()]
        );

        // Retención 0 desactiva la limpieza; una retención mayor que el listado
        // no borra nada.
        assert!(stale_builds(&builds, 0, &[]).is_empty());
        assert!(stale_builds(&builds, 10, &[]).is_empty());
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
    fn environment_editor_preserves_managed_keys() {
        let original = "APP_IMAGE=tinkiva/demo:abc\nTDM_MEMORY_LIMIT=512m\nOLD=1\n";
        let managed = vec!["TDM_MEMORY_LIMIT".to_owned(), "APP_IMAGE".to_owned()];
        let parsed = parse_environment_with_reserved("NEW=value\nPORT=3000", &["TDM_MEMORY_LIMIT", "APP_IMAGE"]).unwrap();
        let updated = replace_editable_environment(original, &parsed, &managed);

        assert!(updated.contains("APP_IMAGE=tinkiva/demo:abc"));
        assert!(updated.contains("TDM_MEMORY_LIMIT=512m"));
        assert!(updated.contains("NEW=value"));
        assert!(updated.contains("PORT=3000"));
        assert!(!updated.contains("OLD=1"));
    }

    #[test]
    fn environment_editor_rejects_duplicate_keys() {
        assert!(parse_environment_with_reserved("PORT=3000\nPORT=4000", &[]).is_err());
    }

    #[test]
    fn compose_text_requires_content_and_has_a_safe_size_limit() {
        assert!(validate_compose_text("services:\n  app:\n    image: nginx:alpine\n").is_ok());
        assert!(validate_compose_text(" \n").is_err());
        assert!(validate_compose_text(&"x".repeat(128 * 1024 + 1)).is_err());
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
