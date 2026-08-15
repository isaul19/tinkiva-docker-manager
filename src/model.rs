use crate::util::{json_optional, json_string};
use std::path::PathBuf;

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
}

impl Project {
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
                "\"created_at\":{}",
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
