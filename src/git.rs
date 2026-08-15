//! Operaciones de git para los proyectos que vienen de un repositorio GitHub.
//!
//! El token de instalación nunca viaja por `argv` (sería visible en `ps`) ni se
//! escribe en el `remote.origin.url`: se entrega a git mediante un archivo de
//! credenciales temporal con permisos 0600 que se borra al terminar.

use crate::proc::{self, CommandResult};
use crate::util::unique_suffix;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CLONE_TIMEOUT: Duration = Duration::from_secs(900);
const FETCH_TIMEOUT: Duration = Duration::from_secs(300);
const QUICK_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GitClient {
    binary: PathBuf,
}

impl Default for GitClient {
    fn default() -> Self {
        Self::new(PathBuf::from("git"))
    }
}

impl GitClient {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }

    pub fn available(&self) -> bool {
        proc::run(
            &self.binary,
            ["--version"],
            None,
            &[],
            Duration::from_secs(10),
        )
        .is_ok_and(|result| result.success)
    }

    /// Clona `owner/name` en `destination` con historial superficial.
    pub fn clone_repository(
        &self,
        repository: &str,
        branch: &str,
        token: &str,
        destination: &Path,
    ) -> Result<CommandResult, String> {
        let credentials = Credentials::create(token)?;
        let arguments = vec![
            "-c".to_owned(),
            "credential.helper=".to_owned(),
            "-c".to_owned(),
            credentials.helper(),
            "clone".to_owned(),
            "--depth".to_owned(),
            "1".to_owned(),
            "--single-branch".to_owned(),
            "--branch".to_owned(),
            branch.to_owned(),
            format!("https://github.com/{repository}.git"),
            destination.to_string_lossy().into_owned(),
        ];
        let result = proc::run(&self.binary, arguments, None, &environment(), CLONE_TIMEOUT);
        drop(credentials);
        result
    }

    /// Deja el clon existente exactamente en la punta de `branch`.
    pub fn update_repository(
        &self,
        directory: &Path,
        branch: &str,
        token: &str,
    ) -> Result<CommandResult, String> {
        let credentials = Credentials::create(token)?;
        let arguments = vec![
            "-c".to_owned(),
            "credential.helper=".to_owned(),
            "-c".to_owned(),
            credentials.helper(),
            "fetch".to_owned(),
            "--depth".to_owned(),
            "1".to_owned(),
            "origin".to_owned(),
            branch.to_owned(),
        ];
        let fetch = proc::run(
            &self.binary,
            arguments,
            Some(directory),
            &environment(),
            FETCH_TIMEOUT,
        );
        drop(credentials);

        let fetch = fetch?;
        if !fetch.success {
            return Ok(fetch);
        }

        let mut reset = proc::run(
            &self.binary,
            ["reset", "--hard", "FETCH_HEAD"],
            Some(directory),
            &environment(),
            QUICK_TIMEOUT,
        )?;
        if reset.success {
            // Elimina artefactos de compilaciones previas sin tocar archivos ignorados
            // que el usuario haya colocado a propósito.
            let _ = proc::run(
                &self.binary,
                ["clean", "-fd"],
                Some(directory),
                &environment(),
                QUICK_TIMEOUT,
            );
        }
        reset.duration_ms = reset.duration_ms.saturating_add(fetch.duration_ms);
        Ok(reset)
    }

    /// Commit corto en el que está el clon, si se puede determinar.
    pub fn head_commit(&self, directory: &Path) -> Option<String> {
        let result = proc::run(
            &self.binary,
            ["rev-parse", "--short=12", "HEAD"],
            Some(directory),
            &environment(),
            QUICK_TIMEOUT,
        )
        .ok()?;
        result
            .success
            .then(|| result.stdout.trim().to_owned())
            .filter(|commit| !commit.is_empty())
    }
}

fn environment() -> [(&'static str, &'static str); 4] {
    [
        // Sin prompts: si la credencial no sirve, el comando falla en vez de colgarse.
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_ADVICE", "0"),
    ]
}

/// Archivo de credenciales efímero en formato `git-credential-store`.
struct Credentials {
    path: PathBuf,
}

impl Credentials {
    fn create(token: &str) -> Result<Self, String> {
        if token.is_empty() || token.contains(['\n', '\r', '/', '@', ':']) {
            return Err("token de instalación inválido".to_owned());
        }
        let path = std::env::temp_dir().join(format!("tdm-git-{}.cred", unique_suffix()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| format!("no se pudo crear el archivo de credenciales: {error}"))?;
        file.write_all(format!("https://x-access-token:{token}@github.com\n").as_bytes())
            .map_err(|error| format!("no se pudo escribir la credencial: {error}"))?;
        Ok(Self { path })
    }

    fn helper(&self) -> String {
        format!("credential.helper=store --file={}", self.path.display())
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_file_is_private_and_disappears() {
        let path = {
            let credentials = Credentials::create("ghs_token123").unwrap();
            let path = credentials.path.clone();

            let metadata = fs::metadata(&path).unwrap();
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

            let contents = fs::read_to_string(&path).unwrap();
            assert_eq!(contents, "https://x-access-token:ghs_token123@github.com\n");
            assert!(credentials.helper().starts_with("credential.helper=store --file="));
            path
        };
        assert!(!path.exists(), "el archivo de credenciales debe borrarse");
    }

    #[test]
    fn rejects_tokens_that_would_break_the_credential_url() {
        assert!(Credentials::create("").is_err());
        assert!(Credentials::create("con\nsalto").is_err());
        assert!(Credentials::create("con@arroba").is_err());
        assert!(Credentials::create("con:dospuntos").is_err());
    }
}
