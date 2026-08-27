use crate::model::{Deployment, Project};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

const SCHEMA_VERSION: u32 = 1;

pub struct Store {
    max_history: usize,
    connection: Mutex<Connection>,
}

impl Store {
    pub fn load(path: PathBuf, max_history: usize) -> Result<Self, String> {
        reject_legacy_file(&path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("no se pudo crear {}: {error}", parent.display()))?;
        }

        let connection = Connection::open(&path)
            .map_err(|error| format!("no se pudo abrir SQLite en {}: {error}", path.display()))?;
        protect_file(&path)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("no se pudo configurar SQLite: {error}"))?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
            .map_err(|error| format!("no se pudo configurar SQLite: {error}"))?;
        connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| format!("no se pudo activar WAL en SQLite: {error}"))?;

        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| format!("no se pudo leer la versión de SQLite: {error}"))?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "la base SQLite usa el esquema {version}, pero este binario solo entiende hasta {SCHEMA_VERSION}"
            ));
        }
        migrate(&connection, version)?;
        let integrity: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|error| format!("no se pudo verificar SQLite: {error}"))?;
        if integrity != "ok" {
            return Err(format!("la verificación de SQLite falló: {integrity}"));
        }
        protect_sqlite_files(&path)?;

        Ok(Self {
            max_history: max_history.max(10),
            connection: Mutex::new(connection),
        })
    }

    pub fn projects(&self) -> Result<Vec<Project>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {PROJECT_COLUMNS} FROM projects ORDER BY lower(name), slug"
            ))
            .map_err(sql_error)?;
        statement
            .query_map([], row_to_project)
            .map_err(sql_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)
    }

    pub fn project(&self, slug: &str) -> Result<Option<Project>, String> {
        let connection = self.lock()?;
        connection
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE slug = ?1"),
                [slug],
                row_to_project,
            )
            .optional()
            .map_err(sql_error)
    }

    pub fn add_project(&self, project: Project) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(sql_error)?;
        if exists(
            &transaction,
            "SELECT 1 FROM projects WHERE slug = ?1",
            &project.slug,
        )? {
            return Err("ya existe un proyecto con ese slug".to_owned());
        }
        let compose_file = project.compose_file.to_string_lossy().into_owned();
        if exists(
            &transaction,
            "SELECT 1 FROM projects WHERE compose_file = ?1",
            &compose_file,
        )? {
            return Err("ese archivo Compose ya está registrado".to_owned());
        }
        let env_file = project
            .env_file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        transaction
            .execute(
                "INSERT INTO projects (
                    slug, name, compose_file, env_file, image_env, branch, webhook_token,
                    current_image, created_at, kind, engine, repository, installation_id,
                    auto_deploy, source_revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    project.slug,
                    project.name,
                    compose_file,
                    env_file,
                    project.image_env,
                    project.branch,
                    project.webhook_token,
                    project.current_image,
                    to_i64(project.created_at, "created_at")?,
                    project.kind,
                    project.engine,
                    project.repository,
                    optional_u64_to_i64(project.installation_id, "installation_id")?,
                    project.auto_deploy,
                    project.source_revision,
                ],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)
    }

    pub fn remove_project(&self, slug: &str) -> Result<bool, String> {
        let connection = self.lock()?;
        connection
            .execute("DELETE FROM projects WHERE slug = ?1", [slug])
            .map(|changed| changed > 0)
            .map_err(sql_error)
    }

    pub fn update_current_image(&self, slug: &str, image: Option<String>) -> Result<(), String> {
        self.update_project(
            "UPDATE projects SET current_image = ?1 WHERE slug = ?2",
            image,
            slug,
        )
    }

    pub fn update_source_revision(&self, slug: &str, revision: String) -> Result<(), String> {
        self.update_project(
            "UPDATE projects SET source_revision = ?1 WHERE slug = ?2",
            Some(revision),
            slug,
        )
    }

    /// Habilita la indirección de imagen en proyectos creados antes de que existiera.
    pub fn set_image_env(&self, slug: &str, image_env: String) -> Result<(), String> {
        self.update_project(
            "UPDATE projects SET image_env = ?1 WHERE slug = ?2",
            Some(image_env),
            slug,
        )
    }

    pub fn append_deployment(&self, mut deployment: Deployment) -> Result<Deployment, String> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(sql_error)?;
        transaction
            .execute(
                "INSERT INTO deployments (
                    project, created_at, status, branch, commit_sha, image, previous_image,
                    message, duration_ms, trigger
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    deployment.project,
                    to_i64(deployment.created_at, "created_at")?,
                    deployment.status,
                    deployment.branch,
                    deployment.commit,
                    deployment.image,
                    deployment.previous_image,
                    deployment.message,
                    u128_to_i64(deployment.duration_ms, "duration_ms")?,
                    deployment.trigger,
                ],
            )
            .map_err(sql_error)?;
        deployment.id = transaction.last_insert_rowid().try_into().map_err(|_| {
            "SQLite devolvió un identificador de deployment fuera de rango".to_owned()
        })?;
        transaction
            .execute(
                "DELETE FROM deployments
                 WHERE id NOT IN (
                    SELECT id FROM deployments ORDER BY id DESC LIMIT ?1
                 )",
                [usize_to_i64(self.max_history, "TDM_MAX_HISTORY")?],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        Ok(deployment)
    }

    pub fn history(&self, project: Option<&str>, limit: usize) -> Result<Vec<Deployment>, String> {
        self.history_page(project, 0, limit).map(|(items, _)| items)
    }

    pub fn history_page(
        &self,
        project: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Deployment>, usize), String> {
        let connection = self.lock()?;
        let limit = limit.clamp(1, 100);
        let total: i64 = match project {
            Some(slug) => connection.query_row(
                "SELECT COUNT(*) FROM deployments WHERE project = ?1",
                [slug],
                |row| row.get(0),
            ),
            None => connection.query_row("SELECT COUNT(*) FROM deployments", [], |row| row.get(0)),
        }
        .map_err(sql_error)?;
        let items = match project {
            Some(slug) => query_deployments(
                &connection,
                "SELECT id, project, created_at, status, branch, commit_sha, image,
                        previous_image, message, duration_ms, trigger
                 FROM deployments WHERE project = ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
                params![
                    slug,
                    usize_to_i64(limit, "limit")?,
                    usize_to_i64(offset, "offset")?
                ],
            ),
            None => query_deployments(
                &connection,
                "SELECT id, project, created_at, status, branch, commit_sha, image,
                        previous_image, message, duration_ms, trigger
                 FROM deployments ORDER BY id DESC LIMIT ?1 OFFSET ?2",
                params![
                    usize_to_i64(limit, "limit")?,
                    usize_to_i64(offset, "offset")?
                ],
            ),
        }?;
        Ok((
            items,
            total
                .try_into()
                .map_err(|_| "conteo de deployments fuera de rango".to_owned())?,
        ))
    }

    pub fn last_deployment(&self, project: &str) -> Result<Option<Deployment>, String> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT id, project, created_at, status, branch, commit_sha, image,
                        previous_image, message, duration_ms, trigger
                 FROM deployments WHERE project = ?1 ORDER BY id DESC LIMIT 1",
                [project],
                row_to_deployment,
            )
            .optional()
            .map_err(sql_error)
    }

    pub fn rollback_target(&self, project: &str) -> Result<Option<String>, String> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT previous_image FROM deployments
                 WHERE project = ?1 AND status = 'success' AND previous_image IS NOT NULL
                   AND previous_image <> '' AND (image IS NULL OR previous_image <> image)
                 ORDER BY id DESC LIMIT 1",
                [project],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)
    }

    pub fn counts(&self) -> Result<(usize, usize), String> {
        let connection = self.lock()?;
        let projects: i64 = connection
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .map_err(sql_error)?;
        let deployments: i64 = connection
            .query_row("SELECT COUNT(*) FROM deployments", [], |row| row.get(0))
            .map_err(sql_error)?;
        Ok((
            projects
                .try_into()
                .map_err(|_| "conteo de proyectos fuera de rango".to_owned())?,
            deployments
                .try_into()
                .map_err(|_| "conteo de deployments fuera de rango".to_owned())?,
        ))
    }

    fn update_project(&self, sql: &str, value: Option<String>, slug: &str) -> Result<(), String> {
        let connection = self.lock()?;
        let changed = connection
            .execute(sql, params![value, slug])
            .map_err(sql_error)?;
        if changed == 0 {
            Err("proyecto no encontrado".to_owned())
        } else {
            Ok(())
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "la conexión SQLite quedó bloqueada".to_owned())
    }
}

const PROJECT_COLUMNS: &str = "slug, name, compose_file, env_file, image_env, branch,
    webhook_token, current_image, created_at, kind, engine, repository, installation_id,
    auto_deploy, source_revision";

fn migrate(connection: &Connection, version: u32) -> Result<(), String> {
    if version == 0 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE projects (
                    slug TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    compose_file TEXT NOT NULL UNIQUE,
                    env_file TEXT,
                    image_env TEXT,
                    branch TEXT,
                    webhook_token TEXT NOT NULL,
                    current_image TEXT,
                    created_at INTEGER NOT NULL CHECK(created_at >= 0),
                    kind TEXT NOT NULL,
                    engine TEXT,
                    repository TEXT,
                    installation_id INTEGER CHECK(installation_id IS NULL OR installation_id >= 0),
                    auto_deploy INTEGER NOT NULL DEFAULT 0 CHECK(auto_deploy IN (0, 1)),
                    source_revision TEXT
                 );
                 CREATE TABLE deployments (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project TEXT NOT NULL,
                    created_at INTEGER NOT NULL CHECK(created_at >= 0),
                    status TEXT NOT NULL,
                    branch TEXT,
                    commit_sha TEXT,
                    image TEXT,
                    previous_image TEXT,
                    message TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL CHECK(duration_ms >= 0),
                    trigger TEXT NOT NULL
                 );
                 CREATE INDEX deployments_project_id ON deployments(project, id DESC);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )
            .map_err(|error| format!("no se pudo crear el esquema SQLite: {error}"))?;
    }
    Ok(())
}

fn row_to_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    let compose_file: String = row.get(2)?;
    let env_file: Option<String> = row.get(3)?;
    Ok(Project {
        slug: row.get(0)?,
        name: row.get(1)?,
        compose_file: PathBuf::from(compose_file),
        env_file: env_file.map(PathBuf::from),
        image_env: row.get(4)?,
        branch: row.get(5)?,
        webhook_token: row.get(6)?,
        current_image: row.get(7)?,
        created_at: read_u64(row, 8)?,
        kind: row.get(9)?,
        engine: row.get(10)?,
        repository: row.get(11)?,
        installation_id: read_optional_u64(row, 12)?,
        auto_deploy: row.get(13)?,
        source_revision: row.get(14)?,
    })
}

fn row_to_deployment(row: &Row<'_>) -> rusqlite::Result<Deployment> {
    let duration_ms = read_u64(row, 9)?;
    Ok(Deployment {
        id: read_u64(row, 0)?,
        project: row.get(1)?,
        created_at: read_u64(row, 2)?,
        status: row.get(3)?,
        branch: row.get(4)?,
        commit: row.get(5)?,
        image: row.get(6)?,
        previous_image: row.get(7)?,
        message: row.get(8)?,
        duration_ms: u128::from(duration_ms),
        trigger: row.get(10)?,
    })
}

fn read_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn read_optional_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    let value: Option<i64> = row.get(index)?;
    value
        .map(|value| {
            value
                .try_into()
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

fn query_deployments<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<Vec<Deployment>, String> {
    let mut statement = connection.prepare(sql).map_err(sql_error)?;
    statement
        .query_map(parameters, row_to_deployment)
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)
}

fn exists(connection: &Connection, sql: &str, value: &str) -> Result<bool, String> {
    connection
        .query_row(sql, [value], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
        .map_err(sql_error)
}

fn reject_legacy_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut file = fs::File::open(path)
        .map_err(|error| format!("no se pudo leer {}: {error}", path.display()))?;
    let mut header = [0_u8; 4];
    let read = file
        .read(&mut header)
        .map_err(|error| format!("no se pudo leer {}: {error}", path.display()))?;
    if read == header.len() && matches!(&header, b"TDM1" | b"TDM2" | b"TDM3") {
        return Err(format!(
            "{} contiene el formato TDM antiguo; muévelo o configura TDM_SQLITE_PATH antes de iniciar",
            path.display()
        ));
    }
    Ok(())
}

fn protect_sqlite_files(path: &Path) -> Result<(), String> {
    protect_file(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = OsString::from(path.as_os_str());
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            protect_file(&sidecar)?;
        }
    }
    Ok(())
}

fn protect_file(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("no se pudo proteger {}: {error}", path.display()))
}

fn sql_error(error: rusqlite::Error) -> String {
    format!("error de SQLite: {error}")
}

fn to_i64(value: u64, field: &str) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| format!("{field} está fuera del rango de SQLite"))
}

fn optional_u64_to_i64(value: Option<u64>, field: &str) -> Result<Option<i64>, String> {
    value.map(|value| to_i64(value, field)).transpose()
}

fn usize_to_i64(value: usize, field: &str) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| format!("{field} está fuera del rango de SQLite"))
}

fn u128_to_i64(value: u128, field: &str) -> Result<i64, String> {
    value
        .try_into()
        .map_err(|_| format!("{field} está fuera del rango de SQLite"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KIND_REPOSITORY, Project};
    use crate::util::unique_suffix;

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!("tdm-store-{}.sqlite3", unique_suffix()))
    }

    fn cleanup(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
            let _ = fs::remove_file(candidate);
        }
    }

    fn project(slug: &str) -> Project {
        Project {
            slug: slug.to_owned(),
            name: format!("Project {slug}"),
            compose_file: PathBuf::from(format!("/opt/tinkiva/apps/{slug}/compose.yaml")),
            env_file: Some(PathBuf::from(format!("/opt/tinkiva/apps/{slug}/.env"))),
            image_env: Some("APP_IMAGE".to_owned()),
            branch: Some("main".to_owned()),
            webhook_token: "secret-token".to_owned(),
            current_image: Some("ghcr.io/demo/api:one".to_owned()),
            created_at: 10,
            kind: KIND_REPOSITORY.to_owned(),
            engine: None,
            repository: Some("isaul19/demo".to_owned()),
            installation_id: Some(42),
            auto_deploy: true,
            source_revision: Some("sha256:one".to_owned()),
        }
    }

    fn deployment(project: &str, number: u64) -> Deployment {
        Deployment {
            id: 0,
            project: project.to_owned(),
            created_at: number,
            status: "success".to_owned(),
            branch: Some("main".to_owned()),
            commit: Some(format!("commit-{number}")),
            image: Some(format!("ghcr.io/demo/api:{number}")),
            previous_image: (number > 0).then(|| format!("ghcr.io/demo/api:{}", number - 1)),
            message: "deployed".to_owned(),
            duration_ms: u128::from(number) * 10,
            trigger: "manual".to_owned(),
        }
    }

    #[test]
    fn sqlite_round_trip_preserves_projects_and_deployments() {
        let path = path();
        {
            let store = Store::load(path.clone(), 20).unwrap();
            let expected = project("demo");
            store.add_project(expected.clone()).unwrap();
            let saved = store.append_deployment(deployment("demo", 1)).unwrap();
            assert_eq!(saved.id, 1);
            assert_eq!(store.project("demo").unwrap(), Some(expected));
            assert_eq!(store.history(None, 10).unwrap(), vec![saved]);
        }
        assert_eq!(&fs::read(&path).unwrap()[..16], b"SQLite format 3\0");
        let reopened = Store::load(path.clone(), 20).unwrap();
        assert_eq!(reopened.counts().unwrap(), (1, 1));
        assert_eq!(
            reopened.project("demo").unwrap().unwrap().installation_id,
            Some(42)
        );
        cleanup(&path);
    }

    #[test]
    fn history_retention_and_filters_are_enforced_in_sql() {
        let path = path();
        let store = Store::load(path.clone(), 10).unwrap();
        for number in 1..=12 {
            store
                .append_deployment(deployment(
                    if number % 2 == 0 { "api" } else { "web" },
                    number,
                ))
                .unwrap();
        }
        let (page, total) = store.history_page(Some("api"), 1, 2).unwrap();
        assert_eq!(total, 5);
        assert_eq!(
            page.iter().map(|item| item.created_at).collect::<Vec<_>>(),
            vec![10, 8]
        );
        assert_eq!(store.counts().unwrap().1, 10);
        cleanup(&path);
    }

    #[test]
    fn project_updates_and_duplicate_checks_match_the_old_contract() {
        let path = path();
        let store = Store::load(path.clone(), 10).unwrap();
        store.add_project(project("demo")).unwrap();
        assert_eq!(
            store.add_project(project("demo")).unwrap_err(),
            "ya existe un proyecto con ese slug"
        );
        store
            .update_current_image("demo", Some("image:two".to_owned()))
            .unwrap();
        store
            .update_source_revision("demo", "sha256:two".to_owned())
            .unwrap();
        store.set_image_env("demo", "IMAGE".to_owned()).unwrap();
        let updated = store.project("demo").unwrap().unwrap();
        assert_eq!(updated.current_image.as_deref(), Some("image:two"));
        assert_eq!(updated.source_revision.as_deref(), Some("sha256:two"));
        assert_eq!(updated.image_env.as_deref(), Some("IMAGE"));
        assert!(store.remove_project("demo").unwrap());
        assert!(!store.remove_project("demo").unwrap());
        cleanup(&path);
    }

    #[test]
    fn legacy_text_is_never_overwritten_as_sqlite() {
        let path = path();
        fs::write(&path, "TDM3\t1\n").unwrap();
        let error = Store::load(path.clone(), 10).err().unwrap();
        assert!(error.contains("formato TDM antiguo"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "TDM3\t1\n");
        cleanup(&path);
    }
}
