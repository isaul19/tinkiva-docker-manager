use crate::proc;
use crate::util::{json_string, truncate_text, valid_container_ref};
use std::collections::HashMap;
use std::path::PathBuf;
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
    pub error: Option<String>,
}

impl DockerInfo {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"available\":{},\"server_version\":{},\"error\":{}}}",
            self.available,
            optional_json(&self.server_version),
            optional_json(&self.error),
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
                "\"id\":{},\"name\":{},\"image\":{},\"status\":{},",
                "\"state\":{},\"ports\":{},\"created_at\":{},",
                "\"cpu\":{},\"memory\":{},\"memory_percent\":{},",
                "\"network_io\":{},\"block_io\":{},\"pids\":{}",
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
            optional_json(&self.pids),
        )
    }
}

#[derive(Debug)]
struct ContainerStats {
    cpu: Option<String>,
    memory: Option<String>,
    memory_percent: Option<String>,
    network_io: Option<String>,
    block_io: Option<String>,
    pids: Option<String>,
}

impl DockerClient {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }

    pub fn info(&self) -> DockerInfo {
        match self.run(
            ["version", "--format", "{{.Server.Version}}"],
            Duration::from_secs(10),
        ) {
            Ok(result) if result.success => DockerInfo {
                available: true,
                server_version: non_empty(result.stdout.trim()),
                error: None,
            },
            Ok(result) => DockerInfo {
                available: false,
                server_version: None,
                error: Some(result.summary()),
            },
            Err(error) => DockerInfo {
                available: false,
                server_version: None,
                error: Some(error),
            },
        }
    }

    pub fn containers(&self) -> Result<Vec<ContainerInfo>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.ID}}}}{separator}{{{{.Names}}}}{separator}{{{{.Image}}}}{separator}{{{{.Status}}}}{separator}{{{{.State}}}}{separator}{{{{.Ports}}}}{separator}{{{{.CreatedAt}}}}"
        );
        let result = self.run(
            ["ps", "-a", "--no-trunc", "--format", &format],
            Duration::from_secs(15),
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let mut containers = parse_containers(&result.stdout);
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

    pub fn container_logs(&self, container: &str, tail: usize) -> Result<String, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        let tail = tail.clamp(10, 2000).to_string();
        let result = self.run(
            ["logs", "--timestamps", "--tail", &tail, container],
            Duration::from_secs(15),
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

    fn stats(&self) -> Result<HashMap<String, ContainerStats>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.Name}}}}{separator}{{{{.CPUPerc}}}}{separator}{{{{.MemUsage}}}}{separator}{{{{.MemPerc}}}}{separator}{{{{.NetIO}}}}{separator}{{{{.BlockIO}}}}{separator}{{{{.PIDs}}}}"
        );
        let result = self.run(
            [
                "stats",
                "--no-stream",
                "--all",
                "--no-trunc",
                "--format",
                &format,
            ],
            Duration::from_secs(20),
        )?;
        if !result.success {
            return Err(result.summary());
        }
        let mut stats = HashMap::new();
        for line in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<&str> = line.split(separator).collect();
            if fields.len() == 7 {
                stats.insert(
                    fields[0].to_owned(),
                    ContainerStats {
                        cpu: non_empty(fields[1]),
                        memory: non_empty(fields[2]),
                        memory_percent: non_empty(fields[3]),
                        network_io: non_empty(fields[4]),
                        block_io: non_empty(fields[5]),
                        pids: non_empty(fields[6]),
                    },
                );
            }
        }
        Ok(stats)
    }

    fn run<I, S>(&self, arguments: I, timeout: Duration) -> Result<proc::CommandResult, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        proc::run(
            &self.binary,
            arguments,
            None,
            &[("DOCKER_CLI_HINTS", "false")],
            timeout,
        )
    }
}

fn parse_containers(output: &str) -> Vec<ContainerInfo> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(FIELD_SEPARATOR).collect();
            (fields.len() == 7).then(|| ContainerInfo {
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
            })
        })
        .collect()
}

fn optional_json(value: &Option<String>) -> String {
    value
        .as_deref()
        .map_or_else(|| "null".to_owned(), json_string)
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "--").then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_container_inventory_without_mutating_docker() {
        let line = format!(
            "abc{0}api{0}example/api:1{0}Up 2 minutes{0}running{0}8080/tcp{0}today",
            FIELD_SEPARATOR
        );
        let containers = parse_containers(&line);
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].name, "api");
        assert_eq!(containers[0].state, "running");
    }
}
