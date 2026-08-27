use crate::util::{atomic_write, decode_field, encode_field, now_unix, random_hex};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const FORMAT: &str = "TDM_AUTH1";
const ATTEMPTS_FORMAT: &str = "TDM_AUTH_ATTEMPTS1";
const SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;
const SHORT_LOCK_SECONDS: u64 = 60;
const LONG_LOCK_SECONDS: u64 = 24 * 60 * 60;
const MAX_FAILURES: u8 = 3;
const MAX_TRACKED_CLIENTS: usize = 4096;

#[derive(Clone)]
struct Credential {
    username: String,
    password_hash: String,
    must_change: bool,
}

struct Session {
    expires_at: u64,
}

#[derive(Default)]
struct Attempt {
    consecutive_failures: u8,
    locked_until: u64,
}

#[derive(Debug)]
pub struct LoginSuccess {
    pub token: String,
    pub must_change: bool,
}

#[derive(Debug)]
pub struct LoginBlocked {
    pub retry_after_seconds: u64,
    pub day_lock: bool,
}

#[derive(Debug)]
pub enum LoginError {
    Blocked(LoginBlocked),
    Internal(String),
}

pub struct Auth {
    path: PathBuf,
    attempts_path: PathBuf,
    credential: Mutex<Credential>,
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<String, Attempt>>,
}

impl Auth {
    pub fn load(path: PathBuf, initial_user: &str, initial_password: &str) -> Result<Self, String> {
        let credential = if path.exists() {
            parse_credential(&path)?
        } else {
            validate_username(initial_user)?;
            validate_password(initial_password)?;
            let credential = Credential {
                username: initial_user.to_owned(),
                password_hash: hash_password(initial_password)?,
                must_change: true,
            };
            persist_credential(&path, &credential)?;
            credential
        };

        let attempts_path = path.with_extension("attempts.conf");
        let attempts = parse_attempts(&attempts_path)?;
        Ok(Self {
            path,
            attempts_path,
            credential: Mutex::new(credential),
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(attempts),
        })
    }

    pub fn login(
        &self,
        client: &str,
        username: &str,
        password: &str,
    ) -> Result<LoginSuccess, LoginError> {
        let now = now_unix();
        {
            let attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|lock| lock.into_inner());
            if let Some(attempt) = attempts
                .get(client)
                .filter(|attempt| attempt.locked_until > now)
            {
                return Err(LoginError::Blocked(LoginBlocked {
                    retry_after_seconds: attempt.locked_until - now,
                    day_lock: attempt.consecutive_failures >= MAX_FAILURES,
                }));
            }
        }

        let credential = self
            .credential
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .clone();
        let valid_user = username == credential.username;
        // Verificamos siempre el hash para que un usuario inexistente no sea distinguible
        // por tiempo de respuesta.
        let valid_password = verify_password(password, &credential.password_hash);
        if !(valid_user && valid_password) {
            let mut attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|lock| lock.into_inner());
            if !attempts.contains_key(client) && attempts.len() >= MAX_TRACKED_CLIENTS {
                if let Some(oldest) = attempts
                    .iter()
                    .min_by_key(|(_, attempt)| attempt.locked_until)
                    .map(|(client, _)| client.clone())
                {
                    attempts.remove(&oldest);
                }
            }
            let attempt = attempts.entry(client.to_owned()).or_default();
            attempt.consecutive_failures = attempt.consecutive_failures.saturating_add(1);
            let day_lock = attempt.consecutive_failures >= MAX_FAILURES;
            let duration = if day_lock {
                LONG_LOCK_SECONDS
            } else {
                SHORT_LOCK_SECONDS
            };
            attempt.locked_until = now.saturating_add(duration);
            if let Err(error) = persist_attempts(&self.attempts_path, &attempts) {
                eprintln!("autenticación: {error}");
            }
            return Err(LoginError::Blocked(LoginBlocked {
                retry_after_seconds: duration,
                day_lock,
            }));
        }

        {
            let mut attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|lock| lock.into_inner());
            if attempts.remove(client).is_some() {
                persist_attempts(&self.attempts_path, &attempts).map_err(LoginError::Internal)?;
            }
        }
        let token = self.issue_session(now).map_err(LoginError::Internal)?;
        Ok(LoginSuccess {
            token,
            must_change: credential.must_change,
        })
    }

    pub fn authorize(&self, token: &str, allow_password_change: bool) -> bool {
        let now = now_unix();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|lock| lock.into_inner());
        sessions.retain(|_, session| session.expires_at > now);
        if !sessions.contains_key(token) {
            return false;
        }
        allow_password_change
            || !self
                .credential
                .lock()
                .unwrap_or_else(|lock| lock.into_inner())
                .must_change
    }

    pub fn change_password(&self, token: &str, password: &str) -> Result<String, String> {
        if !self.authorize(token, true) {
            return Err("la sesión ya no es válida".to_owned());
        }
        validate_password(password)?;

        let mut updated = self
            .credential
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .clone();
        if verify_password(password, &updated.password_hash) {
            return Err(
                "la nueva contraseña debe ser diferente de la contraseña actual".to_owned(),
            );
        }
        updated.password_hash = hash_password(password)?;
        updated.must_change = false;
        persist_credential(&self.path, &updated)?;
        *self
            .credential
            .lock()
            .unwrap_or_else(|lock| lock.into_inner()) = updated;

        self.sessions
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .clear();
        self.issue_session(now_unix())
    }

    fn issue_session(&self, now: u64) -> Result<String, String> {
        let token =
            random_hex(32).map_err(|error| format!("no se pudo crear la sesión: {error}"))?;
        self.sessions
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .insert(
                token.clone(),
                Session {
                    expires_at: now.saturating_add(SESSION_TTL_SECONDS),
                },
            );
        Ok(token)
    }
}

fn validate_username(username: &str) -> Result<(), String> {
    if !(3..=64).contains(&username.len())
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(
            "TDM_ADMIN_USER debe tener 3–64 caracteres alfanuméricos, '.', '_' o '-'".to_owned(),
        );
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), String> {
    if !(12..=256).contains(&password.len())
        || password.chars().any(|character| character.is_control())
    {
        return Err("la contraseña debe tener entre 12 y 256 caracteres".to_owned());
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, String> {
    let entropy = random_hex(16).map_err(|error| format!("no se pudo generar la sal: {error}"))?;
    let salt = SaltString::encode_b64(entropy.as_bytes())
        .map_err(|error| format!("no se pudo codificar la sal: {error}"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("no se pudo proteger la contraseña: {error}"))
}

fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

fn persist_credential(path: &Path, credential: &Credential) -> Result<(), String> {
    let contents = format!(
        "{FORMAT}\nuser={}\nhash={}\nmust_change={}\n",
        encode_field(&credential.username),
        encode_field(&credential.password_hash),
        u8::from(credential.must_change),
    );
    atomic_write(path, contents.as_bytes(), 0o600)
        .map_err(|error| format!("no se pudo guardar {}: {error}", path.display()))
}

fn parse_credential(path: &Path) -> Result<Credential, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("no se pudo leer {}: {error}", path.display()))?;
    let mut lines = contents.lines();
    if lines.next() != Some(FORMAT) {
        return Err(format!(
            "{} tiene un formato de autenticación desconocido",
            path.display()
        ));
    }
    let mut fields = HashMap::new();
    for line in lines {
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key, decode_field(value)?);
        }
    }
    let credential = Credential {
        username: fields
            .remove("user")
            .ok_or_else(|| "falta user en auth.conf".to_owned())?,
        password_hash: fields
            .remove("hash")
            .ok_or_else(|| "falta hash en auth.conf".to_owned())?,
        must_change: fields.get("must_change").is_some_and(|value| value == "1"),
    };
    validate_username(&credential.username)?;
    PasswordHash::new(&credential.password_hash)
        .map_err(|_| "el hash de auth.conf no es válido".to_owned())?;
    Ok(credential)
}

fn persist_attempts(path: &Path, attempts: &HashMap<String, Attempt>) -> Result<(), String> {
    let mut contents = String::from(ATTEMPTS_FORMAT);
    contents.push('\n');
    let mut clients: Vec<_> = attempts.iter().collect();
    clients.sort_by_key(|(client, _)| *client);
    for (client, attempt) in clients {
        contents.push_str(&format!(
            "{}\t{}\t{}\n",
            encode_field(client),
            attempt.consecutive_failures,
            attempt.locked_until,
        ));
    }
    atomic_write(path, contents.as_bytes(), 0o600)
        .map_err(|error| format!("no se pudo guardar {}: {error}", path.display()))
}

fn parse_attempts(path: &Path) -> Result<HashMap<String, Attempt>, String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(format!("no se pudo leer {}: {error}", path.display())),
    };
    let mut lines = contents.lines();
    if lines.next() != Some(ATTEMPTS_FORMAT) {
        return Err(format!(
            "{} tiene un formato de intentos desconocido",
            path.display()
        ));
    }
    let mut attempts = HashMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let mut fields = line.split('\t');
        let client = decode_field(fields.next().unwrap_or_default())?;
        let consecutive_failures = fields
            .next()
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| "conteo inválido en auth-attempts.conf".to_owned())?;
        let locked_until = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| "bloqueo inválido en auth-attempts.conf".to_owned())?;
        if client.is_empty() || fields.next().is_some() {
            return Err("cliente inválido en auth-attempts.conf".to_owned());
        }
        attempts.insert(
            client,
            Attempt {
                consecutive_failures,
                locked_until,
            },
        );
    }
    Ok(attempts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::unique_suffix;

    fn temporary_auth() -> (Auth, PathBuf) {
        let path = std::env::temp_dir().join(format!("tdm-auth-{}.conf", unique_suffix()));
        let auth = Auth::load(path.clone(), "admin", "initial-password-123").unwrap();
        (auth, path)
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(path.with_extension("attempts.conf"));
    }

    #[test]
    fn password_policy_rejects_short_values() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("correct-horse-battery").is_ok());
    }

    #[test]
    fn password_hashes_are_salted_and_verifiable() {
        let first = hash_password("correct-horse-battery").unwrap();
        let second = hash_password("correct-horse-battery").unwrap();
        assert_ne!(first, second);
        assert!(verify_password("correct-horse-battery", &first));
        assert!(!verify_password("wrong-password", &first));
    }

    #[test]
    fn first_login_requires_a_new_password() {
        let (auth, path) = temporary_auth();
        let login = auth
            .login("192.0.2.10", "admin", "initial-password-123")
            .unwrap();
        assert!(login.must_change);
        assert!(!auth.authorize(&login.token, false));

        let new_token = auth
            .change_password(&login.token, "replacement-password-456")
            .unwrap();
        assert!(auth.authorize(&new_token, false));
        assert!(!auth.authorize(&login.token, false));

        let reloaded = Auth::load(path.clone(), "ignored", "ignored-password").unwrap();
        let login = reloaded
            .login("192.0.2.10", "admin", "replacement-password-456")
            .unwrap();
        assert!(!login.must_change);
        cleanup(&path);
    }

    #[test]
    fn third_consecutive_failure_locks_for_a_day() {
        let (auth, path) = temporary_auth();
        for expected_day_lock in [false, false, true] {
            let error = auth
                .login("192.0.2.20", "admin", "wrong-password-value")
                .unwrap_err();
            let LoginError::Blocked(blocked) = error else {
                panic!("se esperaba un bloqueo de acceso");
            };
            assert_eq!(blocked.day_lock, expected_day_lock);
            assert_eq!(
                blocked.retry_after_seconds,
                if expected_day_lock {
                    LONG_LOCK_SECONDS
                } else {
                    SHORT_LOCK_SECONDS
                }
            );
            // Simula el paso del minuto sin hacer lenta la prueba.
            auth.attempts
                .lock()
                .unwrap()
                .get_mut("192.0.2.20")
                .unwrap()
                .locked_until = 0;
        }
        let reloaded = Auth::load(path.clone(), "ignored", "ignored-password").unwrap();
        let error = reloaded
            .login("192.0.2.20", "admin", "initial-password-123")
            .unwrap_err();
        let LoginError::Blocked(blocked) = error else {
            panic!("se esperaba un bloqueo persistido");
        };
        assert!(blocked.day_lock);
        assert!(blocked.retry_after_seconds > LONG_LOCK_SECONDS - 5);
        cleanup(&path);
    }
}
