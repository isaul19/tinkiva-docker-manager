use crate::model::{Project, KIND_REPOSITORY};
use crate::proc::{self, CommandResult, FileCommandResult};
use crate::util::{ json_string, truncate_text, valid_container_ref, valid_schema_name };
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{ Path, PathBuf };
use std::time::Duration;

const FIELD_SEPARATOR: char = '\u{1f}';

#[derive(Clone)]
pub struct DockerClient {
    binary: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DockerInfo {
    pub available: bool,
    pub server_version: Option<String>,
    pub compose_version: Option<String>,
    pub error: Option<String>,
    pub compose_error: Option<String>,
}

impl DockerInfo {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"available\":{},",
                "\"server_version\":{},",
                "\"compose_version\":{},",
                "\"error\":{},",
                "\"compose_error\":{}",
                "}}"
            ),
            self.available,
            self.server_version.as_deref().map_or_else(|| "null".to_owned(), json_string),
            self.compose_version.as_deref().map_or_else(|| "null".to_owned(), json_string),
            self.error.as_deref().map_or_else(|| "null".to_owned(), json_string),
            self.compose_error.as_deref().map_or_else(|| "null".to_owned(), json_string)
        )
    }
}

#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub state: String,
    pub ports: String,
    pub created_at: String,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub memory_percent: Option<String>,
    pub network_io: Option<String>,
    pub block_io: Option<String>,
    pub pids: Option<String>,
}

/// Imagen local del host. `containers` lista los contenedores que la usan; si
/// está vacío, la imagen puede borrarse sin romper nada.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub id: String,
    /// Lo que hay que pasarle a `docker rmi`: `repo:tag`, o el id si no tiene
    /// etiqueta.
    pub reference: String,
    pub repository: String,
    pub tag: String,
    /// Cifra redondeada de `docker images`, por si el inspect no responde.
    pub size: String,
    pub size_bytes: u64,
    /// Antigüedad tal y como la da Docker («3 weeks ago»); se traduce en la
    /// interfaz, que es donde vive el idioma del panel.
    pub created_since: String,
    pub containers: Vec<String>,
    /// Slug del recurso que aún puede volver a esta imagen con «Rollback». Lo
    /// rellena la capa de aplicación, que es la que conoce el historial.
    pub protected_by: Option<String>,
}

impl ImageInfo {
    pub fn to_json(&self) -> String {
        let containers = self
            .containers
            .iter()
            .map(|name| json_string(name))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{",
                "\"id\":{},",
                "\"reference\":{},",
                "\"repository\":{},",
                "\"tag\":{},",
                "\"size\":{},",
                "\"size_bytes\":{},",
                "\"created_since\":{},",
                "\"in_use\":{},",
                "\"protected_by\":{},",
                "\"containers\":[{}]",
                "}}"
            ),
            json_string(&self.id),
            json_string(&self.reference),
            json_string(&self.repository),
            json_string(&self.tag),
            json_string(&self.size),
            self.size_bytes,
            json_string(&self.created_since),
            !self.containers.is_empty(),
            self.protected_by
                .as_deref()
                .map_or_else(|| "null".to_owned(), json_string),
            containers
        )
    }
}

/// `sha256:abc…` → `abc123456789`, como muestra `docker images`.
fn short_image_id(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

/// Información puntual para decidir la experiencia de consola. Se calcula al
/// abrirla; no se guarda ni mantiene procesos en segundo plano.
#[derive(Debug, Clone)]
pub struct ConsoleInfo {
    pub database: Option<&'static str>,
    pub user: String,
}

impl ContainerInfo {
    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"id\":{},",
                "\"name\":{},",
                "\"image\":{},",
                "\"status\":{},",
                "\"state\":{},",
                "\"ports\":{},",
                "\"created_at\":{},",
                "\"cpu\":{},",
                "\"memory\":{},",
                "\"memory_percent\":{},",
                "\"network_io\":{},",
                "\"block_io\":{},",
                "\"pids\":{}",
                "}}"
            ),
            json_string(&self.id),
            json_string(&self.name),
            json_string(&self.image),
            json_string(&self.status),
            json_string(&self.state),
            json_string(&self.ports),
            json_string(&self.created_at),
            optional_json(&self.cpu),
            optional_json(&self.memory),
            optional_json(&self.memory_percent),
            optional_json(&self.network_io),
            optional_json(&self.block_io),
            optional_json(&self.pids)
        )
    }
}

fn optional_json(value: &Option<String>) -> String {
    value.as_deref().map_or_else(|| "null".to_owned(), json_string)
}

impl DockerClient {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }

    pub fn info(&self) -> DockerInfo {
        let server = self.run(
            ["version", "--format", "{{.Server.Version}}"],
            None,
            Duration::from_secs(10)
        );

        let Ok(server) = server else {
            return DockerInfo {
                available: false,
                server_version: None,
                compose_version: None,
                error: Some("no se pudo ejecutar Docker".to_owned()),
                compose_error: None,
            };
        };

        if !server.success {
            return DockerInfo {
                available: false,
                server_version: None,
                compose_version: None,
                error: Some(server.summary()),
                compose_error: None,
            };
        }

        let compose = self.run(["compose", "version"], None, Duration::from_secs(10));
        let (compose_version, compose_error) = match compose {
            Ok(result) if result.success => (Some(result.stdout.trim().to_owned()), None),
            Ok(result) => (None, Some(compose_error_message(&result.summary()))),
            Err(error) => (None, Some(error)),
        };

        DockerInfo {
            available: true,
            server_version: Some(server.stdout.trim().to_owned()),
            compose_version,
            error: None,
            compose_error,
        }
    }

    pub fn containers(&self) -> Result<Vec<ContainerInfo>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.ID}}}}{separator}{{{{.Names}}}}{separator}{{{{.Image}}}}{separator}{{{{.Status}}}}{separator}{{{{.State}}}}{separator}{{{{.Ports}}}}{separator}{{{{.CreatedAt}}}}"
        );
        let result = self.run(
            ["ps", "-a", "--no-trunc", "--format", &format],
            None,
            Duration::from_secs(15)
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let mut containers = Vec::new();
        for line in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<&str> = line.split(separator).collect();
            if fields.len() != 7 {
                continue;
            }
            containers.push(ContainerInfo {
                id: fields[0].to_owned(),
                name: fields[1].to_owned(),
                image: fields[2].to_owned(),
                status: fields[3].to_owned(),
                state: fields[4].to_owned(),
                ports: fields[5].to_owned(),
                created_at: fields[6].to_owned(),
                cpu: None,
                memory: None,
                memory_percent: None,
                network_io: None,
                block_io: None,
                pids: None,
            });
        }

        let stats = self.stats().unwrap_or_default();
        for container in &mut containers {
            if let Some(stat) = stats.get(&container.name) {
                container.cpu.clone_from(&stat.cpu);
                container.memory.clone_from(&stat.memory);
                container.memory_percent.clone_from(&stat.memory_percent);
                container.network_io.clone_from(&stat.network_io);
                container.block_io.clone_from(&stat.block_io);
                container.pids.clone_from(&stat.pids);
            }
        }

        containers.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(containers)
    }

    /// Imágenes locales con su peso y qué contenedores las están usando. El uso
    /// se resuelve comparando el id completo que devuelve `inspect`, no la
    /// referencia con la que se arrancó el contenedor: así una imagen sigue
    /// contando como en uso aunque le hayan movido la etiqueta.
    pub fn images(&self) -> Result<Vec<ImageInfo>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.ID}}}}{separator}{{{{.Repository}}}}{separator}{{{{.Tag}}}}{separator}{{{{.Size}}}}{separator}{{{{.CreatedSince}}}}"
        );
        let result = self.run(
            ["images", "--no-trunc", "--format", &format],
            None,
            Duration::from_secs(15),
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let users = self.image_users().unwrap_or_default();
        let sizes = self.image_sizes(&result.stdout, separator).unwrap_or_default();
        let mut images = Vec::new();
        for line in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<&str> = line.split(separator).collect();
            if fields.len() != 5 {
                continue;
            }
            let id = fields[0].to_owned();
            let repository = fields[1].to_owned();
            let tag = fields[2].to_owned();
            // Una imagen sin etiqueta solo puede borrarse por id; una etiquetada
            // se borra por `repo:tag` para no arrastrar sus otras etiquetas.
            let reference = if repository == "<none>" || tag == "<none>" {
                short_image_id(&id)
            } else {
                format!("{repository}:{tag}")
            };
            let containers = users
                .get(&id)
                .cloned()
                .unwrap_or_default();
            images.push(ImageInfo {
                size: fields[3].to_owned(),
                size_bytes: sizes.get(&id).copied().unwrap_or(0),
                created_since: fields[4].to_owned(),
                id: short_image_id(&id),
                reference,
                repository,
                tag,
                containers,
                protected_by: None,
            });
        }
        // Las más pesadas primero: la lista existe sobre todo para recuperar disco.
        images.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.reference.cmp(&right.reference))
        });
        Ok(images)
    }

    /// Tamaño exacto en bytes de cada imagen. `docker images` solo da la cifra
    /// redondeada («142MB»), que no sirve ni para sumar ni para ordenar.
    fn image_sizes(&self, listing: &str, separator: char) -> Result<HashMap<String, u64>, String> {
        let mut ids: Vec<&str> = listing
            .lines()
            .filter_map(|line| line.split(separator).next())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        let format = format!("{{{{.Id}}}}{separator}{{{{.Size}}}}");
        let mut arguments = vec!["image", "inspect", "--format", &format];
        arguments.extend(ids);
        let inspect = self.run(arguments, None, Duration::from_secs(20))?;
        if !inspect.success {
            return Err(inspect.summary());
        }

        let mut sizes = HashMap::new();
        for line in inspect.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let mut parts = line.split(separator);
            let (Some(id), Some(size)) = (parts.next(), parts.next()) else {
                continue;
            };
            if let Ok(bytes) = size.trim().parse::<u64>() {
                sizes.insert(id.trim().to_owned(), bytes);
            }
        }
        Ok(sizes)
    }

    /// Mapa `id completo de imagen → contenedores que la usan`, incluidos los
    /// detenidos: borrar la imagen de un contenedor parado también lo rompería.
    fn image_users(&self) -> Result<HashMap<String, Vec<String>>, String> {
        let list = self.run(["ps", "-aq", "--no-trunc"], None, Duration::from_secs(15))?;
        if !list.success {
            return Err(list.summary());
        }
        let ids: Vec<&str> = list
            .stdout
            .lines()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .collect();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        let separator = FIELD_SEPARATOR;
        let format = format!("{{{{.Image}}}}{separator}{{{{.Name}}}}");
        let mut arguments = vec!["inspect", "--format", &format];
        arguments.extend(ids);
        let inspect = self.run(arguments, None, Duration::from_secs(20))?;
        if !inspect.success {
            return Err(inspect.summary());
        }

        let mut users: HashMap<String, Vec<String>> = HashMap::new();
        for line in inspect.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let mut parts = line.split(separator);
            let (Some(image), Some(name)) = (parts.next(), parts.next()) else {
                continue;
            };
            users
                .entry(image.trim().to_owned())
                .or_default()
                .push(name.trim().trim_start_matches('/').to_owned());
        }
        Ok(users)
    }

    /// Etiquetas de un repositorio local, de la más reciente a la más antigua.
    /// Es el orden natural de `docker images`, que es justo el que necesita la
    /// retención de builds.
    pub fn repository_images(&self, repository: &str) -> Result<Vec<String>, String> {
        if !valid_container_ref(repository) {
            return Err("repositorio inválido".to_owned());
        }
        let result = self.run(
            ["images", repository, "--format", "{{.Repository}}:{{.Tag}}"],
            None,
            Duration::from_secs(15),
        )?;
        if !result.success {
            return Err(result.summary());
        }
        Ok(result
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.ends_with(":<none>"))
            .map(str::to_owned)
            .collect())
    }

    /// Autentica Docker contra un registro. La contraseña viaja por la entrada
    /// estándar (`--password-stdin`), nunca por `argv`: de otro modo cualquier
    /// usuario del servidor la vería con un simple `ps`.
    pub fn login(&self, registry: &str, username: &str, password: &str) -> Result<(), String> {
        if !valid_container_ref(registry) || !valid_container_ref(username) {
            return Err("registro o usuario inválidos".to_owned());
        }
        let result = proc::run_with_input(
            &self.binary,
            ["login", "--username", username, "--password-stdin", registry],
            None,
            &[("DOCKER_CLI_HINTS", "false")],
            Some(password),
            Duration::from_secs(30),
        )?;
        if result.success {
            Ok(())
        } else {
            Err(result.summary())
        }
    }

    pub fn logout(&self, registry: &str) -> Result<(), String> {
        if !valid_container_ref(registry) {
            return Err("registro inválido".to_owned());
        }
        self.run(["logout", registry], None, Duration::from_secs(20))
            .map(|_| ())
    }

    /// Borra una imagen local. Nunca usa `--force`: si Docker considera que algo
    /// depende de ella, preferimos fallar y contarlo.
    pub fn remove_image(&self, reference: &str) -> Result<CommandResult, String> {
        if !valid_container_ref(reference) {
            return Err("referencia de imagen inválida".to_owned());
        }
        self.run(["rmi", reference], None, Duration::from_secs(60))
    }

    /// Estado agregado de los servicios que pertenecen a un proyecto Compose.
    /// Se consulta por archivo para no depender del nombre que Compose dedujo
    /// para el stack (que no siempre coincide con el slug del panel).
    pub fn project_status(&self, project: &Project) -> Result<&'static str, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.State}}}}{separator}{{{{.Health}}}}{separator}{{{{.ExitCode}}}}"
        );
        let result = self.run(
            ["compose", "-f", &compose, "ps", "-a", "--format", &format],
            project.compose_file.parent(),
            Duration::from_secs(15),
        )?;
        if !result.success {
            return Err(result.summary());
        }
        Ok(summarize_project_status(&result.stdout))
    }

    pub fn container_action(&self, container: &str, action: &str) -> Result<CommandResult, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        let args: Vec<&str> = match action {
            "start" => vec!["start", container],
            "stop" => vec!["stop", "--time", "20", container],
            "restart" => vec!["restart", "--time", "20", container],
            _ => {
                return Err("acción de contenedor no permitida".to_owned());
            }
        };
        self.run(args, None, Duration::from_secs(45))
    }

    pub fn container_logs(&self, container: &str, tail: usize) -> Result<String, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }

        let tail = tail.clamp(10, 2000).to_string();

        let result = self.run(
            ["logs", "--timestamps", "--tail", &tail, container],
            None,
            Duration::from_secs(15)
        )?;

        if !result.success && result.stdout.trim().is_empty() && result.stderr.trim().is_empty() {
            return Err(result.summary());
        }

        let mut output = result.stdout;

        if !result.stderr.trim().is_empty() {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }

            output.push_str(&result.stderr);
        }

        Ok(truncate_text(&output, 2 * 1024 * 1024))
    }

    /// Ejecuta un comando de consola dentro del contenedor, nunca en el host.
    pub fn container_exec(&self, container: &str, command: &str) -> Result<CommandResult, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        if command.trim().is_empty() || command.len() > 4096 || command.contains('\0') {
            return Err("comando inválido o demasiado largo".to_owned());
        }
        // El comando llega como argumento posicional para que sus caracteres no
        // se interpolen en el script de shell que ejecuta Docker.
        self.run(
            ["exec", container, "sh", "-lc", "sh -lc \"$1\" 2>&1 | head -c 1048576", "tdm", command],
            None,
            Duration::from_secs(60),
        )
    }

    /// Inspecciona señales estáticas y prueba clientes en el contenedor activo.
    /// Son comandos cortos bajo demanda; no se crea ningún servicio residente.
    pub fn container_console_info(&self, container: &str) -> Result<ConsoleInfo, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.Config.Image}}}}{separator}{{{{.Config.User}}}}{separator}{{{{.Path}}}} {{{{join .Args \" \"}}}}{separator}{{{{json .NetworkSettings.Ports}}}}"
        );
        let inspect = self.run(
            ["inspect", "--format", &format, container],
            None,
            Duration::from_secs(10),
        )?;
        if !inspect.success {
            return Err(inspect.summary());
        }
        let fields = inspect.stdout.trim().split(separator).collect::<Vec<_>>();
        let image = fields.first().copied().unwrap_or_default().to_ascii_lowercase();
        let configured_user = fields.get(1).copied().unwrap_or_default();
        let command = fields.get(2).copied().unwrap_or_default().to_ascii_lowercase();
        let ports = fields.get(3).copied().unwrap_or_default().to_ascii_lowercase();

        let probe = self.run(
            [
                "exec",
                container,
                "sh",
                "-lc",
                "printf 'USER='; id -un 2>/dev/null || true; for c in psql mysql mariadb mongosh redis-cli; do command -v \"$c\" >/dev/null 2>&1 && printf '\\n%s=1' \"$c\"; done; printf '\\n'",
            ],
            None,
            Duration::from_secs(10),
        )?;
        let probe_text = probe.stdout.to_ascii_lowercase();
        let user = probe
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("USER="))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(configured_user)
            .trim();

        Ok(ConsoleInfo {
            database: detect_database(&image, &ports, &command, &probe_text),
            user: if user.is_empty() { "root".to_owned() } else { user.to_owned() },
        })
    }

    /// Ejecuta una consulta usando el cliente instalado dentro de la base de
    /// datos. La consulta se pasa como `$1`, sin interpolarla en el script.
    pub fn database_query(
        &self,
        container: &str,
        database: &str,
        query: &str,
    ) -> Result<CommandResult, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        if query.trim().is_empty() || query.len() > 4096 || query.contains('\0') {
            return Err("consulta inválida o demasiado larga".to_owned());
        }
        let script = match database {
            "postgres" => "psql -v ON_ERROR_STOP=1 -U \"${POSTGRES_USER:-postgres}\" -d \"${POSTGRES_DB:-postgres}\" -c \"$1\" 2>&1 | head -c 1048576",
            "mysql" | "mariadb" => "client=$(command -v mariadb || command -v mysql) || exit 127; user=\"${MYSQL_USER:-${MARIADB_USER:-root}}\"; database=\"${MYSQL_DATABASE:-${MARIADB_DATABASE:-}}\"; password=\"${MYSQL_PASSWORD:-${MARIADB_PASSWORD:-${MYSQL_ROOT_PASSWORD:-${MARIADB_ROOT_PASSWORD:-}}}}\"; [ -z \"$password\" ] || export MYSQL_PWD=\"$password\"; if [ -n \"$database\" ]; then \"$client\" -u \"$user\" \"$database\" -e \"$1\"; else \"$client\" -u \"$user\" -e \"$1\"; fi 2>&1 | head -c 1048576",
            "mongodb" => "if [ -n \"${MONGO_INITDB_ROOT_USERNAME:-}\" ]; then mongosh --quiet -u \"$MONGO_INITDB_ROOT_USERNAME\" -p \"${MONGO_INITDB_ROOT_PASSWORD:-}\" --authenticationDatabase admin --eval \"$1\"; else mongosh --quiet --eval \"$1\"; fi 2>&1 | head -c 1048576",
            "redis" => "if [ -n \"${REDIS_PASSWORD:-}\" ]; then printf '%s\\n' \"$1\" | redis-cli --no-auth-warning -a \"$REDIS_PASSWORD\" --raw; else printf '%s\\n' \"$1\" | redis-cli --raw; fi 2>&1 | head -c 1048576",
            _ => return Err("motor de base de datos no soportado".to_owned()),
        };
        self.run(
            ["exec", container, "sh", "-lc", script, "tdm-query", query],
            None,
            Duration::from_secs(60),
        )
    }

    /// Lista las bases de datos exportables del contenedor. En MySQL y MariaDB
    /// «esquema» y «base de datos» son lo mismo; en PostgreSQL se listan las
    /// bases conectables para que el volcado sea uno por base.
    pub fn database_schemas(&self, container: &str, engine: &str) -> Result<Vec<String>, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        let script = match engine {
            "postgres" => "export PGPASSWORD=\"${POSTGRES_PASSWORD:-}\"; psql -Atq -U \"${POSTGRES_USER:-postgres}\" -d \"${POSTGRES_DB:-postgres}\" -c \"SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY 1\"",
            "mysql" | "mariadb" => "client=$(command -v mariadb || command -v mysql) || exit 127; password=\"${MYSQL_PASSWORD:-${MARIADB_PASSWORD:-${MYSQL_ROOT_PASSWORD:-${MARIADB_ROOT_PASSWORD:-}}}}\"; [ -z \"$password\" ] || export MYSQL_PWD=\"$password\"; \"$client\" -u \"${MYSQL_USER:-${MARIADB_USER:-root}}\" -N -B -e \"SHOW DATABASES\"",
            _ => return Err("el motor no admite exportación SQL".to_owned()),
        };

        let result = self.run(
            ["exec", container, "sh", "-lc", script],
            None,
            Duration::from_secs(30),
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let schemas: Vec<String> = result
            .stdout
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty() && !is_system_schema(engine, name))
            .filter(|name| valid_schema_name(name))
            .map(str::to_owned)
            .collect();
        if schemas.is_empty() {
            return Err("no se encontraron bases de datos exportables".to_owned());
        }
        Ok(schemas)
    }

    /// Vuelca las bases indicadas a `destination` usando el cliente de volcado
    /// del propio contenedor. La salida nunca pasa por la memoria del panel: el
    /// subproceso escribe directamente en el archivo.
    pub fn database_dump(
        &self,
        container: &str,
        engine: &str,
        mode: &str,
        schemas: &[String],
        destination: &Path,
    ) -> Result<FileCommandResult, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        if schemas.is_empty() {
            return Err("selecciona al menos una base de datos".to_owned());
        }
        if schemas.len() > 64 {
            return Err("demasiadas bases seleccionadas".to_owned());
        }
        if !schemas.iter().all(|schema| valid_schema_name(schema)) {
            return Err("nombre de base de datos inválido".to_owned());
        }
        let script = dump_script(engine, mode).ok_or_else(|| {
            "combinación de motor y modo de exportación no soportada".to_owned()
        })?;

        // Los nombres viajan como argumentos posicionales (`$@`): el script no
        // los interpola, así que no hay forma de inyectar shell desde el panel.
        let mut arguments: Vec<&str> =
            vec!["exec", container, "sh", "-lc", script.as_str(), "tdm-dump"];
        arguments.extend(schemas.iter().map(String::as_str));

        proc::run_to_file(
            &self.binary,
            arguments,
            destination,
            &[("DOCKER_CLI_HINTS", "false")],
            Duration::from_secs(1800),
        )
    }

    /// Restaura un `.sql` dentro del contenedor.
    ///
    /// El archivo entra por la entrada estándar de `docker exec -i`: no se copia
    /// dentro del contenedor, no pasa por `argv` y el panel nunca lo carga en
    /// memoria. El nombre de la base viaja como `$1`, igual que en el volcado.
    pub fn database_restore(
        &self,
        container: &str,
        engine: &str,
        schema: &str,
        source: &Path,
    ) -> Result<CommandResult, String> {
        if !valid_container_ref(container) {
            return Err("identificador de contenedor inválido".to_owned());
        }
        if !valid_schema_name(schema) {
            return Err("nombre de base de datos inválido".to_owned());
        }
        let script = restore_script(engine)
            .ok_or_else(|| "este motor no restaura volcados SQL".to_owned())?;

        proc::run_with_file_input(
            &self.binary,
            ["exec", "-i", container, "sh", "-lc", script, "tdm-restore", schema],
            None,
            &[("DOCKER_CLI_HINTS", "false")],
            source,
            Duration::from_secs(1800),
        )
    }

    pub fn compose_logs(&self, project: &Project, tail: usize) -> Result<String, String> {
        let tail = tail.clamp(10, 2000).to_string();
        let compose = project.compose_file.to_string_lossy().into_owned();

        let result = self.run(
            ["compose", "-f", &compose, "logs", "--no-color", "--timestamps", "--tail", &tail],
            project.compose_file.parent(),
            Duration::from_secs(20)
        )?;

        if !result.success && result.stdout.trim().is_empty() && result.stderr.trim().is_empty() {
            return Err(result.summary());
        }

        let mut output = result.stdout;

        if !result.stderr.trim().is_empty() {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }

            output.push_str(&result.stderr);
        }

        Ok(truncate_text(&output, 2 * 1024 * 1024))
    }

    pub fn validate_compose(&self, compose_file: &Path) -> Result<(), String> {
        let compose = compose_file.to_string_lossy().into_owned();
        let result = self.run(
            ["compose", "-f", &compose, "config", "--quiet"],
            compose_file.parent(),
            Duration::from_secs(20)
        )?;
        if result.success {
            Ok(())
        } else {
            Err(result.summary())
        }
    }

    pub fn deploy(&self, project: &Project) -> Result<CommandResult, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        let working_directory = project.compose_file.parent();

        // Los proyectos de GitHub construyen la imagen a partir del clon local:
        // no hay nada que descargar de un registro y `pull` fallaría.
        if project.kind == KIND_REPOSITORY {
            return self.run(
                ["compose", "-f", &compose, "up", "-d", "--build", "--remove-orphans"],
                working_directory,
                Duration::from_secs(1800)
            );
        }

        let pull = self.run(
            ["compose", "-f", &compose, "pull", "--quiet"],
            working_directory,
            Duration::from_secs(300)
        )?;
        if !pull.success {
            return Ok(pull);
        }

        let mut up = self.run(
            ["compose", "-f", &compose, "up", "-d", "--remove-orphans"],
            working_directory,
            Duration::from_secs(300)
        )?;
        if !pull.stdout.trim().is_empty() {
            up.stdout = format!("{}\n{}", pull.stdout.trim(), up.stdout.trim());
        }
        if !pull.stderr.trim().is_empty() {
            up.stderr = format!("{}\n{}", pull.stderr.trim(), up.stderr.trim());
        }
        up.duration_ms = up.duration_ms.saturating_add(pull.duration_ms);
        Ok(up)
    }

    /// Digest inmutable del registry (o ID local como fallback), si la imagen existe.
    pub fn image_revision(&self, image: &str) -> Result<Option<String>, String> {
        let result = self.run(
            [
                "image",
                "inspect",
                "--format",
                "{{range .RepoDigests}}{{println .}}{{end}}",
                image,
            ],
            None,
            Duration::from_secs(20),
        )?;
        if !result.success {
            return Ok(None);
        }
        if let Some(digest) = result
            .stdout
            .split_whitespace()
            .find_map(|entry| entry.split_once('@').map(|(_, digest)| digest.to_owned()))
        {
            return Ok(Some(digest));
        }
        let fallback = self.run(
            ["image", "inspect", "--format", "{{.Id}}", image],
            None,
            Duration::from_secs(20),
        )?;
        Ok(fallback.success.then(|| fallback.stdout.trim().to_owned()).filter(|id| !id.is_empty()))
    }

    /// Actualiza la caché local desde el registry. Docker reutiliza capas y
    /// credenciales existentes, sin mantener otro cliente residente en memoria.
    pub fn pull_image(&self, image: &str) -> Result<CommandResult, String> {
        self.run(["pull", "--quiet", image], None, Duration::from_secs(600))
    }

    /// Aplica `compose up -d` sin `--build`: para una imagen que ya está en el
    /// Docker local (pull del watcher, rollback o restauración automática).
    pub fn deploy_pulled(&self, project: &Project) -> Result<CommandResult, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        self.run(
            ["compose", "-f", &compose, "up", "-d", "--remove-orphans"],
            project.compose_file.parent(),
            Duration::from_secs(300),
        )
    }

    /// Detiene y elimina el stack. `remove_volumes` borra también los datos, por
    /// lo que el panel solo lo pide con confirmación explícita del usuario.
    pub fn compose_down(
        &self,
        project: &Project,
        remove_volumes: bool
    ) -> Result<CommandResult, String> {
        let compose = project.compose_file.to_string_lossy().into_owned();
        let mut arguments = vec![
            "compose".to_owned(),
            "-f".to_owned(),
            compose,
            "down".to_owned(),
            "--remove-orphans".to_owned()
        ];
        if remove_volumes {
            arguments.push("--volumes".to_owned());
        }
        self.run(arguments, project.compose_file.parent(), Duration::from_secs(180))
    }

    pub fn ensure_network(&self, network: &str) -> Result<(), String> {
        if !valid_container_ref(network) {
            return Err("nombre de red inválido".to_owned());
        }
        let inspect = self.run(["network", "inspect", network], None, Duration::from_secs(10))?;
        if inspect.success {
            return Ok(());
        }
        let create = self.run(["network", "create", network], None, Duration::from_secs(20))?;
        if create.success {
            Ok(())
        } else {
            Err(create.summary())
        }
    }

    fn stats(&self) -> Result<HashMap<String, ContainerStats>, String> {
        let separator = FIELD_SEPARATOR;
        let format = format!(
            "{{{{.Name}}}}{separator}{{{{.CPUPerc}}}}{separator}{{{{.MemUsage}}}}{separator}{{{{.MemPerc}}}}{separator}{{{{.NetIO}}}}{separator}{{{{.BlockIO}}}}{separator}{{{{.PIDs}}}}"
        );
        let result = self.run(
            ["stats", "--no-stream", "--all", "--no-trunc", "--format", &format],
            None,
            Duration::from_secs(20)
        )?;
        if !result.success {
            return Err(result.summary());
        }

        let mut stats = HashMap::new();
        for line in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<&str> = line.split(separator).collect();
            if fields.len() != 7 {
                continue;
            }
            stats.insert(fields[0].to_owned(), ContainerStats {
                cpu: non_empty(fields[1]),
                memory: non_empty(fields[2]),
                memory_percent: non_empty(fields[3]),
                network_io: non_empty(fields[4]),
                block_io: non_empty(fields[5]),
                pids: non_empty(fields[6]),
            });
        }
        Ok(stats)
    }

    fn run<I, S>(
        &self,
        arguments: I,
        working_directory: Option<&Path>,
        timeout: Duration
    )
        -> Result<CommandResult, String>
        where I: IntoIterator<Item = S>, S: AsRef<OsStr>
    {
        proc::run(
            &self.binary,
            arguments,
            working_directory,
            &[("DOCKER_CLI_HINTS", "false"), ("COMPOSE_ANSI", "never")],
            timeout
        )
    }
}

fn summarize_project_status(output: &str) -> &'static str {
    let services: Vec<Vec<&str>> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split(FIELD_SEPARATOR).collect())
        .collect();

    if services.is_empty() {
        return "stopped";
    }

    let all_running = services.iter().all(|fields| {
        fields.first().is_some_and(|state| state.eq_ignore_ascii_case("running"))
            && !fields
                .get(1)
                .is_some_and(|health| health.eq_ignore_ascii_case("unhealthy"))
    });
    if all_running {
        return "running";
    }

    let has_error = services.iter().any(|fields| {
        let state = fields.first().copied().unwrap_or_default();
        let health = fields.get(1).copied().unwrap_or_default();
        let exit_code = fields.get(2).copied().unwrap_or_default();
        health.eq_ignore_ascii_case("unhealthy")
            || matches!(state.to_ascii_lowercase().as_str(), "dead" | "restarting")
            || (state.eq_ignore_ascii_case("exited") && exit_code != "0")
    });
    let partially_running = services.iter().any(|fields| {
        fields
            .first()
            .is_some_and(|state| state.eq_ignore_ascii_case("running"))
    });
    if has_error || partially_running {
        "error"
    } else {
        "stopped"
    }
}

/// Motores cuyo volcado es un `.sql` restaurable. MongoDB y Redis se detectan
/// para la consola, pero sus copias son binarias y no encajan en este flujo.
pub fn exportable_engine(engine: &str) -> bool {
    matches!(engine, "postgres" | "mysql" | "mariadb")
}

fn is_system_schema(engine: &str, name: &str) -> bool {
    match engine {
        "mysql" | "mariadb" => matches!(
            name.to_ascii_lowercase().as_str(),
            "information_schema" | "performance_schema" | "mysql" | "sys"
        ),
        _ => false,
    }
}

/// Script de volcado por motor y modo. Solo cambian las banderas, que salen de
/// esta tabla cerrada: nada de lo que llega en la petición se interpola aquí.
/// Los nombres de las bases viajan aparte, como `$@`.
///
/// Los modos con datos usan siempre `INSERT` con las columnas nombradas
/// (`--column-inserts` en PostgreSQL, `--complete-insert` en MySQL y MariaDB).
/// Es más lento de restaurar y ocupa más que el `COPY` o el `INSERT` posicional
/// por omisión, pero el volcado sobrevive a que las columnas cambien de orden.
fn dump_script(engine: &str, mode: &str) -> Option<String> {
    const PG_PREFIX: &str = "export PGPASSWORD=\"${POSTGRES_PASSWORD:-}\"; user=\"${POSTGRES_USER:-postgres}\"; for db in \"$@\"; do printf -- '--\\n-- tinkiva: %s\\n--\\n' \"$db\"; pg_dump -U \"$user\"";
    const PG_SUFFIX: &str = " -d \"$db\" || exit 1; done";
    const MY_PREFIX: &str = "client=$(command -v mysqldump || command -v mariadb-dump) || exit 127; password=\"${MYSQL_PASSWORD:-${MARIADB_PASSWORD:-${MYSQL_ROOT_PASSWORD:-${MARIADB_ROOT_PASSWORD:-}}}}\"; [ -z \"$password\" ] || export MYSQL_PWD=\"$password\"; \"$client\" -u \"${MYSQL_USER:-${MARIADB_USER:-root}}\"";
    const MY_SUFFIX: &str = " --databases \"$@\"";

    let (prefix, flags, suffix) = match (engine, mode) {
        ("postgres", "all") => (PG_PREFIX, " --column-inserts", PG_SUFFIX),
        ("postgres", "structure") => (PG_PREFIX, " --schema-only", PG_SUFFIX),
        ("postgres", "data") => (PG_PREFIX, " --data-only --column-inserts", PG_SUFFIX),
        ("mysql" | "mariadb", "all") => (
            MY_PREFIX,
            " --routines --triggers --events --single-transaction --complete-insert",
            MY_SUFFIX,
        ),
        ("mysql" | "mariadb", "structure") => (
            MY_PREFIX,
            " --no-data --routines --triggers --events",
            MY_SUFFIX,
        ),
        ("mysql" | "mariadb", "data") => (
            MY_PREFIX,
            " --no-create-info --skip-triggers --single-transaction --complete-insert",
            MY_SUFFIX,
        ),
        _ => return None,
    };
    Some(format!("{prefix}{flags}{suffix}"))
}

/// Decide si un contenedor *es* una base de datos, no si *usa* una.
///
/// El cliente instalado dentro manda: sin `psql` no hay consola de PostgreSQL
/// que abrir ni volcado que generar, por muy claro que hable el resto. Y hace
/// falta una segunda señal estructural —la imagen, el proceso o un puerto
/// expuesto—, que viene de cómo se construyó el contenedor.
///
/// El entorno queda deliberadamente fuera: cualquier backend lleva una
/// `DATABASE_URL=postgresql://…:5432/…` y con eso bastaba antes para que el
/// panel tratara una API de Node como si fuera su propia base de datos.
/// Script de restauración por motor. El cliente lee el `.sql` de su entrada
/// estándar y la base de destino llega como `$1`; nada de la petición se
/// interpola en el script.
///
/// PostgreSQL se detiene en el primer error (`ON_ERROR_STOP=1`) pero no envuelve
/// la restauración en una transacción: un volcado hecho con `--create` trae
/// `CREATE DATABASE`, que PostgreSQL prohíbe dentro de una transacción, y esos
/// archivos son justo los que la gente trae de otro servidor.
fn restore_script(engine: &str) -> Option<&'static str> {
    match engine {
        "postgres" => Some(
            "export PGPASSWORD=\"${POSTGRES_PASSWORD:-}\"; exec psql -v ON_ERROR_STOP=1 --quiet -U \"${POSTGRES_USER:-postgres}\" -d \"$1\"",
        ),
        "mysql" | "mariadb" => Some(
            "client=$(command -v mariadb || command -v mysql) || exit 127; password=\"${MYSQL_PASSWORD:-${MARIADB_PASSWORD:-${MYSQL_ROOT_PASSWORD:-${MARIADB_ROOT_PASSWORD:-}}}}\"; [ -z \"$password\" ] || export MYSQL_PWD=\"$password\"; exec \"$client\" -u \"${MYSQL_USER:-${MARIADB_USER:-root}}\" \"$1\"",
        ),
        _ => None,
    }
}

fn detect_database(
    image: &str,
    ports: &str,
    command: &str,
    clients: &str,
) -> Option<&'static str> {
    let structural = |needles: &[&str]| -> bool {
        needles
            .iter()
            .any(|needle| image.contains(needle) || ports.contains(needle) || command.contains(needle))
    };
    let installed = |client: &str| clients.contains(client);

    // MariaDB se evalúa antes que MySQL: ambos suelen incluir el cliente mysql.
    if installed("mariadb=1") && structural(&["mariadb", "3306"]) {
        return Some("mariadb");
    }
    if installed("psql=1") && structural(&["postgres", "pgvector", "postgis", "timescale", "5432"]) {
        return Some("postgres");
    }
    if installed("mysql=1") && structural(&["mysql", "percona", "3306"]) {
        return Some("mysql");
    }
    if installed("mongosh=1") && structural(&["mongo", "27017"]) {
        return Some("mongodb");
    }
    if installed("redis-cli=1") && structural(&["redis", "valkey", "6379"]) {
        return Some("redis");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_compose_service_states() {
        let separator = FIELD_SEPARATOR;
        assert_eq!(summarize_project_status(""), "stopped");
        assert_eq!(
            summarize_project_status(&format!("running{separator}healthy{separator}0\n")),
            "running"
        );
        assert_eq!(
            summarize_project_status(&format!("exited{separator}{separator}0\n")),
            "stopped"
        );
        assert_eq!(
            summarize_project_status(&format!("exited{separator}{separator}1\n")),
            "error"
        );
        assert_eq!(
            summarize_project_status(&format!("running{separator}unhealthy{separator}0\n")),
            "error"
        );
        assert_eq!(
            summarize_project_status(&format!(
                "running{separator}healthy{separator}0\nexited{separator}{separator}0\n"
            )),
            "error"
        );
    }

    #[test]
    fn dump_scripts_cover_the_three_modes_of_each_sql_engine() {
        let structure = dump_script("postgres", "structure").unwrap();
        assert!(structure.contains("--schema-only"));
        assert!(dump_script("postgres", "data").unwrap().contains("--data-only"));
        let full = dump_script("postgres", "all").unwrap();
        assert!(!full.contains("--schema-only") && !full.contains("--data-only"));

        // Los datos siempre salen como INSERT con las columnas nombradas.
        for mode in ["all", "data"] {
            assert!(dump_script("postgres", mode).unwrap().contains("--column-inserts"));
            assert!(dump_script("mysql", mode).unwrap().contains("--complete-insert"));
            assert!(dump_script("mariadb", mode).unwrap().contains("--complete-insert"));
        }
        // Sin datos no hay INSERT que nombrar.
        assert!(!dump_script("postgres", "structure").unwrap().contains("--column-inserts"));
        assert!(!dump_script("mysql", "structure").unwrap().contains("--complete-insert"));

        // Procedimientos, funciones y triggers solo entran con --routines.
        assert!(dump_script("mariadb", "structure").unwrap().contains("--routines --triggers"));
        assert!(dump_script("mysql", "data").unwrap().contains("--no-create-info"));

        // Todos los scripts reciben las bases como argumentos posicionales.
        for engine in ["postgres", "mysql", "mariadb"] {
            for mode in ["all", "structure", "data"] {
                assert!(dump_script(engine, mode).unwrap().contains("\"$@\""));
            }
        }

        assert!(dump_script("mongodb", "all").is_none());
        assert!(dump_script("postgres", "todo").is_none());
    }

    #[test]
    fn restore_scripts_read_stdin_and_never_interpolate_the_target() {
        for engine in ["postgres", "mysql", "mariadb"] {
            let script = restore_script(engine).unwrap();
            // La base de destino se lee como argumento posicional, no se pega.
            assert!(script.contains("\"$1\""));
            // Sin -c ni -f el cliente toma el volcado de su entrada estándar.
            assert!(!script.contains(" -f ") && !script.contains(" -c "));
        }
        // PostgreSQL corta en el primer error en vez de dejar media base.
        assert!(restore_script("postgres").unwrap().contains("ON_ERROR_STOP=1"));
        // Los mismos motores que exportan son los que importan.
        for engine in ["mongodb", "redis"] {
            assert!(restore_script(engine).is_none());
            assert!(!exportable_engine(engine));
        }
    }

    #[test]
    fn system_schemas_are_hidden_only_for_mysql_family() {
        assert!(is_system_schema("mysql", "information_schema"));
        assert!(is_system_schema("mariadb", "SYS"));
        assert!(!is_system_schema("mysql", "app"));
        assert!(!is_system_schema("postgres", "postgres"));
    }

    #[test]
    fn detects_databases_from_combined_signals() {
        assert_eq!(
            detect_database("postgres:16", "5432/tcp", "postgres", "psql=1"),
            Some("postgres")
        );
        assert_eq!(detect_database("node:22", "", "node server.js", ""), None);
        assert_eq!(
            detect_database("mariadb:11", "3306/tcp", "mariadbd", "mariadb=1"),
            Some("mariadb")
        );
        // Imágenes derivadas: el nombre no dice «postgres» pero el puerto y el
        // cliente sí.
        assert_eq!(
            detect_database("pgvector/pgvector:pg17-trixie", "5432/tcp", "postgres", "psql=1"),
            Some("postgres")
        );
    }

    #[test]
    fn an_application_that_talks_to_a_database_is_not_one() {
        // Un backend de Node con su DATABASE_URL apuntando a PostgreSQL: sin
        // cliente dentro no hay consola SQL que ofrecer.
        assert_eq!(
            detect_database(
                "160358212333.dkr.ecr.us-east-1.amazonaws.com/tinkiva-store-dev:latest",
                "3000/tcp",
                "node dist/src/main.js",
                "user=node",
            ),
            None
        );
        // Ni siquiera si alguien instaló psql en la imagen para migraciones:
        // nada en su estructura dice que sea una base de datos.
        assert_eq!(
            detect_database("tinkiva/api:latest", "3000/tcp", "node dist/src/main.js", "psql=1"),
            None
        );
    }
}

fn compose_error_message(error: &str) -> String {
    if error.contains("not a docker command") || error.contains("unknown flag") {
        "Docker Compose no está instalado".to_owned()
    } else {
        truncate_text(error, 500)
    }
}

#[derive(Default)]
struct ContainerStats {
    cpu: Option<String>,
    memory: Option<String>,
    memory_percent: Option<String>,
    network_io: Option<String>,
    block_io: Option<String>,
    pids: Option<String>,
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "--").then(|| value.to_owned())
}
