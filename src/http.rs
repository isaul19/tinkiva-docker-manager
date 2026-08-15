use crate::util::parse_urlencoded;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 128 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
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

#[derive(Debug)]
pub struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            headers: Vec::new(),
        }
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
        Self::new(200, "text/html; charset=utf-8", body.as_bytes().to_vec())
    }

    pub fn javascript(body: &'static str) -> Self {
        Self::new(
            200,
            "text/javascript; charset=utf-8",
            body.as_bytes().to_vec(),
        )
    }

    pub fn css(body: &'static str) -> Self {
        Self::new(200, "text/css; charset=utf-8", body.as_bytes().to_vec())
    }

    pub fn svg(body: &'static str) -> Self {
        Self::new(200, "image/svg+xml; charset=utf-8", body.as_bytes().to_vec())
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
            self.body.len()
        );

        head.push_str("X-Content-Type-Options: nosniff\r\n");
        head.push_str("X-Frame-Options: DENY\r\n");
        head.push_str("Referrer-Policy: no-referrer\r\n");
        head.push_str("Permissions-Policy: camera=(), microphone=(), geolocation=()\r\n");
        head.push_str("Cross-Origin-Resource-Policy: same-origin\r\n");
        head.push_str("Cache-Control: no-store\r\n");
        head.push_str(concat!(
            "Content-Security-Policy: default-src 'self'; ",
            "connect-src 'self'; script-src 'self'; style-src 'self'; ",
            "img-src 'self' data:; object-src 'none'; frame-ancestors 'none'; ",
            "base-uri 'none'; form-action 'self'\r\n"
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
        if !head_only {
            stream.write_all(&self.body)?;
        }
        stream.flush()
    }
}

pub fn read_request(stream: &mut TcpStream) -> Result<Request, HttpReadError> {
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
    if content_length > MAX_BODY_BYTES {
        return Err(HttpReadError::PayloadTooLarge);
    }

    let mut body = buffer[header_end..].to_vec();
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

    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let query = parse_urlencoded(raw_query)
        .map_err(|_| HttpReadError::BadRequest("query string inválido"))?;

    Ok(Request {
        method: method.to_ascii_uppercase(),
        path: path.to_owned(),
        query,
        headers,
        body,
    })
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
}
