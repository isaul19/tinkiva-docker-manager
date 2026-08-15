use crate::model::{Project, KIND_REPOSITORY};
use crate::proc::{self, CommandResult};
use crate::util::{ json_string, truncate_text, valid_container_ref };
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{ Path, PathBuf };
use std::time::Duration;

const FIELD_SEPARATOR: char = '\u{1f}';

#[derive(Clone)]
pub struct DockerClient {
    binary: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DockerInfo {
    pub available: bool,
    pub server_version: Option<String>,
    pub compose_version: Option<String>,
    pub error: Option<String>,
    pub compose_error: Option<String>,
}

impl DockerInfo {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"available\":{},",
                "\"server_version\":{},",
                "\"compose_version\":{},",
                "\"error\":{},",
                "\"compose_error\":{}",
                "}}"
            ),
            self.available,
            self.server_version.as_deref().map_or_else(|| "null".to_owned(), json_string),
            self.compose_version.as_deref().map_or_else(|| "null".to_owned(), json_string),
            self.error.as_deref().map_or_else(|| "null".to_owned(), json_string),
            self.compose_error.as_deref().map_or_else(|| "null".to_owned(), json_string)
        )
    }
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub state: String,
    pub ports: String,
    pub created_at: String,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub memory_percent: Option<String>,
    pub network_io: Option<String>,
    pub block_io: Option<String>,
    pub pids: Option<String>,
}

impl ContainerInfo {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"id\":{},",
                "\"name\":{},",
                "\"image\":{},",
                "\"status\":{},",
                "\"state\":{},",
                "\"ports\":{},",
                "\"created_at\":{},",
                "\"cpu\":{},",
                "\"memory\":{},",
                "\"memory_percent\":{},",
                "\"network_io\":{},",
                "\"block_io\":{},",
                "\"pids\":{}",
                "}}"
            ),
            json_string(&self.id),
            json_string(&self.name),
            json_string(&self.image),
            json_string(&self.status),
            json_string(&self.state),
            json_string(&self.ports),
            json_string(&self.created_at),
            optional_json(&self.cpu),
            optional_json(&self.memory),
            optional_json(&self.memory_percent),
            optional_json(&self.network_io),
            optional_json(&self.block_io),
            optional_json(&self.pids)
        )
    }
}

fn optional_json(value: &Option<String>) -> String {
    value.as_deref().map_or_else(|| "null".to_owned(), json_string)
}

impl DockerClient {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }

    pub fn info(&self) -> DockerInfo {
        let server = self.run(
            ["version", "--format", "{{.Server.Version}}"],
            None,
            Duration::from_secs(10)
        );

        let Ok(server) = server else {
            return DockerInfo {
                available: false,
                server_version: None,
                compose_version: None,
                error: Some("no se pudo ejecutar Docker".to_owned()),
                compose_error: None,
            };
        };

        if !server.success {
            return DockerInfo {
                available: false,
                server_version: None,
                compose_version: None,
                error: Some(server.summary()),
                compose_error: None,
            };
        }

        let compose = self.run(["compose", "version"], None, Duration::from_secs(10));
        let (compose_version, compose_error) = match compose {
            Ok(result) if result.success => (Some(result.stdout.trim().to_owned()), None),
            Ok(result) => (None, Some(compose_error_message(&result.summary()))),
            Err(error) => (None, Some(error)),
        };

        DockerInfo {
            available: true,
            server_version: Some(server.stdout.trim().to_owned()),
            compose_version,
            error: None,
            compose_error,
        }
    }

    pub fn containers(&self) -> Result<Vec<ContainerInfo>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.ID}}}}{separator}{{{{.Names}}}}{separator}{{{{.Image}}}}{separator}{{{{.Status}}}}{separator}{{{{.State}}}}{separator}{{{{.Ports}}}}{separator}{{{{.CreatedAt}}}}"
        );
        let result = self.run(
            ["ps", "-a", "--no-trunc", "--format", &format],
            None,
            Duration::from_secs(15)
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let mut containers = Vec::new();
        for line in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<&str> = line.split(separator).collect();
            if fields.len() != 7 {
                continue;
            }
            containers.push(ContainerInfo {
                id: fields[0].to_owned(),
                name: fields[1].to_owned(),
                image: fields[2].to_owned(),
                status: fields[3].to_owned(),
                state: fields[4].to_owned(),
                ports: fields[5].to_owned(),
                created_at: fields[6].to_owned(),
                cpu: None,
                memory: None,
                memory_percent: None,
                network_io: None,
                block_io: None,
                pids: None,
            });
        }

        let stats = self.stats().unwrap_or_default();
        for container in &mut containers {
            if let Some(stat) = stats.get(&container.name) {
                container.cpu.clone_from(&stat.cpu);
                container.memory.clone_from(&stat.memory);
                container.memory_percent.clone_from(&stat.memory_percent);
                container.network_io.clone_from(&stat.network_io);
                container.block_io.clone_from(&stat.block_io);
                container.pids.clone_from(&stat.pids);
            }
        }

        containers.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(containers)
    }

    /// Estado agregado de los servicios que pertenecen a un proyecto Compose.
    /// Se consulta por archivo para no depender del nombre que Compose dedujo
    /// para el stack (que no siempre coincide con el slug del panel).
    pub fn project_status(&self, project: &Project) -> Result<&'static str, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.State}}}}{separator}{{{{.Health}}}}{separator}{{{{.ExitCode}}}}"
        );
        let result = self.run(
            ["compose", "-f", &compose, "ps", "-a", "--format", &format],
            project.compose_file.parent(),
            Duration::from_secs(15),
        )?;
        if !result.success {
            return Err(result.summary());
        }
        Ok(summarize_project_status(&result.stdout))
    }

    pub fn container_action(&self, container: &str, action: &str) -> Result<CommandResult, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        let args: Vec<&str> = match action {
            "start" => vec!["start", container],
            "stop" => vec!["stop", "--time", "20", container],
            "restart" => vec!["restart", "--time", "20", container],
            _ => {
                return Err("acción de contenedor no permitida".to_owned());
            }
        };
        self.run(args, None, Duration::from_secs(45))
    }

    pub fn container_logs(&self, container: &str, tail: usize) -> Result<String, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }

        let tail = tail.clamp(10, 2000).to_string();

        let result = self.run(
            ["logs", "--timestamps", "--tail", &tail, container],
            None,
            Duration::from_secs(15)
        )?;

        if !result.success && result.stdout.trim().is_empty() && result.stderr.trim().is_empty() {
            return Err(result.summary());
        }

        let mut output = result.stdout;

        if !result.stderr.trim().is_empty() {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }

            output.push_str(&result.stderr);
        }

        Ok(truncate_text(&output, 2 * 1024 * 1024))
    }

    pub fn compose_logs(&self, project: &Project, tail: usize) -> Result<String, String> {
        let tail = tail.clamp(10, 2000).to_string();
        let compose = project.compose_file.to_string_lossy().into_owned();

        let result = self.run(
            ["compose", "-f", &compose, "logs", "--no-color", "--timestamps", "--tail", &tail],
            project.compose_file.parent(),
            Duration::from_secs(20)
        )?;

        if !result.success && result.stdout.trim().is_empty() && result.stderr.trim().is_empty() {
            return Err(result.summary());
        }

        let mut output = result.stdout;

        if !result.stderr.trim().is_empty() {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }

            output.push_str(&result.stderr);
        }

        Ok(truncate_text(&output, 2 * 1024 * 1024))
    }

    pub fn validate_compose(&self, compose_file: &Path) -> Result<(), String> {
        let compose = compose_file.to_string_lossy().into_owned();
        let result = self.run(
            ["compose", "-f", &compose, "config", "--quiet"],
            compose_file.parent(),
            Duration::from_secs(20)
        )?;
        if result.success {
            Ok(())
        } else {
            Err(result.summary())
        }
    }

    pub fn deploy(&self, project: &Project) -> Result<CommandResult, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        let working_directory = project.compose_file.parent();

        // Los proyectos de GitHub construyen la imagen a partir del clon local:
        // no hay nada que descargar de un registro y `pull` fallaría.
        if project.kind == KIND_REPOSITORY {
            return self.run(
                ["compose", "-f", &compose, "up", "-d", "--build", "--remove-orphans"],
                working_directory,
                Duration::from_secs(1800)
            );
        }

        let pull = self.run(
            ["compose", "-f", &compose, "pull", "--quiet"],
            working_directory,
            Duration::from_secs(300)
        )?;
        if !pull.success {
            return Ok(pull);
        }

        let mut up = self.run(
            ["compose", "-f", &compose, "up", "-d", "--remove-orphans"],
            working_directory,
            Duration::from_secs(300)
        )?;
        if !pull.stdout.trim().is_empty() {
            up.stdout = format!("{}\n{}", pull.stdout.trim(), up.stdout.trim());
        }
        if !pull.stderr.trim().is_empty() {
            up.stderr = format!("{}\n{}", pull.stderr.trim(), up.stderr.trim());
        }
        up.duration_ms = up.duration_ms.saturating_add(pull.duration_ms);
        Ok(up)
    }

    /// Digest inmutable del registry (o ID local como fallback), si la imagen existe.
    pub fn image_revision(&self, image: &str) -> Result<Option<String>, String> {
        let result = self.run(
            [
                "image",
                "inspect",
                "--format",
                "{{range .RepoDigests}}{{println .}}{{end}}",
                image,
            ],
            None,
            Duration::from_secs(20),
        )?;
        if !result.success {
            return Ok(None);
        }
        if let Some(digest) = result
            .stdout
            .split_whitespace()
            .find_map(|entry| entry.split_once('@').map(|(_, digest)| digest.to_owned()))
        {
            return Ok(Some(digest));
        }
        let fallback = self.run(
            ["image", "inspect", "--format", "{{.Id}}", image],
            None,
            Duration::from_secs(20),
        )?;
        Ok(fallback.success.then(|| fallback.stdout.trim().to_owned()).filter(|id| !id.is_empty()))
    }

    /// Actualiza la caché local desde el registry. Docker reutiliza capas y
    /// credenciales existentes, sin mantener otro cliente residente en memoria.
    pub fn pull_image(&self, image: &str) -> Result<CommandResult, String> {
        self.run(["pull", "--quiet", image], None, Duration::from_secs(600))
    }

    /// Aplica `compose up -d` sin `--build`: para una imagen que ya está en el
    /// Docker local (pull del watcher, rollback o restauración automática).
    pub fn deploy_pulled(&self, project: &Project) -> Result<CommandResult, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        self.run(
            ["compose", "-f", &compose, "up", "-d", "--remove-orphans"],
            project.compose_file.parent(),
            Duration::from_secs(300),
        )
    }

    /// Detiene y elimina el stack. `remove_volumes` borra también los datos, por
    /// lo que el panel solo lo pide con confirmación explícita del usuario.
    pub fn compose_down(
        &self,
        project: &Project,
        remove_volumes: bool
    ) -> Result<CommandResult, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        let mut arguments = vec![
            "compose".to_owned(),
            "-f".to_owned(),
            compose,
            "down".to_owned(),
            "--remove-orphans".to_owned()
        ];
        if remove_volumes {
            arguments.push("--volumes".to_owned());
        }
        self.run(arguments, project.compose_file.parent(), Duration::from_secs(180))
    }

    pub fn ensure_network(&self, network: &str) -> Result<(), String> {
        if !valid_container_ref(network) {
            return Err("nombre de red inválido".to_owned());
        }
        let inspect = self.run(["network", "inspect", network], None, Duration::from_secs(10))?;
        if inspect.success {
            return Ok(());
        }
        let create = self.run(["network", "create", network], None, Duration::from_secs(20))?;
        if create.success {
            Ok(())
        } else {
            Err(create.summary())
        }
    }

    fn stats(&self) -> Result<HashMap<String, ContainerStats>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.Name}}}}{separator}{{{{.CPUPerc}}}}{separator}{{{{.MemUsage}}}}{separator}{{{{.MemPerc}}}}{separator}{{{{.NetIO}}}}{separator}{{{{.BlockIO}}}}{separator}{{{{.PIDs}}}}"
        );
        let result = self.run(
            ["stats", "--no-stream", "--all", "--no-trunc", "--format", &format],
            None,
            Duration::from_secs(20)
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let mut stats = HashMap::new();
        for line in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<&str> = line.split(separator).collect();
            if fields.len() != 7 {
                continue;
            }
            stats.insert(fields[0].to_owned(), ContainerStats {
                cpu: non_empty(fields[1]),
                memory: non_empty(fields[2]),
                memory_percent: non_empty(fields[3]),
                network_io: non_empty(fields[4]),
                block_io: non_empty(fields[5]),
                pids: non_empty(fields[6]),
            });
        }
        Ok(stats)
    }

    fn run<I, S>(
        &self,
        arguments: I,
        working_directory: Option<&Path>,
        timeout: Duration
    )
        -> Result<CommandResult, String>
        where I: IntoIterator<Item = S>, S: AsRef<OsStr>
    {
        proc::run(
            &self.binary,
            arguments,
            working_directory,
            &[("DOCKER_CLI_HINTS", "false"), ("COMPOSE_ANSI", "never")],
            timeout
        )
    }
}

fn summarize_project_status(output: &str) -> &'static str {
    let services: Vec<Vec<&str>> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split(FIELD_SEPARATOR).collect())
        .collect();

    if services.is_empty() {
        return "stopped";
    }

    let all_running = services.iter().all(|fields| {
        fields.first().is_some_and(|state| state.eq_ignore_ascii_case("running"))
            && !fields
                .get(1)
                .is_some_and(|health| health.eq_ignore_ascii_case("unhealthy"))
    });
    if all_running {
        return "running";
    }

    let has_error = services.iter().any(|fields| {
        let state = fields.first().copied().unwrap_or_default();
        let health = fields.get(1).copied().unwrap_or_default();
        let exit_code = fields.get(2).copied().unwrap_or_default();
        health.eq_ignore_ascii_case("unhealthy")
            || matches!(state.to_ascii_lowercase().as_str(), "dead" | "restarting")
            || (state.eq_ignore_ascii_case("exited") && exit_code != "0")
    });
    let partially_running = services.iter().any(|fields| {
        fields
            .first()
            .is_some_and(|state| state.eq_ignore_ascii_case("running"))
    });
    if has_error || partially_running {
        "error"
    } else {
        "stopped"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_compose_service_states() {
        let separator = FIELD_SEPARATOR;
        assert_eq!(summarize_project_status(""), "stopped");
        assert_eq!(
            summarize_project_status(&format!("running{separator}healthy{separator}0\n")),
            "running"
        );
        assert_eq!(
            summarize_project_status(&format!("exited{separator}{separator}0\n")),
            "stopped"
        );
        assert_eq!(
            summarize_project_status(&format!("exited{separator}{separator}1\n")),
            "error"
        );
        assert_eq!(
            summarize_project_status(&format!("running{separator}unhealthy{separator}0\n")),
            "error"
        );
        assert_eq!(
            summarize_project_status(&format!(
                "running{separator}healthy{separator}0\nexited{separator}{separator}0\n"
            )),
            "error"
        );
    }
}

fn compose_error_message(error: &str) -> String {
    if error.contains("not a docker command") || error.contains("unknown flag") {
        "Docker Compose no está instalado".to_owned()
    } else {
        truncate_text(error, 500)
    }
}

#[derive(Default)]
struct ContainerStats {
    cpu: Option<String>,
    memory: Option<String>,
    memory_percent: Option<String>,
    network_io: Option<String>,
    block_io: Option<String>,
    pids: Option<String>,
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "--").then(|| value.to_owned())
}
