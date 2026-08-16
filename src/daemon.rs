use crate::setup;
use std::fs::{ self, OpenOptions };
use std::io;
use std::path::{ Path, PathBuf };
use std::process::{ Command, Stdio };
use std::thread;
use std::time::Duration;

const SERVE_FLAG: &str = "__serve";

fn state_dir() -> PathBuf {
    crate::setup::state_root()
}

pub fn pid_file() -> PathBuf {
    state_dir().join("tinkiva.pid")
}

pub fn log_file() -> PathBuf {
    state_dir().join("tinkiva.log")
}

pub fn is_serve_invocation(argument: Option<&String>) -> bool {
    argument.is_some_and(|value| value == SERVE_FLAG)
}

fn read_pid() -> Option<u32> {
    fs::read_to_string(pid_file()).ok()?.trim().parse().ok()
}

fn is_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

pub fn write_pid() -> io::Result<()> {
    fs::write(pid_file(), std::process::id().to_string())
}

pub fn clear_pid() {
    let _ = fs::remove_file(pid_file());
}

pub fn start() -> Result<(), String> {
    if let Some(pid) = read_pid() {
        if is_alive(pid) {
            return Err(
                format!("ya hay una instancia en ejecución (pid {pid}). Ejecuta stop primero.")
            );
        }
    }
    clear_pid();

    let executable = std::env
        ::current_exe()
        .map_err(|error| format!("no se pudo determinar la ruta del binario: {error}"))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
        .map_err(|error| format!("no se pudo abrir {}: {error}", log_file().display()))?;
    let log_err = log
        .try_clone()
        .map_err(|error| format!("no se pudo duplicar el descriptor de log: {error}"))?;

    let mut command = Command::new("setsid");
    command
        .arg(&executable)
        .arg(SERVE_FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    let spawned = match command.spawn() {
        Ok(child) => Ok(child),
        Err(_) => {
            let plain_log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file())
                .map_err(|error| format!("no se pudo abrir {}: {error}", log_file().display()))?;
            let plain_err = plain_log
                .try_clone()
                .map_err(|error| { format!("no se pudo duplicar el descriptor de log: {error}") })?;
            Command::new(&executable)
                .arg(SERVE_FLAG)
                .stdin(Stdio::null())
                .stdout(Stdio::from(plain_log))
                .stderr(Stdio::from(plain_err))
                .spawn()
        }
    };
    if spawned.is_err() {
        return Err(
            format!(
                "no se pudo iniciar el proceso en segundo plano; revisa {}",
                log_file().display()
            )
        );
    }

    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if let Some(pid) = read_pid() {
            if is_alive(pid) {
                println!("✔ Panel iniciado en segundo plano (pid {pid}).");
                if let Some(bind) = current_bind() {
                    println!("  Panel: http://{bind}");
                }
                println!("  Logs:  {}", log_file().display());
                println!("  Detén el proceso con: tmanager stop");
                return Ok(());
            }
            break;
        }
    }
    Err(
        format!(
            "la instancia no llegó a arrancar. Última línea del log: {} (revisa {})",
            last_log_line().unwrap_or_else(|| "sin salida".to_owned()),
            log_file().display()
        )
    )
}

fn last_log_line() -> Option<String> {
    let contents = fs::read_to_string(log_file()).ok()?;
    contents
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
}

fn current_bind() -> Option<String> {
    if let Ok(bind) = std::env::var("TDM_BIND") {
        return Some(bind);
    }
    setup::read_config_file()?.get("TDM_BIND").cloned()
}

pub fn stop() -> Result<(), String> {
    let Some(pid) = read_pid() else {
        println!("No hay instancia en ejecución.");
        return Ok(());
    };
    if !is_alive(pid) {
        clear_pid();
        println!("La instancia ya no estaba activa; se limpió el archivo pid.");
        return Ok(());
    }

    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map_err(|error| format!("no se pudo ejecutar kill: {error}"))?;
    if !status.success() {
        return Err(format!("no se pudo detener el pid {pid} (¿permisos?). Usa: sudo kill {pid}"));
    }

    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if !is_alive(pid) {
            clear_pid();
            println!("✔ Panel detenido (pid {pid}).");
            return Ok(());
        }
    }

    let forced = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .map_err(|error| format!("no se pudo ejecutar kill: {error}"))?;
    clear_pid();
    if forced.success() {
        println!("✔ Panel detenido forzosamente (pid {pid}).");
        Ok(())
    } else {
        Err(format!("no se pudo detener el pid {pid}"))
    }
}

pub fn status() -> Result<(), String> {
    match read_pid() {
        Some(pid) if is_alive(pid) => {
            println!("En ejecución: pid {pid}.");
            if let Some(bind) = current_bind() {
                println!("Panel: http://{bind}");
            }
            Ok(())
        }
        Some(pid) => {
            println!("No está en ejecución (pid obsoleto {pid}).");
            Ok(())
        }
        None => {
            println!("No está en ejecución.");
            Ok(())
        }
    }
}

pub fn logs(follow: bool, lines: usize) -> Result<(), String> {
    let path = log_file();
    if !path.exists() {
        println!("Aún no existe el log ({})", path.display());
        return Ok(());
    }

    let tail = Command::new("tail")
        .args(["-n", &lines.to_string()])
        .arg(&path)
        .status()
        .map_err(|error| format!("no se pudo ejecutar tail: {error}"))?;
    if !tail.success() {
        return Err("tail terminó con error".to_owned());
    }

    if follow {
        println!("— siguiendo {} (Ctrl+C para salir) —", path.display());
        let follow_status = Command::new("tail")
            .args(["-n", "0", "-f"])
            .arg(&path)
            .status()
            .map_err(|error| format!("no se pudo ejecutar tail: {error}"))?;
        if !follow_status.success() {
            return Err("tail -f terminó con error".to_owned());
        }
    }
    Ok(())
}
