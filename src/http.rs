use crate::util::{parse_urlencoded, unique_suffix};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{IpAddr, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::Duration;

const MAX_HEADER_BYTES: usize = 32 * 1024;
/// Los webhooks de `push` de GitHub incluyen la lista de commits, así que el
/// límite anterior de 128 KiB se quedaba corto en pushes grandes.
const MAX_BODY_BYTES: usize = 512 * 1024;
/// Las subidas de volcados SQL no pasan por memoria: se escriben en un temporal
/// mientras llegan, así que aquí el techo puede ser el de un `.sql` de verdad.
const MAX_UPLOAD_BYTES: usize = 1024 * 1024 * 1024;
const UPLOAD_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    /// Cuerpo que se guardó en disco en vez de en `body`. Solo lo tienen las
    /// rutas de subida; el handler lo consume y `discard_upload` lo borra.
    pub upload: Option<PathBuf>,
    peer_ip: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Solo confiamos en X-Forwarded-For cuando la conexión viene del Nginx
    /// local; en una conexión remota directa esa cabecera puede falsificarse.
    pub fn client_ip(&self) -> String {
        let peer = self.peer_ip.parse::<IpAddr>().ok();
        if peer.is_some_and(|address| address.is_loopback()) {
            if let Some(forwarded) = self
                .header("x-forwarded-for")
                // El proxy local agrega la IP real al final. Tomar el último
                // elemento evita que una cabecera enviada por el cliente eluda
                // el limitador mediante una IP falsa al principio de la lista.
                .and_then(|value| value.split(',').next_back())
                .map(str::trim)
                .filter(|value| value.parse::<IpAddr>().is_ok())
            {
                return forwarded.to_owned();
            }
        }
        self.peer_ip.clone()
    }

    /// Borra el temporal de la subida. Se llama siempre al cerrar la conexión,
    /// haya ido bien la petición o no: si no, un `.sql` de un giga se quedaría
    /// en /tmp hasta el próximo reinicio.
    pub fn discard_upload(&self) {
        if let Some(path) = &self.upload {
            let _ = fs::remove_file(path);
        }
    }

    pub fn form(&self) -> Result<HashMap<String, String>, String> {
        let content_type = self.header("content-type").unwrap_or_default();
        if !content_type
            .split(';')
            .next()
            .is_some_and(|value| value.trim() == "application/x-www-form-urlencoded")
        {
            return Err("usa Content-Type application/x-www-form-urlencoded".to_owned());
        }
        let body = std::str::from_utf8(&self.body)
            .map_err(|_| "el cuerpo de la solicitud no es UTF-8".to_owned())?;
        parse_urlencoded(body)
    }
}

const STREAM_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug)]
enum Body {
    /// Los archivos estáticos se sirven prestados desde `.rodata`: sin esta
    /// distinción cada petición de `/app.js` copiaría el bundle entero a un
    /// `Vec` nuevo, multiplicando el consumo del panel por número de workers.
    Bytes(Cow<'static, [u8]>),
    /// Cuerpo que vive en disco y se envía por trozos. Un volcado SQL puede
    /// pesar gigabytes: cargarlo en un `Vec` tiraría el panel. El archivo se
    /// borra siempre al terminar, aunque el cliente corte la descarga.
    TemporaryFile { path: PathBuf, length: u64 },
}

impl Body {
    fn length(&self) -> u64 {
        match self {
            Self::Bytes(bytes) => bytes.len() as u64,
            Self::TemporaryFile { length, .. } => *length,
        }
    }
}

#[derive(Debug)]
pub struct Response {
    status: u16,
    content_type: &'static str,
    body: Body,
    headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body: Body::Bytes(Cow::Owned(body)),
            headers: Vec::new(),
        }
    }

    fn asset(content_type: &'static str, body: &'static str) -> Self {
        Self {
            status: 200,
            content_type,
            body: Body::Bytes(Cow::Borrowed(body.as_bytes())),
            headers: Vec::new(),
        }
    }

    /// Descarga servida desde un archivo temporal que se borra al terminar.
    /// `filename` se sanea aquí: solo llega al navegador como ASCII seguro.
    pub fn temporary_file_download(
        path: PathBuf,
        length: u64,
        content_type: &'static str,
        filename: &str,
    ) -> Self {
        let disposition = format!("attachment; filename=\"{}\"", safe_filename(filename));
        Self {
            status: 200,
            content_type,
            body: Body::TemporaryFile { path, length },
            headers: Vec::new(),
        }
        .with_header("Content-Disposition", disposition)
    }

    pub fn json(status: u16, body: String) -> Self {
        Self::new(status, "application/json; charset=utf-8", body.into_bytes())
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::new(
            status,
            "text/plain; charset=utf-8",
            body.into().into_bytes(),
        )
    }

    pub fn html(body: &'static str) -> Self {
        Self::asset("text/html; charset=utf-8", body)
    }

    pub fn javascript(body: &'static str) -> Self {
        Self::asset("text/javascript; charset=utf-8", body)
    }

    pub fn css(body: &'static str) -> Self {
        Self::asset("text/css; charset=utf-8", body)
    }

    pub fn svg(body: &'static str) -> Self {
        Self::asset("image/svg+xml; charset=utf-8", body)
    }

    /// Redirección usada por los retornos del navegador desde GitHub.
    pub fn redirect(location: &str) -> Self {
        Self::new(303, "text/plain; charset=utf-8", Vec::new())
            .with_header("Location", location)
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn write_to(self, stream: &mut TcpStream, head_only: bool) -> io::Result<()> {
        let reason = reason_phrase(self.status);
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.status,
            reason,
            self.content_type,
            self.body.length()
        );

        head.push_str("X-Content-Type-Options: nosniff\r\n");
        head.push_str("X-Frame-Options: DENY\r\n");
        head.push_str("Referrer-Policy: no-referrer\r\n");
        head.push_str("Permissions-Policy: camera=(), microphone=(), geolocation=()\r\n");
        head.push_str("Cross-Origin-Resource-Policy: same-origin\r\n");
        head.push_str("Cache-Control: no-store\r\n");
        // `form-action` incluye github.com porque el alta «un clic» de la GitHub App
        // se hace con un POST del navegador al formulario de manifiesto de GitHub.
        // `img-src` añade los avatares de las cuentas donde está instalada la App.
        head.push_str(concat!(
            "Content-Security-Policy: default-src 'self'; ",
            "connect-src 'self'; script-src 'self'; style-src 'self'; ",
            "img-src 'self' data: https://avatars.githubusercontent.com; ",
            "object-src 'none'; frame-ancestors 'none'; ",
            "base-uri 'none'; form-action 'self' https://github.com\r\n"
        ));

        for (name, value) in self.headers {
            if !name.contains('\r') && !name.contains('\n') && !value.contains('\r') && !value.contains('\n') {
                head.push_str(&name);
                head.push_str(": ");
                head.push_str(&value);
                head.push_str("\r\n");
            }
        }
        head.push_str("\r\n");

        stream.write_all(head.as_bytes())?;
        match self.body {
            Body::Bytes(bytes) => {
                if !head_only {
                    stream.write_all(&bytes)?;
                }
            }
            Body::TemporaryFile { path, .. } => {
                let result = if head_only {
                    Ok(())
                } else {
                    write_file_body(stream, &path)
                };
                // El volcado es de un solo uso: se borra tanto si la descarga
                // terminó como si el navegador cortó a mitad.
                let _ = fs::remove_file(&path);
                result?;
            }
        }
        stream.flush()
    }
}

fn write_file_body(stream: &mut TcpStream, path: &PathBuf) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut chunk = vec![0_u8; STREAM_CHUNK_BYTES];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        stream.write_all(&chunk[..read])?;
    }
}

/// Deja el nombre en ASCII imprimible sin comillas ni separadores de ruta, para
/// que no pueda romper la cabecera `Content-Disposition`.
fn safe_filename(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    if cleaned.is_empty() {
        "descarga".to_owned()
    } else {
        cleaned
    }
}

pub fn read_request(stream: &mut TcpStream) -> Result<Request, HttpReadError> {
    let peer_ip = stream
        .peer_addr()
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_owned());
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(HttpReadError::Io)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(HttpReadError::Io)?;

    let mut buffer = Vec::with_capacity(4096);
    let header_end = loop {
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(HttpReadError::BadRequest("cabeceras demasiado grandes"));
        }

        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).map_err(HttpReadError::Io)?;
        if read == 0 {
            return Err(HttpReadError::BadRequest("solicitud incompleta"));
        }
        buffer.extend_from_slice(&chunk[..read]);

        if let Some(index) = find_subslice(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
    };

    let header_bytes = &buffer[..header_end - 4];
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|_| HttpReadError::BadRequest("cabeceras no válidas"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or(HttpReadError::BadRequest("falta la línea de solicitud"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or(HttpReadError::BadRequest("falta el método"))?;
    let target = request_parts
        .next()
        .ok_or(HttpReadError::BadRequest("falta la ruta"))?;
    let version = request_parts
        .next()
        .ok_or(HttpReadError::BadRequest("falta la versión HTTP"))?;

    if request_parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(HttpReadError::BadRequest("línea de solicitud inválida"));
    }
    if method.len() > 12 || target.len() > 8192 || !target.starts_with('/') {
        return Err(HttpReadError::BadRequest("método o ruta inválidos"));
    }

    let mut headers = HashMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or(HttpReadError::BadRequest("cabecera inválida"))?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name.is_empty()
            || name.len() > 128
            || value.len() > 8192
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value
                .bytes()
                .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
        {
            return Err(HttpReadError::BadRequest("cabecera inválida"));
        }
        if headers.insert(name, value).is_some() {
            return Err(HttpReadError::BadRequest("cabecera duplicada"));
        }
    }

    if headers
        .get("transfer-encoding")
        .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
    {
        return Err(HttpReadError::BadRequest(
            "Transfer-Encoding no está soportado",
        ));
    }

    let content_length = headers
        .get("content-length")
        .map_or(Ok(0), |value| value.parse::<usize>())
        .map_err(|_| HttpReadError::BadRequest("Content-Length inválido"))?;

    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let streamed = is_upload(method, path, &headers);
    if content_length > if streamed { MAX_UPLOAD_BYTES } else { MAX_BODY_BYTES } {
        return Err(HttpReadError::PayloadTooLarge);
    }

    let mut body = Vec::new();
    let mut upload = None;
    if streamed {
        upload = Some(stream_body_to_file(
            stream,
            &buffer[header_end..],
            content_length,
        )?);
    } else {
        body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let remaining = content_length - body.len();
            let mut chunk = vec![0_u8; remaining.min(4096)];
            let read = stream.read(&mut chunk).map_err(HttpReadError::Io)?;
            if read == 0 {
                return Err(HttpReadError::BadRequest("cuerpo incompleto"));
            }
            body.extend_from_slice(&chunk[..read]);
        }
        body.truncate(content_length);
    }

    let query = parse_urlencoded(raw_query)
        .map_err(|_| HttpReadError::BadRequest("query string inválido"))?;

    Ok(Request {
        method: method.to_ascii_uppercase(),
        path: path.to_owned(),
        query,
        headers,
        body,
        upload,
        peer_ip,
    })
}

/// Rutas cuyo cuerpo se escribe en disco en vez de en memoria.
///
/// Se exige la cabecera `Authorization`: el token se valida más adelante, pero
/// sin esta comprobación cualquiera podría hacer que el panel escribiera un giga
/// en `/tmp` antes de recibir su 401.
fn is_upload(method: &str, path: &str, headers: &HashMap<String, String>) -> bool {
    method.eq_ignore_ascii_case("POST")
        && headers.contains_key("authorization")
        && path.starts_with("/api/containers/")
        && path.ends_with("/import")
}

/// Vuelca el cuerpo en un temporal 0600, empezando por lo que ya se leyó junto
/// a las cabeceras. Si algo falla, el archivo se borra antes de devolver.
fn stream_body_to_file(
    stream: &mut TcpStream,
    prefix: &[u8],
    content_length: usize,
) -> Result<PathBuf, HttpReadError> {
    // Subir cientos de megabytes desde una conexión doméstica tiene pausas más
    // largas que los 10 s con los que se leen las cabeceras.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));

    let path = std::env::temp_dir().join(format!("tdm-upload-{}.bin", unique_suffix()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(HttpReadError::Io)?;

    let mut written = prefix.len().min(content_length);
    let outcome = (|| -> Result<(), HttpReadError> {
        file.write_all(&prefix[..written]).map_err(HttpReadError::Io)?;
        let mut chunk = vec![0_u8; UPLOAD_CHUNK_BYTES];
        while written < content_length {
            let wanted = (content_length - written).min(UPLOAD_CHUNK_BYTES);
            let read = stream.read(&mut chunk[..wanted]).map_err(HttpReadError::Io)?;
            if read == 0 {
                return Err(HttpReadError::BadRequest("cuerpo incompleto"));
            }
            file.write_all(&chunk[..read]).map_err(HttpReadError::Io)?;
            written += read;
        }
        file.flush().map_err(HttpReadError::Io)
    })();

    match outcome {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = fs::remove_file(&path);
            Err(error)
        }
    }
}

#[derive(Debug)]
pub enum HttpReadError {
    Io(io::Error),
    BadRequest(&'static str),
    PayloadTooLarge,
}

impl HttpReadError {
    pub fn response(&self) -> Option<Response> {
        match self {
            Self::Io(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset
                        | io::ErrorKind::BrokenPipe
                        | io::ErrorKind::UnexpectedEof
                ) =>
            {
                None
            }
            Self::Io(_) => Some(Response::text(400, "Error leyendo la solicitud")),
            Self::BadRequest(message) => Some(Response::text(400, *message)),
            Self::PayloadTooLarge => Some(Response::text(413, "Cuerpo demasiado grande")),
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Response",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_header_terminator() {
        assert_eq!(find_subslice(b"a\r\n\r\nb", b"\r\n\r\n"), Some(1));
    }

    #[test]
    fn sanitizes_download_filenames() {
        assert_eq!(safe_filename("app_20260815143052.sql"), "app_20260815143052.sql");
        assert_eq!(safe_filename("../etc/passwd"), ".._etc_passwd");
        assert_eq!(safe_filename("a\"\r\nb"), "a___b");
        assert_eq!(safe_filename(""), "descarga");
    }

    #[test]
    fn forwarded_ip_is_only_trusted_from_loopback_and_uses_the_last_hop() {
        let request = |peer_ip: &str, forwarded: &str| Request {
            method: "GET".to_owned(),
            path: "/".to_owned(),
            query: HashMap::new(),
            headers: HashMap::from([("x-forwarded-for".to_owned(), forwarded.to_owned())]),
            body: Vec::new(),
            upload: None,
            peer_ip: peer_ip.to_owned(),
        };

        assert_eq!(
            request("127.0.0.1", "198.51.100.7, 203.0.113.9").client_ip(),
            "203.0.113.9"
        );
        assert_eq!(
            request("203.0.113.10", "198.51.100.7").client_ip(),
            "203.0.113.10"
        );
    }
}
