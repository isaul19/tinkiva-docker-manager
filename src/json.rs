//! Analizador JSON mínimo, sin dependencias externas.
//!
//! Solo se usa para leer respuestas de APIs remotas (GitHub y Docker Hub).
//! La serialización sigue haciéndose con `format!` y `util::json_string`.

const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut parser = Parser {
            bytes: input.as_bytes(),
            position: 0,
        };
        parser.skip_whitespace();
        let value = parser.value(0)?;
        parser.skip_whitespace();
        if parser.position != parser.bytes.len() {
            return Err("contenido extra tras el valor JSON".to_owned());
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values.as_slice()),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(value) if *value >= 0.0 && value.fract() == 0.0 && *value < 1.8e19 => {
                Some(*value as u64)
            }
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Atajo para `value.get(key)?.as_str()`.
    pub fn string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Self::as_str)
    }

    /// Atajo para `value.get(key)?.as_u64()`.
    pub fn number(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(Self::as_u64)
    }

    /// Atajo para `value.get(key)?.as_array()`.
    pub fn array(&self, key: &str) -> Option<&[Self]> {
        self.get(key).and_then(Self::as_array)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn skip_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b'\t' | b'\n' | b'\r')
        ) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.position += 1;
            Ok(())
        } else {
            Err(format!(
                "se esperaba '{}' en la posición {}",
                char::from(byte),
                self.position
            ))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("JSON demasiado anidado".to_owned());
        }
        match self.peek().ok_or_else(|| "JSON incompleto".to_owned())? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.string().map(Json::String),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.number(),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.position..].starts_with(text.as_bytes()) {
            self.position += text.len();
            Ok(value)
        } else {
            Err(format!("literal JSON inválido en {}", self.position))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(entries));
        }

        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err("objeto JSON mal formado".to_owned()),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(values));
        }

        loop {
            self.skip_whitespace();
            values.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(values));
                }
                _ => return Err("arreglo JSON mal formado".to_owned()),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
            self.position += 1;
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
                self.position += 1;
            }
        }

        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| "número JSON inválido".to_owned())?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("número JSON inválido en {start}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut output = String::new();

        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "cadena JSON sin cerrar".to_owned())?;
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(output);
                }
                b'\\' => {
                    self.position += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| "escape JSON incompleto".to_owned())?;
                    self.position += 1;
                    match escape {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{08}'),
                        b'f' => output.push('\u{0c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => output.push(self.unicode_escape()?),
                        _ => return Err("escape JSON desconocido".to_owned()),
                    }
                }
                byte if byte < 0x20 => {
                    return Err("carácter de control sin escapar en la cadena".to_owned());
                }
                _ => {
                    // Copia el carácter UTF-8 completo.
                    let rest = std::str::from_utf8(&self.bytes[self.position..])
                        .map_err(|_| "cadena JSON no es UTF-8".to_owned())?;
                    let character = rest
                        .chars()
                        .next()
                        .ok_or_else(|| "cadena JSON sin cerrar".to_owned())?;
                    self.position += character.len_utf8();
                    output.push(character);
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let first = self.hex_quad()?;
        if (0xD800..0xDC00).contains(&first) {
            if self.bytes[self.position..].starts_with(b"\\u") {
                self.position += 2;
                let second = self.hex_quad()?;
                if (0xDC00..0xE000).contains(&second) {
                    let combined =
                        0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                    return char::from_u32(combined)
                        .ok_or_else(|| "par sustituto inválido".to_owned());
                }
            }
            return Ok('\u{fffd}');
        }
        char::from_u32(first).ok_or_else(|| "escape unicode inválido".to_owned())
    }

    fn hex_quad(&mut self) -> Result<u32, String> {
        let end = self.position + 4;
        if end > self.bytes.len() {
            return Err("escape unicode incompleto".to_owned());
        }
        let text = std::str::from_utf8(&self.bytes[self.position..end])
            .map_err(|_| "escape unicode inválido".to_owned())?;
        let value =
            u32::from_str_radix(text, 16).map_err(|_| "escape unicode inválido".to_owned())?;
        self.position = end;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_documents() {
        let value = Json::parse(
            r#"{"id":42,"name":"tinkiva","tags":["a","b"],"nested":{"ok":true,"none":null}}"#,
        )
        .unwrap();

        assert_eq!(value.number("id"), Some(42));
        assert_eq!(value.string("name"), Some("tinkiva"));
        assert_eq!(value.array("tags").map(<[Json]>::len), Some(2));
        assert_eq!(value.array("tags").unwrap()[1].as_str(), Some("b"));
        assert_eq!(
            value.get("nested").and_then(|nested| nested.get("ok")),
            Some(&Json::Bool(true))
        );
    }

    #[test]
    fn decodes_escapes_and_surrogates() {
        let value = Json::parse(r#"{"text":"a\n\"b\\cé🚀"}"#).unwrap();
        assert_eq!(value.string("text"), Some("a\n\"b\\cé🚀"));
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(Json::parse("{").is_err());
        assert!(Json::parse(r#"{"a":1}extra"#).is_err());
        assert!(Json::parse(r#"{"a":}"#).is_err());
        assert!(Json::parse(&"[".repeat(64)).is_err());
    }
}
