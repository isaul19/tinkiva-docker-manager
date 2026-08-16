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
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
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
    run_with_input(binary, arguments, working_directory, environment, None, timeout)
}

/// Igual que [`run`] pero escribiendo `input` en la entrada estándar del hijo.
/// Es la forma de pasarle un secreto a un comando sin que aparezca en `argv` y,
/// por tanto, en la salida de `ps` para cualquier usuario del servidor.
pub fn run_with_input<I, S>(
    binary: &Path,
    arguments: I,
    working_directory: Option<&Path>,
    environment: &[(&str, &str)],
    input: Option<&str>,
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
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
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

    if let Some(input) = input {
        // El descriptor se cierra al salir del bloque: sin EOF el hijo esperaría
        // para siempre y el timeout lo mataría sin haber hecho nada.
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes());
        }
    }

    let (status, timed_out) = match wait_with_timeout(&mut child, started, timeout) {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(error);
        }
    };

    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);

    Ok(CommandResult {
        success: status.success() && !timed_out,
        stdout,
        stderr,
        timed_out,
        duration_ms: started.elapsed().as_millis(),
    })
}

/// Resultado de [`run_to_file`]: la salida vive en disco, así que aquí solo
/// viajan el estado y el error, nunca los megabytes del volcado.
#[derive(Debug, Clone)]
pub struct FileCommandResult {
    pub success: bool,
    pub stderr: String,
    pub bytes: u64,
    pub timed_out: bool,
}

impl FileCommandResult {
    pub fn summary(&self) -> String {
        let stderr = self.stderr.trim();
        let message = if !stderr.is_empty() {
            stderr
        } else if self.timed_out {
            "el comando agotó el tiempo de espera"
        } else {
            "el comando no devolvió detalles"
        };
        truncate_text(message, 4000)
    }
}

/// Igual que [`run`] pero dejando stdout directamente en `stdout_path`, sin
/// pasarlo nunca por memoria. Es lo que permite exportar bases de datos de
/// varios gigabytes sin que el panel crezca: el archivo se escribe con 0600 y
/// quien llama es responsable de borrarlo.
pub fn run_to_file<I, S>(
    binary: &Path,
    arguments: I,
    stdout_path: &Path,
    environment: &[(&str, &str)],
    timeout: Duration,
) -> Result<FileCommandResult, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let stderr_path = std::env::temp_dir().join(format!("tdm-{}.stderr", unique_suffix()));

    let stdout_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(stdout_path)
        .map_err(|error| format!("no se pudo crear el archivo de salida: {error}"))?;
    let stderr_file = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&stderr_path)
    {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_file(stdout_path);
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

    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(format!("no se pudo ejecutar {}: {error}", binary.display()));
        }
    };

    let (status, timed_out) = match wait_with_timeout(&mut child, started, timeout) {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = fs::remove_file(stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Err(error);
        }
    };

    // El stderr de un volcado son avisos cortos; se lee entero pero acotado.
    let stderr = truncate_text(&fs::read_to_string(&stderr_path).unwrap_or_default(), 8000);
    let _ = fs::remove_file(&stderr_path);
    let bytes = fs::metadata(stdout_path).map(|meta| meta.len()).unwrap_or(0);

    Ok(FileCommandResult {
        success: status.success() && !timed_out,
        stderr,
        bytes,
        timed_out,
    })
}

/// Espera al hijo sondeando cada 50 ms y lo mata si agota el tiempo. Devuelve
/// `true` en el segundo campo cuando hubo que matarlo.
fn wait_with_timeout(
    child: &mut Child,
    started: Instant,
    timeout: Duration,
) -> Result<(ExitStatus, bool), String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status, false)),
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let status = child
                    .wait()
                    .map_err(|error| format!("no se pudo finalizar el proceso: {error}"))?;
                return Ok((status, true));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no se pudo esperar el proceso: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_of_a_failed_command() {
        let result = run(
            Path::new("/bin/sh"),
            ["-c", "echo hola; echo fallo >&2; exit 3"],
            None,
            &[],
            Duration::from_secs(10),
        )
        .unwrap();

        assert!(!result.success);
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
    fn writes_stdout_to_the_requested_file() {
        let path = std::env::temp_dir().join(format!("tdm-test-{}.sql", unique_suffix()));
        let result = run_to_file(
            Path::new("/bin/sh"),
            ["-c", "echo 'CREATE TABLE t();'; echo aviso >&2"],
            &path,
            &[],
            Duration::from_secs(10),
        )
        .unwrap();

        assert!(result.success);
        assert_eq!(result.stderr.trim(), "aviso");
        assert_eq!(result.bytes, 18);
        assert_eq!(fs::read_to_string(&path).unwrap().trim(), "CREATE TABLE t();");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn redaction_hides_credentials_from_summaries() {
        let result = CommandResult {
            success: false,
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
