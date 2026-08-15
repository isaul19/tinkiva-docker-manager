use crate::util::{atomic_write, random_hex, unique_suffix};
use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_CONFIG_DIR: &str = "tinkiva-docker-manager";
const DEFAULT_CONFIG_FILE: &str = "tinkiva.env";
const LEGACY_STATE_DIR: &str = "tinkiva";
const SYSTEM_CONFIG_FILE: &str = "/etc/tinkiva-docker-manager/env";
const DEFAULT_PORT: u16 = 8787;
const DEFAULT_UPDATE_REPO: &str = "isaul19/tinkiva-docker-manager";

pub enum MenuChoice {
    StartBackground,
    StartForeground,
    Reconfigure,
    Update,
    Exit,
}

pub fn stdin_is_interactive() -> bool {
    io::stdin().is_terminal()
}

pub fn config_exists() -> bool {
    read_config_file().is_some()
}

pub fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var("TDM_CONFIG_FILE") {
        return PathBuf::from(path);
    }
    let system = PathBuf::from(SYSTEM_CONFIG_FILE);
    if system.exists() {
        return system;
    }
    let local = PathBuf::from(DEFAULT_CONFIG_DIR).join(DEFAULT_CONFIG_FILE);
    if local.exists() {
        return local;
    }
    let legacy_dir = PathBuf::from(LEGACY_STATE_DIR).join(DEFAULT_CONFIG_FILE);
    if legacy_dir.exists() {
        return legacy_dir;
    }
    let legacy = PathBuf::from(DEFAULT_CONFIG_FILE);
    if legacy.exists() {
        return legacy;
    }
    local
}

pub fn migrate_legacy_state() {
    let legacy_pid = PathBuf::from("tinkiva.pid");
    if legacy_pid.exists() {
        let _ = std::fs::remove_file(&legacy_pid);
    }

    let legacy_dir = PathBuf::from(LEGACY_STATE_DIR);
    let new_dir = PathBuf::from(DEFAULT_CONFIG_DIR);
    if !legacy_dir.exists() || new_dir.exists() {
        return;
    }
    if let Ok(contents) = std::fs::read_to_string(legacy_dir.join("tinkiva.pid")) {
        if let Ok(pid) = contents.trim().parse::<u32>() {
            if Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
        }
    }
    let _ = std::fs::rename(&legacy_dir, &new_dir);
}

pub fn state_root() -> PathBuf {
    let parent = config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if parent.starts_with("/etc") {
        PathBuf::from(DEFAULT_CONFIG_DIR)
    } else {
        parent
    }
}

pub fn read_config_file() -> Option<HashMap<String, String>> {
    let contents = std::fs::read_to_string(config_path()).ok()?;
    let mut settings = HashMap::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if key.starts_with("TDM_") {
                settings.insert(key.to_owned(), value.trim().to_owned());
            }
        }
    }
    if settings
        .get("TDM_ADMIN_TOKEN")
        .is_some_and(|token| valid_token(token))
    {
        Some(settings)
    } else {
        None
    }
}

fn valid_token(value: &str) -> bool {
    (32..=256).contains(&value.len()) && !value.chars().any(char::is_whitespace)
}

pub fn show_menu() -> Result<MenuChoice, String> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    println!("Se encontró configuración previa en {}.", config_path().display());
    println!();
    println!("? ¿Qué deseas hacer?");
    println!("    1) Iniciar el panel en segundo plano");
    println!("    2) Iniciar el panel en primer plano");
    println!("    3) Volver a configurar");
    println!("    4) Buscar e instalar actualizaciones");
    println!("    5) Salir");

    loop {
        let answer = read_line("  Selección [default: 1]: ", &mut reader)?;
        match answer.as_str() {
            "" | "1" => return Ok(MenuChoice::StartBackground),
            "2" => return Ok(MenuChoice::StartForeground),
            "3" => return Ok(MenuChoice::Reconfigure),
            "4" => return Ok(MenuChoice::Update),
            "5" => return Ok(MenuChoice::Exit),
            other => println!("  ✗ Opción no válida: {other}"),
        }
    }
}

pub fn run_wizard(current: Option<&HashMap<String, String>>) -> Result<(), String> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║   Tinkiva Docker Manager — configuración inicial     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    let current_token = current.and_then(|settings| settings.get("TDM_ADMIN_TOKEN")).map(String::as_str);
    let admin_token = ask_token(&mut reader, current_token)?;

    println!();
    let root = state_root();
    let default_data_dir = current
        .and_then(|settings| settings.get("TDM_DATA_DIR"))
        .cloned()
        .unwrap_or_else(|| root.join("data").display().to_string());
    let data_dir = ask_path(
        &mut reader,
        "Directorio de datos (estado local)",
        &default_data_dir,
    )?;
    let default_allowed_root = current
        .and_then(|settings| settings.get("TDM_ALLOWED_ROOT"))
        .cloned()
        .unwrap_or_else(|| root.join("apps").display().to_string());
    let allowed_root = ask_path(
        &mut reader,
        "Raíz permitida para apps Compose",
        &default_allowed_root,
    )?;
    let default_port = current
        .and_then(|settings| settings.get("TDM_BIND"))
        .and_then(|bind| bind.rsplit_once(':'))
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let port = ask_port(&mut reader, default_port)?;

    let bind = format!("127.0.0.1:{port}");
    let path = config_path();
    let contents = format!(
        concat!(
            "# Tinkiva Docker Manager\n",
            "# Generado por el asistente inicial. Edita y reinicia para aplicar cambios.\n",
            "TDM_BIND={}\n",
            "TDM_ADMIN_TOKEN={}\n",
            "TDM_DATA_DIR={}\n",
            "TDM_ALLOWED_ROOT={}\n"
        ),
        bind, admin_token, data_dir, allowed_root
    );
    atomic_write(&path, contents.as_bytes(), 0o600)
        .map_err(|error| format!("no se pudo escribir {}: {error}", path.display()))?;

    println!();
    println!("  ✔ Configuración guardada en {} (permisos 0600)", path.display());
    println!("  ✔ Panel:            http://{bind}");
    println!("  ✔ Datos:            {data_dir}");
    println!("  ✔ Apps Compose:     {allowed_root}");
    if current_token.is_none() {
        println!("  ✔ Token:            {admin_token}");
        println!("  Guárdalo ahora: no se volverá a mostrar completo en pantalla.");
    }
    println!();
    cleanup_install_residuals(&mut reader)?;
    Ok(())
}

fn cleanup_install_residuals(reader: &mut io::StdinLock) -> Result<(), String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("no se pudo leer el directorio actual: {error}"))?;
    let executable = std::env::current_exe().ok();

    let mut residuals: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cwd) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with("tinkiva-docker-manager-linux-")
                && executable.as_ref() != Some(&entry.path())
            {
                residuals.push(entry.path());
            }
        }
    }
    if residuals.is_empty() {
        return Ok(());
    }

    println!("Se encontraron archivos de instalación residuales:");
    for path in &residuals {
        println!("    - {}", path.display());
    }
    let answer = read_line("? ¿Eliminarlos? [default: 1 = sí, 2 = no]: ", reader)?;
    if answer == "2" {
        println!("  Se conservaron los archivos.");
        return Ok(());
    }
    let mut removed = 0;
    for path in &residuals {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    println!("  ✔ {removed} archivo(s) residual(es) eliminado(s).");
    Ok(())
}

pub fn run_self_update(requested_tag: Option<&str>) -> Result<(), String> {
    let repo = std::env::var("TDM_UPDATE_REPO").unwrap_or_else(|_| DEFAULT_UPDATE_REPO.to_owned());
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => return Err(format!("arquitectura no soportada para actualizar: {other}")),
    };

    let tag = match requested_tag {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.starts_with('v') {
                trimmed.to_owned()
            } else {
                format!("v{trimmed}")
            }
        }
        None => {
            println!("Buscando la última versión de {repo}…");
            let latest = fetch_latest_tag(&repo)?;
            let current = env!("CARGO_PKG_VERSION");
            if latest.trim_start_matches('v') == current {
                println!("Ya estás en la última versión ({current}).");
                return Ok(());
            }
            latest
        }
    };

    println!("Descargando {tag} para linux-{arch}…");
    let base = format!(
        "https://github.com/{repo}/releases/download/{tag}/tinkiva-docker-manager-linux-{arch}"
    );
    let directory = std::env::temp_dir().join(format!("tdm-update-{}", unique_suffix()));
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("no se pudo crear el directorio temporal: {error}"))?;
    let binary_path = directory.join("tinkiva-docker-manager");
    let checksum_path = directory.join("checksum.sha256");

    curl_download(&format!("{base}.sha256"), &checksum_path)?;
    curl_download(&base, &binary_path)?;

    let expected = std::fs::read_to_string(&checksum_path)
        .map_err(|error| format!("no se pudo leer la suma descargada: {error}"))?
        .split_whitespace()
        .next()
        .ok_or_else(|| "el archivo de suma está vacío".to_owned())?
        .to_owned();
    let actual = compute_sha256(&binary_path)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        let _ = std::fs::remove_dir_all(&directory);
        return Err("la suma sha256 no coincide; se descartó la descarga".to_owned());
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("no se pudo determinar la ruta del binario: {error}"))?;
    std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("no se pudo dar permisos de ejecución: {error}"))?;
    if let Err(error) = std::fs::rename(&binary_path, &executable) {
        let _ = std::fs::remove_dir_all(&directory);
        if error.kind() == io::ErrorKind::PermissionDenied {
            return Err(format!(
                "sin permisos para reemplazar {}. Ejecuta el mismo comando con sudo.",
                executable.display()
            ));
        }
        return Err(format!("no se pudo reemplazar el binario: {error}"));
    }
    let _ = std::fs::remove_dir_all(&directory);

    println!("✔ Actualizado a {} (antes {}).", tag.trim_start_matches('v'), env!("CARGO_PKG_VERSION"));
    if Path::new("/etc/systemd/system/tinkiva-docker-manager.service").exists() {
        println!("  Reinicia el servicio: sudo systemctl restart tinkiva-docker-manager");
    } else {
        println!("  Vuelve a ejecutar el binario para usar la nueva versión.");
    }
    Ok(())
}

fn fetch_latest_tag(repo: &str) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "30",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: tinkiva-docker-manager",
            &url,
        ])
        .output()
        .map_err(|error| format!("no se pudo ejecutar curl: {error}"))?;
    if !output.status.success() {
        return Err(
            "no se pudo consultar la última versión en GitHub (¿repositorio privado o sin releases?)"
                .to_owned(),
        );
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let marker = "\"tag_name\":\"";
    let start = body
        .find(marker)
        .ok_or_else(|| "respuesta inesperada de GitHub".to_owned())?;
    let tag = body[start + marker.len()..]
        .split('"')
        .next()
        .unwrap_or_default();
    if tag.is_empty() {
        return Err("no se pudo leer la versión publicada".to_owned());
    }
    Ok(tag.to_owned())
}

fn curl_download(url: &str, destination: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "300",
            "--output",
        ])
        .arg(destination)
        .arg(url)
        .status()
        .map_err(|error| format!("no se pudo ejecutar curl: {error}"))?;
    if !status.success() {
        Err(format!("falló la descarga de {url}"))
    } else {
        Ok(())
    }
}

fn compute_sha256(path: &Path) -> Result<String, String> {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|error| format!("no se pudo ejecutar sha256sum: {error}"))?;
    if !output.status.success() {
        return Err("sha256sum terminó con error".to_owned());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "sha256sum no devolvió resultado".to_owned())
}

fn read_line(prompt: &str, reader: &mut io::StdinLock) -> Result<String, String> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|error| format!("no se pudo escribir en stdout: {error}"))?;
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|error| format!("no se pudo leer la entrada: {error}"))?;
    if bytes == 0 {
        return Err(
            "no hay entrada interactiva disponible. Define TDM_ADMIN_TOKEN o crea tinkiva.env"
                .to_owned(),
        );
    }
    Ok(line.trim().to_owned())
}

fn ask_token(reader: &mut io::StdinLock, current: Option<&str>) -> Result<String, String> {
    loop {
        println!("? Token administrador:");
        if let Some(existing) = current {
            println!("    1) Conservar el token actual (recomendado)");
            println!("    2) Generar uno nuevo automáticamente");
            println!("    3) Ingresar mi propio token (32–256 caracteres)");
            let answer = read_line("  Selección [default: 1]: ", reader)?;
            match answer.as_str() {
                "" | "1" => return Ok(existing.to_owned()),
                "2" => return generate_token(),
                "3" => {
                    let token = read_line("  Token: ", reader)?;
                    if valid_token(&token) {
                        return Ok(token);
                    }
                    println!("  ✗ El token debe tener entre 32 y 256 caracteres sin espacios.");
                }
                other => println!("  ✗ Opción no válida: {other}"),
            }
        } else {
            println!("    1) Generar automáticamente (recomendado)");
            println!("    2) Ingresar mi propio token (32–256 caracteres)");
            let answer = read_line("  Selección [default: 1]: ", reader)?;
            match answer.as_str() {
                "" | "1" => return generate_token(),
                "2" => {
                    let token = read_line("  Token: ", reader)?;
                    if valid_token(&token) {
                        return Ok(token);
                    }
                    println!("  ✗ El token debe tener entre 32 y 256 caracteres sin espacios.");
                }
                other => println!("  ✗ Opción no válida: {other}"),
            }
        }
    }
}

fn generate_token() -> Result<String, String> {
    random_hex(24).map_err(|error| format!("no se pudo generar el token: {error}"))
}

fn ask_path(
    reader: &mut io::StdinLock,
    label: &str,
    default: &str,
) -> Result<String, String> {
    loop {
        let answer = read_line(&format!("? {label} [default: {default}]: "), reader)?;
        if answer.is_empty() {
            return Ok(default.to_owned());
        }
        if !answer.contains('\0') && !answer.contains('\r') && !answer.contains('\n') {
            return Ok(answer);
        }
        println!("  ✗ La ruta contiene caracteres no permitidos.");
    }
}

fn ask_port(reader: &mut io::StdinLock, default: u16) -> Result<u16, String> {
    loop {
        let answer = read_line(&format!("? Puerto [default: {default}]: "), reader)?;
        if answer.is_empty() {
            return Ok(default);
        }
        match answer.parse::<u16>() {
            Ok(port) if port >= 1 => return Ok(port),
            _ => println!("  ✗ Ingresa un puerto entre 1 y 65535."),
        }
    }
}
