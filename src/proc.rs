//! Lanzador de subprocesos compartido por `docker` y `git`.
//!
//! Redirige stdout/stderr a archivos temporales con permisos 0600 en lugar de a
//! tuberías: evita bloqueos cuando el proceso hijo escribe más de lo que cabe en
//! el búfer del pipe y mantiene el uso de memoria del panel constante aunque el
//! comando produzca megabytes de salida.

use crate::util::{truncate_text, unique_suffix};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u128,
}

impl CommandResult {
    pub fn summary(&self) -> String {
        let stdout = self.stdout.trim();
        let stderr = self.stderr.trim();
        let message = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else if self.timed_out {
            "el comando agotó el tiempo de espera"
        } else {
            "el comando no devolvió detalles"
        };
        truncate_text(message, 4000)
    }

    /// Igual que [`Self::summary`] pero eliminando cualquier credencial que el
    /// subproceso haya podido reflejar en su salida.
    pub fn redacted_summary(&self, secrets: &[&str]) -> String {
        let mut summary = self.summary();
        for secret in secrets {
            if secret.len() >= 8 {
                summary = summary.replace(secret, "***");
            }
        }
        summary
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
    let temporary_directory = std::env::temp_dir();
    let suffix = unique_suffix();
    let stdout_path = temporary_directory.join(format!("tdm-{suffix}.stdout"));
    let stderr_path = temporary_directory.join(format!("tdm-{suffix}.stderr"));

    let stdout_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&stdout_path)
        .map_err(|error| format!("no se pudo crear salida temporal: {error}"))?;
    let stderr_file = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&stderr_path)
    {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_file(&stdout_path);
            return Err(format!("no se pudo crear error temporal: {error}"));
        }
    };

    let mut command = Command::new(binary);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    for (key, value) in environment {
        command.env(key, value);
    }
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }

    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(format!("no se pudo ejecutar {}: {error}", binary.display()));
        }
    };

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child
                    .wait()
                    .map_err(|error| format!("no se pudo finalizar el proceso: {error}"))?;
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(format!("no se pudo esperar el proceso: {error}"));
            }
        }
    };

    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);

    Ok(CommandResult {
        success: status.success() && !timed_out,
        code: status.code(),
        stdout,
        stderr,
        timed_out,
        duration_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_exit_code() {
        let result = run(
            Path::new("/bin/sh"),
            ["-c", "echo hola; echo fallo >&2; exit 3"],
            None,
            &[],
            Duration::from_secs(10),
        )
        .unwrap();

        assert!(!result.success);
        assert_eq!(result.code, Some(3));
        assert_eq!(result.stdout.trim(), "hola");
        assert_eq!(result.summary(), "fallo");
    }

    #[test]
    fn kills_processes_that_exceed_the_timeout() {
        let result = run(
            Path::new("/bin/sh"),
            ["-c", "sleep 30"],
            None,
            &[],
            Duration::from_millis(300),
        )
        .unwrap();

        assert!(result.timed_out);
        assert!(!result.success);
    }

    #[test]
    fn redaction_hides_credentials_from_summaries() {
        let result = CommandResult {
            success: false,
            code: Some(128),
            stdout: String::new(),
            stderr: "fatal: no se pudo acceder con ghs_tokensupersecreto".to_owned(),
            timed_out: false,
            duration_ms: 5,
        };
        let summary = result.redacted_summary(&["ghs_tokensupersecreto"]);
        assert!(summary.contains("***"));
        assert!(!summary.contains("ghs_tokensupersecreto"));
    }
}
