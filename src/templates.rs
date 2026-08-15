//! Catálogo de recursos que el panel sabe crear de cero.
//!
//! Cada plantilla genera un `compose.yaml` endurecido (sin privilegios extra,
//! con límite de memoria, healthcheck, volumen persistente y red interna) más un
//! `.env` con las credenciales. Los puertos se enlazan a `127.0.0.1` por defecto;
//! los servicios de aplicación pueden optar explícitamente por `0.0.0.0`.

use crate::util::json_string;

/// Red Docker compartida por todos los recursos creados desde el panel.
pub const SHARED_NETWORK: &str = "tinkiva";

pub struct DatabaseEngine {
    pub id: &'static str,
    pub label: &'static str,
    pub image: &'static str,
    /// Nombre del servicio dentro del Compose y sufijo del contenedor.
    pub service: &'static str,
    pub port: u16,
    pub scheme: &'static str,
    pub needs_database: bool,
    pub needs_username: bool,
    pub default_memory_mb: u32,
    /// Slug de simple-icons usado por la interfaz.
    pub icon: &'static str,
    pub accent: &'static str,
    pub description: &'static str,
}

pub const ENGINES: [DatabaseEngine; 5] = [
    DatabaseEngine {
        id: "postgres",
        label: "PostgreSQL",
        image: "postgres:17-alpine",
        service: "postgres",
        port: 5432,
        scheme: "postgresql",
        needs_database: true,
        needs_username: true,
        default_memory_mb: 512,
        icon: "postgresql",
        accent: "#4169e1",
        description: "Relacional, JSONB, extensiones. La opción por defecto.",
    },
    DatabaseEngine {
        id: "mysql",
        label: "MySQL",
        image: "mysql:8.4",
        service: "mysql",
        port: 3306,
        scheme: "mysql",
        needs_database: true,
        needs_username: true,
        default_memory_mb: 768,
        icon: "mysql",
        accent: "#4479a1",
        description: "Relacional clásico, máxima compatibilidad con PHP y ORMs.",
    },
    DatabaseEngine {
        id: "mariadb",
        label: "MariaDB",
        image: "mariadb:11.4",
        service: "mariadb",
        port: 3306,
        scheme: "mysql",
        needs_database: true,
        needs_username: true,
        default_memory_mb: 640,
        icon: "mariadb",
        accent: "#c0765a",
        description: "Fork de MySQL, más ligero y con licencia libre.",
    },
    DatabaseEngine {
        id: "mongodb",
        label: "MongoDB",
        image: "mongo:8",
        service: "mongodb",
        port: 27017,
        scheme: "mongodb",
        needs_database: true,
        needs_username: true,
        default_memory_mb: 768,
        icon: "mongodb",
        accent: "#47a248",
        description: "Documental. Ideal para esquemas flexibles.",
    },
    DatabaseEngine {
        id: "redis",
        label: "Redis",
        image: "redis:7-alpine",
        service: "redis",
        port: 6379,
        scheme: "redis",
        needs_database: false,
        needs_username: false,
        default_memory_mb: 256,
        icon: "redis",
        accent: "#ff4438",
        description: "Caché y colas en memoria. Arranca en milisegundos.",
    },
];

pub fn engine(id: &str) -> Option<&'static DatabaseEngine> {
    ENGINES.iter().find(|engine| engine.id == id)
}

/// Catálogo serializado que consume la interfaz para pintar las tarjetas.
pub fn engines_json() -> String {
    let entries: Vec<String> = ENGINES
        .iter()
        .map(|engine| {
            format!(
                concat!(
                    "{{",
                    "\"id\":{},\"label\":{},\"image\":{},\"port\":{},",
                    "\"scheme\":{},\"needs_database\":{},\"needs_username\":{},",
                    "\"default_memory_mb\":{},\"icon\":{},\"accent\":{},\"description\":{}",
                    "}}"
                ),
                json_string(engine.id),
                json_string(engine.label),
                json_string(engine.image),
                engine.port,
                json_string(engine.scheme),
                engine.needs_database,
                engine.needs_username,
                engine.default_memory_mb,
                json_string(engine.icon),
                json_string(engine.accent),
                json_string(engine.description),
            )
        })
        .collect();
    format!("[{}]", entries.join(","))
}

pub struct DatabaseRequest<'a> {
    pub engine: &'static DatabaseEngine,
    pub slug: &'a str,
    pub database: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    pub root_password: &'a str,
    pub published_port: Option<u16>,
    pub external_access: bool,
    pub memory_mb: u32,
}

pub struct GeneratedResource {
    pub compose: String,
    pub env: String,
    pub image: String,
    pub host: String,
    pub connection_uri: String,
}

pub fn database(request: &DatabaseRequest) -> GeneratedResource {
    let engine = request.engine;
    let host = format!("{}-{}", request.slug, engine.service);
    let bind = if request.external_access { "0.0.0.0" } else { "127.0.0.1" };
    let ports = request.published_port.map_or_else(String::new, |port| {
        format!(
            "    ports:\n      - \"{bind}:{port}:{}\"\n",
            engine.port
        )
    });

    let (environment, command, healthcheck, data_path, extra) = match engine.id {
        "postgres" => (
            concat!(
                "      POSTGRES_DB: ${POSTGRES_DB}\n",
                "      POSTGRES_USER: ${POSTGRES_USER}\n",
                "      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}\n",
            ),
            "",
            "[\"CMD-SHELL\", \"pg_isready -U $$POSTGRES_USER -d $$POSTGRES_DB\"]",
            "/var/lib/postgresql/data",
            "    shm_size: 128m\n",
        ),
        "mysql" => (
            concat!(
                "      MYSQL_DATABASE: ${MYSQL_DATABASE}\n",
                "      MYSQL_USER: ${MYSQL_USER}\n",
                "      MYSQL_PASSWORD: ${MYSQL_PASSWORD}\n",
                "      MYSQL_ROOT_PASSWORD: ${MYSQL_ROOT_PASSWORD}\n",
            ),
            "    command: [\"--default-authentication-plugin=caching_sha2_password\"]\n",
            "[\"CMD-SHELL\", \"mysqladmin ping -h 127.0.0.1 -u root -p$$MYSQL_ROOT_PASSWORD --silent\"]",
            "/var/lib/mysql",
            "",
        ),
        "mariadb" => (
            concat!(
                "      MARIADB_DATABASE: ${MARIADB_DATABASE}\n",
                "      MARIADB_USER: ${MARIADB_USER}\n",
                "      MARIADB_PASSWORD: ${MARIADB_PASSWORD}\n",
                "      MARIADB_ROOT_PASSWORD: ${MARIADB_ROOT_PASSWORD}\n",
            ),
            "",
            "[\"CMD-SHELL\", \"healthcheck.sh --connect --innodb_initialized\"]",
            "/var/lib/mysql",
            "",
        ),
        "mongodb" => (
            concat!(
                "      MONGO_INITDB_ROOT_USERNAME: ${MONGO_INITDB_ROOT_USERNAME}\n",
                "      MONGO_INITDB_ROOT_PASSWORD: ${MONGO_INITDB_ROOT_PASSWORD}\n",
                "      MONGO_INITDB_DATABASE: ${MONGO_INITDB_DATABASE}\n",
            ),
            "",
            "[\"CMD-SHELL\", \"mongosh --quiet --eval 'db.adminCommand({ping:1}).ok' | grep -q 1\"]",
            "/data/db",
            "",
        ),
        _ => (
            "      REDIS_PASSWORD: ${REDIS_PASSWORD}\n",
            "    command: [\"sh\", \"-c\", \"exec redis-server --requirepass \\\"$$REDIS_PASSWORD\\\" --appendonly yes\"]\n",
            "[\"CMD-SHELL\", \"redis-cli -a $$REDIS_PASSWORD ping | grep -q PONG\"]",
            "/data",
            "",
        ),
    };

    let compose = format!(
        concat!(
            "# Generado por Tinkiva Docker Manager. Edita con cuidado.\n",
            "services:\n",
            "  {service}:\n",
            "    image: {image}\n",
            "    container_name: {host}\n",
            "    restart: unless-stopped\n",
            "{command}",
            "    environment:\n",
            "{environment}",
            "    env_file:\n",
            "      - .env\n",
            "{ports}",
            "    volumes:\n",
            "      - {service}_data:{data_path}\n",
            "    networks:\n",
            "      - {network}\n",
            "    mem_limit: ${{TDM_MEMORY_LIMIT:-512m}}\n",
            "{extra}",
            "    healthcheck:\n",
            "      test: {healthcheck}\n",
            "      interval: 10s\n",
            "      timeout: 5s\n",
            "      retries: 5\n",
            "      start_period: 30s\n",
            "    security_opt:\n",
            "      - no-new-privileges:true\n",
            "\n",
            "volumes:\n",
            "  {service}_data:\n",
            "\n",
            "networks:\n",
            "  {network}:\n",
            "    external: true\n"
        ),
        service = engine.service,
        image = engine.image,
        host = host,
        command = command,
        environment = environment,
        ports = ports,
        data_path = data_path,
        network = SHARED_NETWORK,
        extra = extra,
        healthcheck = healthcheck,
    );

    let memory = request.memory_mb;
    let env = match engine.id {
        "postgres" => format!(
            "POSTGRES_DB={}\nPOSTGRES_USER={}\nPOSTGRES_PASSWORD={}\nTDM_MEMORY_LIMIT={memory}m\n",
            request.database, request.username, request.password
        ),
        "mysql" => format!(
            "MYSQL_DATABASE={}\nMYSQL_USER={}\nMYSQL_PASSWORD={}\nMYSQL_ROOT_PASSWORD={}\nTDM_MEMORY_LIMIT={memory}m\n",
            request.database, request.username, request.password, request.root_password
        ),
        "mariadb" => format!(
            "MARIADB_DATABASE={}\nMARIADB_USER={}\nMARIADB_PASSWORD={}\nMARIADB_ROOT_PASSWORD={}\nTDM_MEMORY_LIMIT={memory}m\n",
            request.database, request.username, request.password, request.root_password
        ),
        "mongodb" => format!(
            "MONGO_INITDB_DATABASE={}\nMONGO_INITDB_ROOT_USERNAME={}\nMONGO_INITDB_ROOT_PASSWORD={}\nTDM_MEMORY_LIMIT={memory}m\n",
            request.database, request.username, request.password
        ),
        _ => format!(
            "REDIS_PASSWORD={}\nTDM_MEMORY_LIMIT={memory}m\n",
            request.password
        ),
    };

    let connection_uri = match engine.id {
        "mongodb" => format!(
            "mongodb://{}:{}@{host}:{}/{}?authSource=admin",
            request.username, request.password, engine.port, request.database
        ),
        "redis" => format!("redis://:{}@{host}:{}/0", request.password, engine.port),
        _ => format!(
            "{}://{}:{}@{host}:{}/{}",
            engine.scheme, request.username, request.password, engine.port, request.database
        ),
    };

    GeneratedResource {
        compose,
        env,
        image: engine.image.to_owned(),
        host,
        connection_uri,
    }
}

pub struct ServiceRequest<'a> {
    pub slug: &'a str,
    pub image: &'a str,
    pub container_port: Option<u16>,
    pub published_port: Option<u16>,
    pub external_access: bool,
    pub memory_mb: u32,
    pub volume_path: Option<&'a str>,
    /// Pares `CLAVE=valor` ya validados.
    pub environment: &'a [(String, String)],
}

/// Servicio suelto a partir de una imagen ya publicada (Docker Hub, GHCR, …).
pub fn service(request: &ServiceRequest) -> GeneratedResource {
    let host = request.slug.to_owned();
    let bind = if request.external_access { "0.0.0.0" } else { "127.0.0.1" };
    let ports = match (request.published_port, request.container_port) {
        (Some(published), Some(container)) => {
            format!("    ports:\n      - \"{bind}:{published}:{container}\"\n")
        }
        _ => String::new(),
    };
    let volumes = request.volume_path.map_or_else(String::new, |path| {
        format!("    volumes:\n      - app_data:{path}\n")
    });
    let volume_block = if request.volume_path.is_some() {
        "\nvolumes:\n  app_data:\n"
    } else {
        ""
    };

    let compose = format!(
        concat!(
            "# Generado por Tinkiva Docker Manager. Edita con cuidado.\n",
            "services:\n",
            "  app:\n",
            "    image: ${{APP_IMAGE}}\n",
            "    container_name: {host}\n",
            "    restart: unless-stopped\n",
            "    env_file:\n",
            "      - .env\n",
            "{ports}",
            "{volumes}",
            "    networks:\n",
            "      - {network}\n",
            "    mem_limit: ${{TDM_MEMORY_LIMIT:-512m}}\n",
            "    security_opt:\n",
            "      - no-new-privileges:true\n",
            "{volume_block}",
            "\nnetworks:\n",
            "  {network}:\n",
            "    external: true\n"
        ),
        host = host,
        ports = ports,
        volumes = volumes,
        network = SHARED_NETWORK,
        volume_block = volume_block,
    );

    let mut env = format!(
        "APP_IMAGE={}\nTDM_MEMORY_LIMIT={}m\n",
        request.image, request.memory_mb
    );
    for (key, value) in request.environment {
        env.push_str(&format!("{key}={value}\n"));
    }

    let connection_uri = request
        .published_port
        .map_or_else(String::new, |port| format!("http://127.0.0.1:{port}"));

    GeneratedResource {
        compose,
        env,
        image: request.image.to_owned(),
        host,
        connection_uri,
    }
}

pub struct RepositoryRequest<'a> {
    pub slug: &'a str,
    pub repository: &'a str,
    pub branch: &'a str,
    pub dockerfile: &'a str,
    pub build_context: &'a str,
    pub container_port: Option<u16>,
    pub published_port: Option<u16>,
    pub external_access: bool,
    pub memory_mb: u32,
    pub environment: &'a [(String, String)],
}

/// Servicio construido desde un repositorio de GitHub clonado en `<slug>/repo`.
pub fn repository(request: &RepositoryRequest) -> GeneratedResource {
    let host = request.slug.to_owned();
    let bind = if request.external_access { "0.0.0.0" } else { "127.0.0.1" };
    let ports = match (request.published_port, request.container_port) {
        (Some(published), Some(container)) => {
            format!("    ports:\n      - \"{bind}:{published}:{container}\"\n")
        }
        _ => String::new(),
    };

    let compose = format!(
        concat!(
            "# Generado por Tinkiva Docker Manager desde {repository}@{branch}.\n",
            "services:\n",
            "  app:\n",
            "    build:\n",
            "      context: ./repo/{context}\n",
            "      dockerfile: {dockerfile}\n",
            "    image: ${{APP_IMAGE}}\n",
            "    container_name: {host}\n",
            "    restart: unless-stopped\n",
            "    env_file:\n",
            "      - .env\n",
            "{ports}",
            "    networks:\n",
            "      - {network}\n",
            "    mem_limit: ${{TDM_MEMORY_LIMIT:-512m}}\n",
            "    security_opt:\n",
            "      - no-new-privileges:true\n",
            "\nnetworks:\n",
            "  {network}:\n",
            "    external: true\n"
        ),
        repository = request.repository,
        branch = request.branch,
        context = request.build_context.trim_matches('/'),
        dockerfile = request.dockerfile,
        host = host,
        ports = ports,
        network = SHARED_NETWORK,
    );

    // Cada despliegue fija APP_IMAGE a `tinkiva/<slug>:<commit>`: las versiones
    // anteriores quedan en el Docker local y el rollback no necesita reconstruir.
    let mut env = format!("APP_IMAGE=tinkiva/{host}:latest\nTDM_MEMORY_LIMIT={memory}m\n", host = host, memory = request.memory_mb);
    for (key, value) in request.environment {
        env.push_str(&format!("{key}={value}\n"));
    }

    let connection_uri = request
        .published_port
        .map_or_else(String::new, |port| format!("http://127.0.0.1:{port}"));

    GeneratedResource {
        compose,
        env,
        image: format!("tinkiva/{host}:latest"),
        host,
        connection_uri,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(engine_id: &str, port: Option<u16>) -> GeneratedResource {
        database(&DatabaseRequest {
            engine: engine(engine_id).unwrap(),
            slug: "demo",
            database: "app",
            username: "app",
            password: "s3cret-password",
            root_password: "root-password",
            published_port: port,
            external_access: false,
            memory_mb: 512,
        })
    }

    #[test]
    fn every_engine_generates_a_hardened_compose() {
        for engine in &ENGINES {
            let generated = sample(engine.id, None);
            assert!(
                !generated.compose.contains("ports:"),
                "{} publica puertos sin pedirlo",
                engine.id
            );
            assert!(generated.compose.contains("no-new-privileges:true"));
            assert!(generated.compose.contains("healthcheck:"));
            assert!(generated.compose.contains("mem_limit:"));
            assert!(generated.compose.contains("external: true"));
            assert!(generated
                .compose
                .contains(&format!("container_name: demo-{}", engine.service)));
            assert!(generated.env.contains("TDM_MEMORY_LIMIT=512m"));
        }
    }

    #[test]
    fn published_ports_are_bound_to_loopback() {
        let generated = sample("postgres", Some(5433));
        assert!(generated.compose.contains("\"127.0.0.1:5433:5432\""));
    }

    #[test]
    fn external_database_ports_are_bound_to_all_interfaces() {
        let generated = database(&DatabaseRequest {
            engine: engine("postgres").unwrap(),
            slug: "demo",
            database: "app",
            username: "app",
            password: "s3cret-password",
            root_password: "root-password",
            published_port: Some(5433),
            external_access: true,
            memory_mb: 512,
        });
        assert!(generated.compose.contains("\"0.0.0.0:5433:5432\""));
    }

    #[test]
    fn connection_uris_match_each_engine() {
        assert_eq!(
            sample("postgres", None).connection_uri,
            "postgresql://app:s3cret-password@demo-postgres:5432/app"
        );
        assert_eq!(
            sample("mariadb", None).connection_uri,
            "mysql://app:s3cret-password@demo-mariadb:3306/app"
        );
        assert_eq!(
            sample("mongodb", None).connection_uri,
            "mongodb://app:s3cret-password@demo-mongodb:27017/app?authSource=admin"
        );
        assert_eq!(
            sample("redis", None).connection_uri,
            "redis://:s3cret-password@demo-redis:6379/0"
        );
    }

    #[test]
    fn root_password_only_reaches_engines_that_use_it() {
        assert!(sample("mysql", None).env.contains("MYSQL_ROOT_PASSWORD=root-password"));
        assert!(sample("mariadb", None).env.contains("MARIADB_ROOT_PASSWORD=root-password"));
        assert!(!sample("postgres", None).env.contains("root-password"));
        assert!(!sample("redis", None).env.contains("root-password"));
    }

    #[test]
    fn engine_catalog_is_valid_json() {
        let catalog = engines_json();
        let parsed = crate::json::Json::parse(&catalog).unwrap();
        assert_eq!(parsed.as_array().map(<[_]>::len), Some(ENGINES.len()));
    }

    #[test]
    fn image_service_uses_env_indirection_for_rollbacks() {
        let generated = service(&ServiceRequest {
            slug: "cache-proxy",
            image: "nginx:1.27-alpine",
            container_port: Some(80),
            published_port: Some(8080),
            external_access: false,
            memory_mb: 256,
            volume_path: None,
            environment: &[("LOG_LEVEL".to_owned(), "info".to_owned())],
        });
        assert!(generated.compose.contains("image: ${APP_IMAGE}"));
        assert!(generated.compose.contains("\"127.0.0.1:8080:80\""));
        assert!(generated.env.contains("APP_IMAGE=nginx:1.27-alpine"));
        assert!(generated.env.contains("LOG_LEVEL=info"));
    }

    #[test]
    fn image_service_can_be_exposed_on_all_interfaces() {
        let generated = service(&ServiceRequest {
            slug: "public-api",
            image: "node:24-alpine",
            container_port: Some(3000),
            published_port: Some(3000),
            external_access: true,
            memory_mb: 256,
            volume_path: None,
            environment: &[],
        });
        assert!(generated.compose.contains("\"0.0.0.0:3000:3000\""));
    }

    #[test]
    fn repository_can_be_exposed_on_all_interfaces() {
        let generated = repository(&RepositoryRequest {
            slug: "public-api",
            repository: "isaul19/public-api",
            branch: "main",
            dockerfile: "Dockerfile",
            build_context: ".",
            container_port: Some(3000),
            published_port: Some(3000),
            external_access: true,
            memory_mb: 512,
            environment: &[],
        });
        assert!(generated.compose.contains("\"0.0.0.0:3000:3000\""));
    }

    #[test]
    fn repository_build_context_is_relative_to_the_clone() {
        let generated = repository(&RepositoryRequest {
            slug: "storagia-api",
            repository: "isaul19/storagia",
            branch: "main",
            dockerfile: "Dockerfile",
            build_context: "/services/api/",
            container_port: Some(3000),
            published_port: None,
            external_access: false,
            memory_mb: 512,
            environment: &[],
        });
        assert!(generated.compose.contains("context: ./repo/services/api"));
        assert!(!generated.compose.contains("ports:"));
        assert_eq!(generated.image, "tinkiva/storagia-api:latest");
        assert!(generated.compose.contains("image: ${APP_IMAGE}"));
        assert!(generated.env.contains("APP_IMAGE=tinkiva/storagia-api:latest"));
    }
}
