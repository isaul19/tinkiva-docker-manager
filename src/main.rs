#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

#[cfg(not(target_os = "linux"))]
compile_error!("Tinkiva Docker Manager solo soporta Linux porque utiliza /proc, df y Docker.");

mod app;
mod buildpack;
mod crypto;
mod daemon;
mod docker;
mod git;
mod github;
mod http;
mod json;
mod metrics;
mod model;
mod net;
mod proc;
mod registry;
mod setup;
mod store;
mod templates;
mod uninstall;
mod util;

use crate::app::{App, Config};
use crate::http::read_request;
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const HELP: &str = "\
Tinkiva Docker Manager — panel de despliegue Docker de un solo nodo

Uso: tmanager [comando]

Comandos:
  start            Arranca el panel en segundo plano (asistente si no hay config)
  stop             Detiene la instancia en segundo plano
  status           Muestra si el panel está en ejecución, pid y URL
  logs [N] [-f]    Muestra las últimas N líneas del log (default 50); -f lo sigue en vivo
  config           Reejecuta el asistente de configuración
  token            Imprime el token administrador
  update [versión] Descarga una release de GitHub (verifica sha256) y se reemplaza
  uninstall        Detiene y elimina la instalación (--purge borra config y datos)
  version          Imprime la versión actual
  help             Muestra esta ayuda

Sin comando abre el menú interactivo (o el asistente la primera vez).
El estado local vive en ./tinkiva-docker-manager/ — config, pid, log, datos y apps.";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) != Some("__serve") {
        setup::migrate_legacy_state();
    }
    let first = arguments.first();
    let outcome = match first.map(String::as_str) {
        Some("__serve") => serve_with_pidfile(),
        Some("start") => run_start(),
        Some("stop") => daemon::stop(),
        Some("status") => daemon::status(),
        Some("logs") => daemon::logs(
            arguments.iter().skip(1).any(|argument| argument == "-f" || argument == "--follow"),
            arguments
                .iter()
                .skip(1)
                .find(|argument| argument.parse::<usize>().is_ok())
                .and_then(|argument| argument.parse::<usize>().ok())
                .filter(|lines| *lines > 0)
                .unwrap_or(50),
        ),
        Some("update") => setup::run_self_update(arguments.get(1).map(String::as_str)),
        Some("config") => setup::run_wizard(setup::read_config_file().as_ref()),
        Some("token") => setup::show_token(),
        Some("uninstall") => uninstall::run(&arguments[1..]),
        Some("help") | Some("--help") | Some("-h") => {
            println!("{HELP}");
            Ok(())
        }
        Some("version") | Some("--version") | Some("-V") => {
            println!("tmanager {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err(format!(
            "comando desconocido: {other}. Ejecuta tmanager help para ver los comandos."
        )),
        None => run(),
    };
    if let Err(error) = outcome {
        eprintln!("tmanager: {error}");
        std::process::exit(1);
    }
}

fn run_start() -> Result<(), String> {
    if std::env::var("TDM_ADMIN_TOKEN").is_err() && !setup::config_exists() {
        if setup::stdin_is_interactive() {
            setup::run_wizard(None)?;
        } else {
            return Err(
                "TDM_ADMIN_TOKEN es obligatorio. Ejecuta el binario en una terminal para el asistente inicial."
                    .to_owned(),
            );
        }
    }
    daemon::start()
}

fn serve_with_pidfile() -> Result<(), String> {
    daemon::write_pid()
        .map_err(|error| format!("no se pudo escribir el archivo pid: {error}"))?;
    let outcome = Config::load().and_then(serve);
    if outcome.is_err() {
        daemon::clear_pid();
    }
    outcome
}

fn run() -> Result<(), String> {
    let config = resolve_config()?;
    serve(config)
}

fn serve(config: Config) -> Result<(), String> {
    let bind = config.bind.clone();
    let workers = config.workers;
    let app = Arc::new(App::new(config)?);

    let listener = TcpListener::bind(&bind)
        .map_err(|error| format!("no se pudo escuchar en {bind}: {error}"))?;
    let (sender, receiver) = mpsc::sync_channel::<TcpStream>(workers.saturating_mul(8).max(8));
    let receiver = Arc::new(Mutex::new(receiver));

    for worker_id in 0..workers {
        let app = Arc::clone(&app);
        let receiver = Arc::clone(&receiver);
        thread::Builder::new()
            .name(format!("tdm-http-{worker_id}"))
            .stack_size(512 * 1024)
            .spawn(move || worker_loop(app, receiver))
            .map_err(|error| format!("no se pudo iniciar worker HTTP: {error}"))?;
    }

    let watcher = Arc::clone(&app);
    let poll_interval = watcher.config().poll_interval_seconds;
    thread::Builder::new()
        .name("tdm-deployment-watcher".to_owned())
        .stack_size(256 * 1024)
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(poll_interval));
            watcher.poll_deployments();
        })
        .map_err(|error| format!("no se pudo iniciar el watcher: {error}"))?;

    eprintln!(
        "Tinkiva Docker Manager {} escuchando en {} con {} workers; polling cada {}s; raíz permitida: {}",
        env!("CARGO_PKG_VERSION"),
        bind,
        workers,
        poll_interval,
        app.config().allowed_root.display()
    );

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                if sender.send(stream).is_err() {
                    return Err("todos los workers HTTP terminaron".to_owned());
                }
            }
            Err(error) => eprintln!("error aceptando conexión: {error}"),
        }
    }
    Ok(())
}

fn resolve_config() -> Result<Config, String> {
    if std::env::var("TDM_ADMIN_TOKEN").is_ok() {
        return Config::load();
    }

    let interactive = setup::stdin_is_interactive();
    if !setup::config_exists() {
        if !interactive {
            return Err(
                "TDM_ADMIN_TOKEN es obligatorio. Ejecuta el binario en una terminal para el asistente inicial."
                    .to_owned(),
            );
        }
        setup::run_wizard(None)?;
        return Config::load();
    }

    if interactive {
        match setup::show_menu()? {
            setup::MenuChoice::StartBackground => {
                daemon::start()?;
                std::process::exit(0);
            }
            setup::MenuChoice::StartForeground => {}
            setup::MenuChoice::Reconfigure => {
                let current = setup::read_config_file();
                setup::run_wizard(current.as_ref())?;
            }
            setup::MenuChoice::Update => {
                setup::run_self_update(None)?;
                std::process::exit(0);
            }
            setup::MenuChoice::Exit => std::process::exit(0),
        }
    }
    Config::load()
}

fn worker_loop(app: Arc<App>, receiver: Arc<Mutex<mpsc::Receiver<TcpStream>>>) {
    loop {
        let stream = {
            let Ok(receiver) = receiver.lock() else {
                eprintln!("cola HTTP bloqueada");
                return;
            };
            receiver.recv()
        };

        let Ok(mut stream) = stream else {
            return;
        };
        handle_connection(&app, &mut stream);
    }
}

fn handle_connection(app: &App, stream: &mut TcpStream) {
    match read_request(stream) {
        Ok(request) => {
            let head_only = request.method == "HEAD";
            let response = app.handle(&request);
            if let Err(error) = response.write_to(stream, head_only) {
                eprintln!("error enviando respuesta HTTP: {error}");
            }
        }
        Err(error) => {
            if let Some(response) = error.response() {
                let _ = response.write_to(stream, false);
            }
        }
    }
}
