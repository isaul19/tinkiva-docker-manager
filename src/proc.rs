use crate::util::{truncate_text, unique_suffix};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_CAPTURE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    pub fn summary(&self) -> String {
        if self.timed_out {
            return "el comando excedió el tiempo máximo".to_owned();
        }
        let stdout = self.stdout.trim();
        let stderr = self.stderr.trim();
        match (stdout.is_empty(), stderr.is_empty()) {
            (false, false) => truncate_text(&format!("{stdout}\n{stderr}"), 16_000),
            (false, true) => truncate_text(stdout, 16_000),
            (true, false) => truncate_text(stderr, 16_000),
            (true, true) if self.success => "comando completado".to_owned(),
            (true, true) => "el comando terminó con error".to_owned(),
        }
    }
}

pub fn run<I, S>(
    binary: &Path,
    arguments: I,
    working_directory: Option<&Path>,
    environment: &[(&str, &str)],
    timeout: Duration,
) -> Result<CommandResult, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let suffix = unique_suffix();
    let stdout_path = std::env::temp_dir().join(format!("tdm-command-{suffix}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("tdm-command-{suffix}.stderr"));
    let result = execute(
        binary,
        arguments,
        working_directory,
        environment,
        timeout,
        &stdout_path,
        &stderr_path,
    );
    let stdout = read_capture(&stdout_path);
    let stderr = read_capture(&stderr_path);
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    let (success, timed_out) = result?;
    Ok(CommandResult {
        success,
        timed_out,
        stdout,
        stderr,
    })
}

fn execute<I, S>(
    binary: &Path,
    arguments: I,
    working_directory: Option<&Path>,
    environment: &[(&str, &str)],
    timeout: Duration,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<(bool, bool), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let stdout = private_file(stdout_path)?;
    let stderr = private_file(stderr_path)?;
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("no se pudo ejecutar {}: {error}", binary.display()))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status.success(), false)),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok((false, true));
            }
            Err(error) => return Err(format!("no se pudo esperar el comando: {error}")),
        }
    }
}

fn private_file(path: &Path) -> Result<fs::File, String> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("no se pudo crear {}: {error}", path.display()))
}

fn read_capture(path: &PathBuf) -> String {
    fs::read(path)
        .map(|bytes| truncate_text(&String::from_utf8_lossy(&bytes), MAX_CAPTURE_BYTES))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_failed_command_output() {
        let result = run(
            Path::new("/bin/sh"),
            ["-c", "printf out; printf err >&2; exit 7"],
            None,
            &[],
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(!result.success);
        assert_eq!(result.stdout, "out");
        assert_eq!(result.stderr, "err");
    }

    #[test]
    fn kills_commands_after_timeout() {
        let result = run(
            Path::new("/bin/sh"),
            ["-c", "sleep 5"],
            None,
            &[],
            Duration::from_millis(50),
        )
        .unwrap();
        assert!(result.timed_out);
        assert!(!result.success);
    }
}
