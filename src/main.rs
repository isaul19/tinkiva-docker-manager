#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

#[cfg(not(target_os = "linux"))]
compile_error!("Tinkiva Docker Manager solo soporta Linux porque utiliza /proc, df y Docker.");

mod app;
mod docker;
mod http;
mod metrics;
mod model;
mod store;
mod util;

use crate::app::{App, Config};
use crate::http::read_request;
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

fn main() {
    if let Err(error) = run() {
        eprintln!("tinkiva-docker-manager: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = Config::load()?;
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

    eprintln!(
        "Tinkiva Docker Manager {} escuchando en {} con {} workers; raíz permitida: {}",
        env!("CARGO_PKG_VERSION"),
        bind,
        workers,
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
