use crate::util::{json_optional, json_string};
use std::path::PathBuf;

/// Cómo se creó el proyecto. Determina qué acciones ofrece la interfaz y cómo
/// se despliega (`repository` necesita `git pull` antes del `compose up`).
pub const KIND_COMPOSE: &str = "compose";
pub const KIND_DATABASE: &str = "database";
pub const KIND_IMAGE: &str = "image";
pub const KIND_REPOSITORY: &str = "repository";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub slug: String,
    pub name: String,
    pub compose_file: PathBuf,
    pub env_file: Option<PathBuf>,
    pub image_env: Option<String>,
    pub branch: Option<String>,
    pub webhook_token: String,
    pub current_image: Option<String>,
    pub created_at: u64,
    /// Uno de `KIND_*`.
    pub kind: String,
    /// Motor de base de datos (`postgres`, `redis`, …) o slug de icono del servicio.
    pub engine: Option<String>,
    /// Repositorio `owner/name` cuando el origen es GitHub.
    pub repository: Option<String>,
    /// Instalación de la GitHub App que da acceso al repositorio.
    pub installation_id: Option<u64>,
    /// El watcher saliente puede redesplegar este proyecto.
    pub auto_deploy: bool,
    /// Último SHA o digest aplicado correctamente por el watcher.
    pub source_revision: Option<String>,
}

impl Project {
    /// Proyecto Compose clásico: el resto de campos quedan vacíos.
    pub fn compose(
        slug: String,
        name: String,
        compose_file: PathBuf,
        webhook_token: String,
        created_at: u64,
    ) -> Self {
        Self {
            slug,
            name,
            compose_file,
            env_file: None,
            image_env: None,
            branch: None,
            webhook_token,
            current_image: None,
            created_at,
            kind: KIND_COMPOSE.to_owned(),
            engine: None,
            repository: None,
            installation_id: None,
            auto_deploy: false,
            source_revision: None,
        }
    }

    /// Directorio del stack; es donde vive el clon de git en proyectos de GitHub.
    pub fn directory(&self) -> Option<&std::path::Path> {
        self.compose_file.parent()
    }

    pub fn to_json(&self, include_secret: bool) -> String {
        let webhook_token = if include_secret {
            json_string(&self.webhook_token)
        } else {
            "null".to_owned()
        };
        let env_file = self
            .env_file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());

        format!(
            concat!(
                "{{",
                "\"slug\":{},",
                "\"name\":{},",
                "\"compose_file\":{},",
                "\"env_file\":{},",
                "\"image_env\":{},",
                "\"branch\":{},",
                "\"webhook_token\":{},",
                "\"current_image\":{},",
                "\"created_at\":{},",
                "\"kind\":{},",
                "\"engine\":{},",
                "\"repository\":{},",
                "\"installation_id\":{},",
                "\"auto_deploy\":{},",
                "\"source_revision\":{}",
                "}}"
            ),
            json_string(&self.slug),
            json_string(&self.name),
            json_string(&self.compose_file.to_string_lossy()),
            json_optional(env_file.as_deref()),
            json_optional(self.image_env.as_deref()),
            json_optional(self.branch.as_deref()),
            webhook_token,
            json_optional(self.current_image.as_deref()),
            self.created_at,
            json_string(&self.kind),
            json_optional(self.engine.as_deref()),
            json_optional(self.repository.as_deref()),
            self.installation_id
                .map_or_else(|| "null".to_owned(), |id| id.to_string()),
            self.auto_deploy,
            json_optional(self.source_revision.as_deref()),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deployment {
    pub id: u64,
    pub project: String,
    pub created_at: u64,
    pub status: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub image: Option<String>,
    pub previous_image: Option<String>,
    pub message: String,
    pub duration_ms: u128,
    pub trigger: String,
}

impl Deployment {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"id\":{},",
                "\"project\":{},",
                "\"created_at\":{},",
                "\"status\":{},",
                "\"branch\":{},",
                "\"commit\":{},",
                "\"image\":{},",
                "\"previous_image\":{},",
                "\"message\":{},",
                "\"duration_ms\":{},",
                "\"trigger\":{}",
                "}}"
            ),
            self.id,
            json_string(&self.project),
            self.created_at,
            json_string(&self.status),
            json_optional(self.branch.as_deref()),
            json_optional(self.commit.as_deref()),
            json_optional(self.image.as_deref()),
            json_optional(self.previous_image.as_deref()),
            json_string(&self.message),
            self.duration_ms,
            json_string(&self.trigger),
        )
    }
}

#[derive(Clone, Debug)]
pub struct PersistedState {
    pub next_deployment_id: u64,
    pub projects: Vec<Project>,
    pub deployments: Vec<Deployment>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            next_deployment_id: 1,
            projects: Vec::new(),
            deployments: Vec::new(),
        }
    }
}
