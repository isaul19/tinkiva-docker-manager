use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub fn unique_suffix() -> String {
    let count = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{count}", std::process::id(), now_unix())
}

pub fn random_hex(bytes: usize) -> io::Result<String> {
    let mut input = File::open("/dev/urandom")?;
    let mut data = vec![0_u8; bytes];
    input.read_exact(&mut data)?;

    let mut output = String::with_capacity(bytes * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in data {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    let max_len = left.len().max(right.len());

    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

pub fn valid_slug(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 48
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

pub fn valid_container_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
}

pub fn valid_env_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn valid_image_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_whitespace)
        && !value.contains('\r') && !value.contains('\n') && !value.contains('\0')
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/' | b'@' | b'+')
        })
}

pub fn valid_db_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && value.len() <= 63
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Nombre de base de datos tal y como lo devuelve el motor. Es más permisivo
/// que [`valid_db_identifier`] (hay bases llamadas `mi-app` o `app.v2`) pero
/// sigue descartando espacios, comillas y cualquier cosa que un cliente de
/// línea de comandos pudiera confundir con una opción.
pub fn valid_schema_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphanumeric() || first == b'_')
        && value.len() <= 64
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'$'))
}

pub fn valid_display_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 100
        && !trimmed.contains('\r') && !trimmed.contains('\n') && !trimmed.contains('\0')
}

/// Un arreglo JSON de cadenas, ya escapadas.
pub fn json_string_array(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|value| json_string(value)).collect();
    format!("[{}]", items.join(","))
}

pub fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", u32::from(character));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

pub fn json_optional(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

pub fn encode_field(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':') {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    output
}

pub fn decode_field(value: &str) -> Result<String, String> {
    percent_decode(value, false)
}

pub fn percent_decode(value: &str, plus_as_space: bool) -> Result<String, String> {
    let input = value.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        match input[index] {
            b'%' => {
                if index + 2 >= input.len() {
                    return Err("secuencia URL incompleta".to_owned());
                }
                let high = hex_value(input[index + 1])
                    .ok_or_else(|| "secuencia URL inválida".to_owned())?;
                let low = hex_value(input[index + 2])
                    .ok_or_else(|| "secuencia URL inválida".to_owned())?;
                output.push((high << 4) | low);
                index += 3;
            }
            b'+' if plus_as_space => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(output).map_err(|_| "texto URL no es UTF-8".to_owned())
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub fn parse_urlencoded(value: &str) -> Result<HashMap<String, String>, String> {
    let mut fields = HashMap::new();
    if value.is_empty() {
        return Ok(fields);
    }

    for pair in value.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key, true)?;
        let value = percent_decode(value, true)?;
        // El editor de Compose acepta YAML de hasta 128 KiB. Codificado como
        // formulario cabe aun en el peor caso dentro del límite HTTP de 512 KiB.
        if key.len() > 128 || value.len() > 128 * 1024 {
            return Err("campo demasiado grande".to_owned());
        }
        fields.insert(key, value);
    }
    Ok(fields)
}

pub fn canonical_existing_within(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("no se pudo resolver la raíz permitida: {error}"))?;
    let candidate = path
        .canonicalize()
        .map_err(|error| format!("no se pudo resolver {}: {error}", path.display()))?;

    if candidate.starts_with(&root) {
        Ok(candidate)
    } else {
        Err(format!(
            "la ruta {} está fuera de {}",
            candidate.display(),
            root.display()
        ))
    }
}

pub fn read_env_value(path: &Path, key: &str) -> io::Result<Option<String>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };

    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((candidate, value)) = trimmed.split_once('=') {
            if candidate.trim() == key {
                return Ok(Some(value.trim().to_owned()));
            }
        }
    }
    Ok(None)
}

pub fn remove_env_key(path: &Path, key: &str) -> io::Result<()> {
    if !valid_env_key(key) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nombre de variable de entorno inválido",
        ));
    }

    let original = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut lines = Vec::new();

    for line in original.lines() {
        let trimmed = line.trim_start();
        let matches_key = !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(candidate, _)| candidate.trim() == key);
        if !matches_key {
            lines.push(line.to_owned());
        }
    }

    let mut updated = lines.join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }
    atomic_write(path, updated.as_bytes(), 0o600)
}

pub fn set_env_value(path: &Path, key: &str, value: &str) -> io::Result<()> {
    if !valid_env_key(key) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nombre de variable de entorno inválido",
        ));
    }
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "valor de variable inválido",
        ));
    }

    let original = fs::read_to_string(path).unwrap_or_default();
    let mut found = false;
    let mut lines = Vec::new();

    for line in original.lines() {
        let trimmed = line.trim_start();
        let matches_key = !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(candidate, _)| candidate.trim() == key);

        if matches_key {
            if !found {
                lines.push(format!("{key}={value}"));
                found = true;
            }
        } else {
            lines.push(line.to_owned());
        }
    }

    if !found {
        lines.push(format!("{key}={value}"));
    }

    let mut updated = lines.join("\n");
    updated.push('\n');
    atomic_write(path, updated.as_bytes(), 0o600)
}

pub fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "archivo sin directorio padre")
    })?;
    fs::create_dir_all(parent)?;

    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tinkiva"),
        unique_suffix()
    ));

    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(mode)
        .open(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))?;
    fs::rename(&temporary, path)?;

    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

pub fn truncate_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }

    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    format!("{}…", &value[..boundary])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation_is_strict() {
        assert!(valid_slug("storagia-api"));
        assert!(valid_slug("a1"));
        assert!(!valid_slug("Storagia"));
        assert!(!valid_slug("-api"));
        assert!(!valid_slug("api-"));
        assert!(!valid_slug("api_dev"));
    }

    #[test]
    fn urlencoded_parser_decodes_values() {
        let fields = parse_urlencoded("name=Hola+Mundo&image=ghcr.io%2Fa%3A1").unwrap();
        assert_eq!(fields.get("name").unwrap(), "Hola Mundo");
        assert_eq!(fields.get("image").unwrap(), "ghcr.io/a:1");
    }

    #[test]
    fn field_encoding_round_trips() {
        let original = "ruta con espacios/áé\nfin";
        assert_eq!(decode_field(&encode_field(original)).unwrap(), original);
    }

    #[test]
    fn schema_names_allow_real_database_names_only() {
        assert!(valid_schema_name("postgres"));
        assert!(valid_schema_name("mi-app"));
        assert!(valid_schema_name("app.v2"));
        assert!(!valid_schema_name(""));
        assert!(!valid_schema_name("-flag"));
        assert!(!valid_schema_name("con espacio"));
        assert!(!valid_schema_name("comi\"lla"));
        assert!(!valid_schema_name(&"a".repeat(65)));
    }

    #[test]
    fn json_escaping_handles_control_characters() {
        assert_eq!(json_string("a\n\"b"), "\"a\\n\\\"b\"");
    }
}
