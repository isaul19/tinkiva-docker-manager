use crate::util::{atomic_write, random_hex};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const DEFAULT_CONFIG_FILE: &str = "tinkiva.env";
const DEFAULT_DATA_DIR: &str = "./tinkiva/data";
const DEFAULT_ALLOWED_ROOT: &str = "./tinkiva/apps";
const DEFAULT_PORT: u16 = 8787;

pub fn config_path() -> PathBuf {
    std::env::var("TDM_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_FILE))
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
    if settings.get("TDM_ADMIN_TOKEN").is_some_and(|token| valid_token(token)) {
        Some(settings)
    } else {
        None
    }
}

fn valid_token(value: &str) -> bool {
    (32..=256).contains(&value.len()) && !value.chars().any(char::is_whitespace)
}

pub fn run_wizard() -> Result<HashMap<String, String>, String> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║   Tinkiva Docker Manager — configuración inicial     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    let admin_token = ask_token(&mut reader)?;

    println!();
    let data_dir = ask_path(
        &mut reader,
        "Directorio de datos (estado local)",
        DEFAULT_DATA_DIR,
    )?;
    let allowed_root = ask_path(
        &mut reader,
        "Raíz permitida para apps Compose",
        DEFAULT_ALLOWED_ROOT,
    )?;
    let port = ask_port(&mut reader)?;

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
    println!("  ✔ Token:            {admin_token}");
    println!("  Guárdalo ahora: no se volverá a mostrar completo en pantalla.");
    println!();

    let mut settings = HashMap::new();
    settings.insert("TDM_BIND".to_owned(), bind);
    settings.insert("TDM_ADMIN_TOKEN".to_owned(), admin_token);
    settings.insert("TDM_DATA_DIR".to_owned(), data_dir);
    settings.insert("TDM_ALLOWED_ROOT".to_owned(), allowed_root);
    Ok(settings)
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

fn ask_token(reader: &mut io::StdinLock) -> Result<String, String> {
    loop {
        println!("? Token administrador:");
        println!("    1) Generar automáticamente (recomendado)");
        println!("    2) Ingresar mi propio token (32–256 caracteres)");
        let answer = read_line("  Selección [default: 1]: ", reader)?;
        match answer.as_str() {
            "" | "1" => {
                let token = random_hex(24)
                    .map_err(|error| format!("no se pudo generar el token: {error}"))?;
                return Ok(token);
            }
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

fn ask_port(reader: &mut io::StdinLock) -> Result<u16, String> {
    loop {
        let answer = read_line(&format!("? Puerto [default: {DEFAULT_PORT}]: "), reader)?;
        if answer.is_empty() {
            return Ok(DEFAULT_PORT);
        }
        match answer.parse::<u16>() {
            Ok(port) if port >= 1 => return Ok(port),
            _ => println!("  ✗ Ingresa un puerto entre 1 y 65535."),
        }
    }
}
