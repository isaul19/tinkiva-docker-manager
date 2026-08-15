use crate::model::{Deployment, PersistedState, Project};
use crate::util::{atomic_write, decode_field, encode_field};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

pub struct Store {
    path: PathBuf,
    max_history: usize,
    state: Mutex<PersistedState>,
}

impl Store {
    pub fn load(path: PathBuf, max_history: usize) -> Result<Self, String> {
        let state = if path.exists() {
            let contents = fs::read_to_string(&path)
                .map_err(|error| format!("no se pudo leer {}: {error}", path.display()))?;
            parse_state(&contents)?
        } else {
            PersistedState::default()
        };

        Ok(Self {
            path,
            max_history: max_history.max(10),
            state: Mutex::new(state),
        })
    }

    pub fn projects(&self) -> Result<Vec<Project>, String> {
        let state = self.lock()?;
        let mut projects = state.projects.clone();
        projects.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        Ok(projects)
    }

    pub fn project(&self, slug: &str) -> Result<Option<Project>, String> {
        let state = self.lock()?;
        Ok(state
            .projects
            .iter()
            .find(|project| project.slug == slug)
            .cloned())
    }

    pub fn add_project(&self, project: Project) -> Result<(), String> {
        let mut state = self.lock()?;
        if state
            .projects
            .iter()
            .any(|existing| existing.slug == project.slug)
        {
            return Err("ya existe un proyecto con ese slug".to_owned());
        }
        if state
            .projects
            .iter()
            .any(|existing| existing.compose_file == project.compose_file)
        {
            return Err("ese archivo Compose ya está registrado".to_owned());
        }

        state.projects.push(project);
        self.save_locked(&state)
    }

    pub fn remove_project(&self, slug: &str) -> Result<bool, String> {
        let mut state = self.lock()?;
        let original_len = state.projects.len();
        state.projects.retain(|project| project.slug != slug);
        let removed = original_len != state.projects.len();
        if removed {
            self.save_locked(&state)?;
        }
        Ok(removed)
    }

    pub fn update_current_image(&self, slug: &str, image: Option<String>) -> Result<(), String> {
        let mut state = self.lock()?;
        let project = state
            .projects
            .iter_mut()
            .find(|project| project.slug == slug)
            .ok_or_else(|| "proyecto no encontrado".to_owned())?;
        project.current_image = image;
        self.save_locked(&state)
    }

    pub fn update_source_revision(&self, slug: &str, revision: String) -> Result<(), String> {
        let mut state = self.lock()?;
        let project = state
            .projects
            .iter_mut()
            .find(|project| project.slug == slug)
            .ok_or_else(|| "proyecto no encontrado".to_owned())?;
        project.source_revision = Some(revision);
        self.save_locked(&state)
    }

    pub fn append_deployment(&self, mut deployment: Deployment) -> Result<Deployment, String> {
        let mut state = self.lock()?;
        deployment.id = state.next_deployment_id;
        state.next_deployment_id = state.next_deployment_id.saturating_add(1);
        state.deployments.push(deployment.clone());

        if state.deployments.len() > self.max_history {
            let excess = state.deployments.len() - self.max_history;
            state.deployments.drain(0..excess);
        }

        self.save_locked(&state)?;
        Ok(deployment)
    }

    pub fn history(&self, project: Option<&str>, limit: usize) -> Result<Vec<Deployment>, String> {
        let state = self.lock()?;
        let limit = limit.clamp(1, 500);
        Ok(state
            .deployments
            .iter()
            .rev()
            .filter(|deployment| project.is_none_or(|slug| deployment.project == slug))
            .take(limit)
            .cloned()
            .collect())
    }

    pub fn rollback_target(&self, project: &str) -> Result<Option<String>, String> {
        let state = self.lock()?;
        Ok(state
            .deployments
            .iter()
            .rev()
            .find(|deployment| deployment.project == project && deployment.status == "success")
            .and_then(|deployment| deployment.previous_image.clone())
            .filter(|image| !image.is_empty()))
    }

    pub fn counts(&self) -> Result<(usize, usize), String> {
        let state = self.lock()?;
        Ok((state.projects.len(), state.deployments.len()))
    }

    fn lock(&self) -> Result<MutexGuard<'_, PersistedState>, String> {
        self.state
            .lock()
            .map_err(|_| "el almacén interno quedó bloqueado".to_owned())
    }

    fn save_locked(&self, state: &PersistedState) -> Result<(), String> {
        let encoded = serialize_state(state);
        atomic_write(&self.path, encoded.as_bytes(), 0o600)
            .map_err(|error| format!("no se pudo guardar {}: {error}", self.path.display()))
    }
}

fn serialize_state(state: &PersistedState) -> String {
    let mut output = format!("TDM3\t{}\n", state.next_deployment_id);

    for project in &state.projects {
        let fields = [
            "P".to_owned(),
            encode_field(&project.slug),
            encode_field(&project.name),
            encode_field(&project.compose_file.to_string_lossy()),
            encode_field(
                &project
                    .env_file
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            encode_field(project.image_env.as_deref().unwrap_or_default()),
            encode_field(project.branch.as_deref().unwrap_or_default()),
            encode_field(&project.webhook_token),
            encode_field(project.current_image.as_deref().unwrap_or_default()),
            project.created_at.to_string(),
            encode_field(&project.kind),
            encode_field(project.engine.as_deref().unwrap_or_default()),
            encode_field(project.repository.as_deref().unwrap_or_default()),
            project
                .installation_id
                .map_or_else(String::new, |id| id.to_string()),
            project.auto_deploy.to_string(),
            encode_field(project.source_revision.as_deref().unwrap_or_default()),
        ];
        output.push_str(&fields.join("\t"));
        output.push('\n');
    }

    for deployment in &state.deployments {
        let fields = [
            "D".to_owned(),
            deployment.id.to_string(),
            encode_field(&deployment.project),
            deployment.created_at.to_string(),
            encode_field(&deployment.status),
            encode_field(deployment.branch.as_deref().unwrap_or_default()),
            encode_field(deployment.commit.as_deref().unwrap_or_default()),
            encode_field(deployment.image.as_deref().unwrap_or_default()),
            encode_field(deployment.previous_image.as_deref().unwrap_or_default()),
            deployment.duration_ms.to_string(),
            encode_field(&deployment.trigger),
            encode_field(&deployment.message),
        ];
        output.push_str(&fields.join("\t"));
        output.push('\n');
    }

    output
}

fn parse_state(contents: &str) -> Result<PersistedState, String> {
    let mut lines = contents.lines();
    let header = lines.next().ok_or_else(|| "estado vacío".to_owned())?;
    let mut header_fields = header.split('\t');
    // TDM1 no tenía tipo de proyecto ni origen GitHub; se sigue leyendo tal cual
    // y se reescribe como TDM2 en el primer guardado.
    if !matches!(header_fields.next(), Some("TDM1" | "TDM2" | "TDM3")) {
        return Err("formato de estado no reconocido".to_owned());
    }
    let next_deployment_id = header_fields
        .next()
        .unwrap_or("1")
        .parse::<u64>()
        .map_err(|_| "contador de deployments inválido".to_owned())?;

    let mut state = PersistedState {
        next_deployment_id: next_deployment_id.max(1),
        ..PersistedState::default()
    };

    for (line_number, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.first().copied() {
            Some("P") => state.projects.push(parse_project(&fields).map_err(|error| {
                format!("línea {} del estado: {error}", line_number + 2)
            })?),
            Some("D") => state.deployments.push(parse_deployment(&fields).map_err(|error| {
                format!("línea {} del estado: {error}", line_number + 2)
            })?),
            _ => {
                return Err(format!(
                    "línea {} del estado tiene un tipo desconocido",
                    line_number + 2
                ));
            }
        }
    }

    Ok(state)
}

fn parse_project(fields: &[&str]) -> Result<Project, String> {
    // TDM1 tenía 10 campos, TDM2 14 y TDM3 añade auto-deploy.
    if fields.len() != 10 && fields.len() != 14 && fields.len() != 15 && fields.len() != 16 {
        return Err(format!(
            "proyecto con {} campos; se esperaban 10, 14, 15 o 16",
            fields.len()
        ));
    }

    Ok(Project {
        slug: decode_field(fields[1])?,
        name: decode_field(fields[2])?,
        compose_file: PathBuf::from(decode_field(fields[3])?),
        env_file: optional_path(fields[4])?,
        image_env: optional_string(fields[5])?,
        branch: optional_string(fields[6])?,
        webhook_token: decode_field(fields[7])?,
        current_image: optional_string(fields[8])?,
        created_at: fields[9]
            .parse::<u64>()
            .map_err(|_| "fecha de proyecto inválida".to_owned())?,
        kind: match fields.get(10) {
            Some(raw) => optional_string(raw)?.unwrap_or_else(|| crate::model::KIND_COMPOSE.to_owned()),
            None => crate::model::KIND_COMPOSE.to_owned(),
        },
        engine: match fields.get(11) {
            Some(raw) => optional_string(raw)?,
            None => None,
        },
        repository: match fields.get(12) {
            Some(raw) => optional_string(raw)?,
            None => None,
        },
        installation_id: match fields.get(13).map(|raw| raw.trim()) {
            Some(raw) if !raw.is_empty() => Some(
                raw.parse::<u64>()
                    .map_err(|_| "installation_id inválido".to_owned())?,
            ),
            _ => None,
        },
        auto_deploy: match fields.get(14).map(|raw| raw.trim()) {
            Some("true") => true,
            Some("false") | None => fields.len() == 14 && fields.get(12).is_some_and(|value| !value.is_empty()),
            Some(_) => return Err("auto_deploy inválido".to_owned()),
        },
        source_revision: match fields.get(15) {
            Some(raw) => optional_string(raw)?,
            None => None,
        },
    })
}

fn parse_deployment(fields: &[&str]) -> Result<Deployment, String> {
    if fields.len() != 12 {
        return Err(format!(
            "deployment con {} campos; se esperaban 12",
            fields.len()
        ));
    }

    Ok(Deployment {
        id: fields[1]
            .parse::<u64>()
            .map_err(|_| "id de deployment inválido".to_owned())?,
        project: decode_field(fields[2])?,
        created_at: fields[3]
            .parse::<u64>()
            .map_err(|_| "fecha de deployment inválida".to_owned())?,
        status: decode_field(fields[4])?,
        branch: optional_string(fields[5])?,
        commit: optional_string(fields[6])?,
        image: optional_string(fields[7])?,
        previous_image: optional_string(fields[8])?,
        duration_ms: fields[9]
            .parse::<u128>()
            .map_err(|_| "duración inválida".to_owned())?,
        trigger: decode_field(fields[10])?,
        message: decode_field(fields[11])?,
    })
}

fn optional_string(value: &str) -> Result<Option<String>, String> {
    let decoded = decode_field(value)?;
    Ok((!decoded.is_empty()).then_some(decoded))
}

fn optional_path(value: &str) -> Result<Option<PathBuf>, String> {
    Ok(optional_string(value)?.map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trip_preserves_values() {
        let state = PersistedState {
            next_deployment_id: 9,
            projects: vec![Project {
                slug: "demo-api".to_owned(),
                name: "Demo API".to_owned(),
                compose_file: PathBuf::from("/opt/tinkiva/apps/demo/compose.yaml"),
                env_file: Some(PathBuf::from("/opt/tinkiva/apps/demo/.env")),
                image_env: Some("APP_IMAGE".to_owned()),
                branch: Some("main".to_owned()),
                webhook_token: "abc123".to_owned(),
                current_image: Some("ghcr.io/demo/api:sha".to_owned()),
                created_at: 10,
                kind: crate::model::KIND_REPOSITORY.to_owned(),
                engine: None,
                repository: Some("isaul19/demo".to_owned()),
                installation_id: Some(4242),
                auto_deploy: true,
                source_revision: Some("sha256:abc".to_owned()),
            }],
            deployments: vec![Deployment {
                id: 8,
                project: "demo-api".to_owned(),
                created_at: 11,
                status: "success".to_owned(),
                branch: Some("main".to_owned()),
                commit: Some("abc".to_owned()),
                image: Some("ghcr.io/demo/api:sha".to_owned()),
                previous_image: None,
                message: "desplegado\ncorrectamente".to_owned(),
                duration_ms: 250,
                trigger: "webhook".to_owned(),
            }],
        };

        let decoded = parse_state(&serialize_state(&state)).unwrap();
        assert_eq!(decoded.next_deployment_id, state.next_deployment_id);
        assert_eq!(decoded.projects, state.projects);
        assert_eq!(decoded.deployments, state.deployments);
    }

    #[test]
    fn legacy_tdm1_state_still_loads() {
        let legacy = concat!(
            "TDM1\t3\n",
            "P\tdemo-api\tDemo%20API\t/opt/tinkiva/apps/demo/compose.yaml\t",
            "/opt/tinkiva/apps/demo/.env\tAPP_IMAGE\tmain\ttok\tghcr.io/demo/api:1\t10\n"
        );
        let decoded = parse_state(legacy).unwrap();
        let project = &decoded.projects[0];

        assert_eq!(project.slug, "demo-api");
        assert_eq!(project.name, "Demo API");
        assert_eq!(project.kind, crate::model::KIND_COMPOSE);
        assert_eq!(project.repository, None);
        assert_eq!(project.installation_id, None);
        // Al reescribirlo queda en TDM3 sin perder nada.
        assert!(serialize_state(&decoded).starts_with("TDM3\t3\n"));
    }
}
