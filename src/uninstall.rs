use crate::daemon;
use crate::setup;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICE_UNIT: &str = "tinkiva-docker-manager.service";
const SERVICE_FILE: &str = "/etc/systemd/system/tinkiva-docker-manager.service";
const SYSTEM_BINARY: &str = "/usr/local/bin/tmanager";
const SYSTEM_DOC_DIR: &str = "/usr/local/share/doc/tinkiva-docker-manager";
const SYSTEM_CONFIG_DIR: &str = "/etc/tinkiva-docker-manager";
const SYSTEM_DATA_DIR: &str = "/var/lib/tinkiva-docker-manager";
const SERVICE_USER: &str = "tinkiva-docker";

pub const HELP: &str = "\
Uso: tmanager uninstall [--purge] [--yes]

Detiene el panel y elimina el servicio systemd, el binario instalado y la
documentación. Los proyectos Compose (TDM_ALLOWED_ROOT) nunca se borran.

  --purge     Elimina también configuración, datos, historial y el usuario
              del sistema tinkiva-docker.
  --yes, -y   No pide confirmación (necesario en modo no interactivo).";

struct Target {
    path: PathBuf,
    label: &'static str,
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let mut purge = false;
    let mut assume_yes = false;
    for argument in arguments {
        match argument.as_str() {
            "--purge" => purge = true,
            "--yes" | "-y" => assume_yes = true,
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            other => {
                return Err(format!(
                    "opción desconocida para uninstall: {other}. Ejecuta tmanager uninstall --help."
                ));
            }
        }
    }

    let settings = setup::read_config_file();
    let allowed_root = setting("TDM_ALLOWED_ROOT", settings.as_ref());
    let data_dir = setting("TDM_DATA_DIR", settings.as_ref());
    let config_file = setup::config_path();
    let service_installed = Path::new(SERVICE_FILE).exists();

    // Las apps Compose son del usuario: se conservan siempre, incluso con --purge.
    let mut preserved: Vec<PathBuf> = Vec::new();
    keep(&mut preserved, allowed_root.as_deref().map(PathBuf::from));
    keep(&mut preserved, Some(PathBuf::from("/opt/tinkiva/apps")));
    if !purge {
        keep(&mut preserved, Some(config_file.clone()));
        keep(&mut preserved, Some(PathBuf::from(SYSTEM_CONFIG_DIR)));
        keep(&mut preserved, data_dir.as_deref().map(PathBuf::from));
        keep(&mut preserved, Some(PathBuf::from(SYSTEM_DATA_DIR)));
    }

    let mut targets: Vec<Target> = Vec::new();
    add(&mut targets, PathBuf::from(SERVICE_FILE), "servicio systemd");
    add(&mut targets, PathBuf::from(SYSTEM_BINARY), "binario instalado");
    add(&mut targets, PathBuf::from(SYSTEM_DOC_DIR), "documentación instalada");
    add(&mut targets, removable_executable(), "binario en uso");
    add(&mut targets, Some(daemon::pid_file()), "archivo pid");
    add(&mut targets, Some(daemon::log_file()), "log");
    if purge {
        add(&mut targets, PathBuf::from(SYSTEM_CONFIG_DIR), "configuración del sistema");
        add(&mut targets, PathBuf::from(SYSTEM_DATA_DIR), "datos e historial del sistema");
        add(&mut targets, Some(config_file.clone()), "configuración local");
        add(&mut targets, data_dir.map(PathBuf::from), "datos e historial locales");
        add(&mut targets, Some(setup::state_root()), "directorio de estado local");
    }
    targets.retain(|target| {
        fs::symlink_metadata(&target.path).is_ok() && !preserved.contains(&target.path)
    });
    dedup(&mut targets);

    if targets.is_empty() && !service_installed {
        println!("No se encontró ninguna instalación de Tinkiva Docker Manager que eliminar.");
        return Ok(());
    }

    if needs_root(&targets, service_installed) && effective_uid() != Some(0) {
        return Err(
            "la instalación es del sistema; vuelve a ejecutar: sudo tmanager uninstall".to_owned(),
        );
    }

    println!("Se eliminará:");
    if service_installed {
        println!("    - servicio {SERVICE_UNIT} (se detiene y deshabilita)");
    }
    for target in &targets {
        println!("    - {} ({})", target.path.display(), target.label);
    }
    if purge && service_installed {
        println!("    - usuario del sistema {SERVICE_USER}");
    }
    println!("Se conservará:");
    for path in &preserved {
        println!("    - {}", path.display());
    }
    println!("Los contenedores y volúmenes ya desplegados no se tocan.");
    if !purge {
        println!("Usa --purge para eliminar también configuración, datos e historial.");
    }
    if !assume_yes {
        confirm()?;
    }

    if service_installed {
        let _ = Command::new("systemctl").args(["disable", "--now", SERVICE_UNIT]).status();
    } else {
        let _ = daemon::stop();
    }

    let mut removed = 0;
    let mut failures: Vec<String> = Vec::new();
    for target in &targets {
        match remove_path(&target.path, &preserved) {
            Ok(()) => removed += 1,
            Err(error) => {
                failures.push(format!("{}: {error}", target.path.display()));
            }
        }
    }

    if service_installed {
        let _ = Command::new("systemctl").arg("daemon-reload").status();
        if purge {
            let _ = Command::new("userdel").arg(SERVICE_USER).status();
        }
    }

    if !failures.is_empty() {
        return Err(format!(
            "no se pudieron eliminar {} ruta(s): {}",
            failures.len(),
            failures.join("; ")
        ));
    }

    println!("✔ Desinstalado: {removed} ruta(s) eliminada(s).");
    if !purge {
        println!("  Configuración e historial se conservaron; usa --purge para borrarlos.");
    }
    println!("  Tus proyectos Compose y sus contenedores siguen en su sitio.");
    Ok(())
}

fn setting(key: &str, settings: Option<&HashMap<String, String>>) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| settings.and_then(|settings| settings.get(key).cloned()))
}

fn keep(preserved: &mut Vec<PathBuf>, path: Option<PathBuf>) {
    let Some(path) = path.map(|path| absolute(&path)) else { return };
    if path.exists() && !preserved.contains(&path) {
        preserved.push(path);
    }
}

fn add<P: Into<Option<PathBuf>>>(targets: &mut Vec<Target>, path: P, label: &'static str) {
    if let Some(path) = path.into() {
        targets.push(Target { path: absolute(&path), label });
    }
}

fn dedup(targets: &mut Vec<Target>) {
    let mut seen: Vec<PathBuf> = Vec::new();
    targets.retain(|target| {
        if seen.contains(&target.path) {
            false
        } else {
            seen.push(target.path.clone());
            true
        }
    });
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// El binario en uso solo se elimina si no es una compilación del árbol de
/// desarrollo: `cargo build` no debe perder su artefacto por un uninstall.
/// El marcador `.cargo-lock` identifica el directorio de perfil de cargo sin
/// depender del nombre, que `CARGO_TARGET_DIR` puede cambiar.
fn removable_executable() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let parent = executable.parent()?;
    if parent.join(".cargo-lock").exists() {
        return None;
    }
    Some(executable)
}

fn needs_root(targets: &[Target], service_installed: bool) -> bool {
    service_installed
        || targets.iter().any(|target| {
            ["/etc", "/usr", "/var", "/opt"]
                .iter()
                .any(|prefix| target.path.starts_with(prefix))
        })
}

fn effective_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("Uid:")?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    })
}

/// Borra `path`, pero si alguna ruta conservada vive dentro se recorre entrada
/// por entrada para no llevarse por delante las apps del usuario (por defecto
/// el asistente las coloca en `<estado>/apps`).
fn remove_path(path: &Path, preserved: &[PathBuf]) -> io::Result<()> {
    if preserved.iter().any(|kept| kept == path) {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return fs::remove_file(path);
    }
    if !preserved.iter().any(|kept| kept.starts_with(path)) {
        return fs::remove_dir_all(path);
    }
    for entry in fs::read_dir(path)? {
        remove_path(&entry?.path(), preserved)?;
    }
    // Queda en pie si aún contiene algo conservado; eso es exactamente lo buscado.
    let _ = fs::remove_dir(path);
    Ok(())
}

fn confirm() -> Result<(), String> {
    if !io::stdin().is_terminal() {
        return Err(
            "sin terminal interactiva: repite el comando con --yes para confirmar".to_owned()
        );
    }
    print!("? Escribe 'si' para continuar: ");
    io::stdout()
        .flush()
        .map_err(|error| format!("no se pudo escribir en stdout: {error}"))?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|error| format!("no se pudo leer la entrada: {error}"))?;
    match line.trim().to_ascii_lowercase().as_str() {
        "si" | "sí" | "s" | "yes" | "y" => Ok(()),
        _ => Err("desinstalación cancelada; no se eliminó nada".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::unique_suffix;

    #[test]
    fn removing_state_dir_keeps_user_apps() {
        let root = std::env::temp_dir().join(format!("tdm-uninstall-{}", unique_suffix()));
        let apps = root.join("apps");
        fs::create_dir_all(apps.join("demo")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(apps.join("demo/compose.yaml"), b"services: {}").unwrap();
        fs::write(root.join("tinkiva.env"), b"TDM_BIND=127.0.0.1:8787").unwrap();

        remove_path(&root, std::slice::from_ref(&apps)).unwrap();

        assert!(apps.join("demo/compose.yaml").exists());
        assert!(!root.join("data").exists());
        assert!(!root.join("tinkiva.env").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn removing_plain_dir_takes_everything() {
        let root = std::env::temp_dir().join(format!("tdm-uninstall-{}", unique_suffix()));
        fs::create_dir_all(root.join("doc")).unwrap();
        fs::write(root.join("doc/README.md"), b"hola").unwrap();

        remove_path(&root, &[]).unwrap();

        assert!(!root.exists());
    }

    #[test]
    fn cargo_artifacts_are_never_removed() {
        assert!(!needs_root(
            &[Target { path: PathBuf::from("/home/user/tinkiva-docker-manager"), label: "x" }],
            false
        ));
        assert!(needs_root(
            &[Target { path: PathBuf::from(SYSTEM_BINARY), label: "x" }],
            false
        ));
    }
}
