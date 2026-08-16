//! Detección ligera de aplicaciones y Dockerfiles generados por Tinkiva.
//!
//! No ejecuta código del repositorio durante la detección. Solo inspecciona
//! archivos conocidos y produce recetas fijas que Docker construirá después.

use crate::json::Json;
use std::fs;
use std::path::Path;

pub const GENERATED_DOCKERFILE: &str = ".tinkiva.Dockerfile";
pub const BLUEPRINT_FILE: &str = ".tinkiva.Dockerfile";
pub const CONTEXT_FILE: &str = ".tinkiva-build-context";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

pub struct BuildPlan {
    pub dockerfile_name: String,
    pub dockerfile: Option<String>,
    pub runtime: &'static str,
    pub default_port: Option<u16>,
}

pub fn detect(context: &Path, requested_dockerfile: &str) -> Result<BuildPlan, String> {
    let existing = context.join(requested_dockerfile);
    if existing.is_file() {
        return Ok(BuildPlan {
            dockerfile_name: requested_dockerfile.to_owned(),
            dockerfile: None,
            runtime: "docker",
            default_port: dockerfile_exposed_port(&existing)?,
        });
    }

    if context.join("package.json").is_file() {
        return node_plan(context);
    }
    if context.join("requirements.txt").is_file() || context.join("pyproject.toml").is_file() {
        return python_plan(context);
    }
    if context.join("index.html").is_file() {
        return Ok(generated(
            "static",
            Some(80),
            concat!(
                "FROM nginx:1.27-alpine\n",
                "COPY . /usr/share/nginx/html\n",
                "RUN printf 'server { listen 80; root /usr/share/nginx/html; location / { try_files $uri $uri/ /index.html; } }\\n' > /etc/nginx/conf.d/default.conf\n",
                "EXPOSE 80\n",
            ),
        ));
    }

    Err(format!(
        "no se encontró {requested_dockerfile}, package.json, requirements.txt, pyproject.toml ni index.html en el contexto"
    ))
}

fn node_plan(context: &Path) -> Result<BuildPlan, String> {
    let package = read_small(&context.join("package.json"))?;
    let document =
        Json::parse(&package).map_err(|error| format!("package.json inválido: {error}"))?;
    let scripts = document.get("scripts");
    let has_build = scripts.and_then(|value| value.string("build")).is_some();
    let has_start = scripts.and_then(|value| value.string("start")).is_some();
    let lower = package.to_ascii_lowercase();
    let is_next = lower.contains("\"next\"");
    let is_vite = lower.contains("\"vite\"");
    let is_cra = lower.contains("\"react-scripts\"");
    let install = node_install(context);

    if has_build && !is_next && (is_vite || is_cra) {
        let output = if is_cra { "build" } else { "dist" };
        let dockerfile = format!(
            concat!(
                "FROM node:22-alpine AS build\n",
                "WORKDIR /app\n",
                "COPY {manifests} ./\n",
                "RUN {install}\n",
                "COPY . .\n",
                "RUN {build}\n",
                "FROM nginx:1.27-alpine\n",
                "COPY --from=build /app/{output} /usr/share/nginx/html\n",
                "RUN printf 'server {{ listen 80; root /usr/share/nginx/html; location / {{ try_files $uri $uri/ /index.html; }} }}\\n' > /etc/nginx/conf.d/default.conf\n",
                "EXPOSE 80\n",
            ),
            manifests = install.manifests,
            install = install.install,
            build = install.build,
            output = output,
        );
        return Ok(generated("static", Some(80), &dockerfile));
    }

    let command = if has_start {
        format!("CMD {}\n", install.start_json)
    } else if context.join("server.js").is_file() {
        "CMD [\"node\",\"server.js\"]\n".to_owned()
    } else if context.join("index.js").is_file() {
        "CMD [\"node\",\"index.js\"]\n".to_owned()
    } else {
        return Err("se detectó Node.js, pero package.json no tiene script start ni existe server.js/index.js".to_owned());
    };
    let build = if has_build {
        format!("RUN {}\n", install.build)
    } else {
        String::new()
    };
    let dockerfile = format!(
        concat!(
            "FROM node:22-alpine\n",
            "WORKDIR /app\n",
            "COPY {manifests} ./\n",
            "RUN {install}\n",
            "COPY . .\n",
            "{build}",
            "ENV NODE_ENV=production\n",
            "EXPOSE 3000\n",
            "{command}",
        ),
        manifests = install.manifests,
        install = install.install,
        build = build,
        command = command,
    );
    Ok(generated("node", Some(3000), &dockerfile))
}

struct NodeInstall {
    manifests: &'static str,
    install: &'static str,
    build: &'static str,
    start_json: &'static str,
}

fn node_install(context: &Path) -> NodeInstall {
    if context.join("pnpm-lock.yaml").is_file() {
        NodeInstall {
            manifests: "package.json pnpm-lock.yaml",
            install: "corepack enable && pnpm install --frozen-lockfile",
            build: "corepack enable && pnpm run build",
            start_json: "[\"pnpm\",\"start\"]",
        }
    } else if context.join("yarn.lock").is_file() {
        NodeInstall {
            manifests: "package.json yarn.lock",
            install: "corepack enable && yarn install --frozen-lockfile",
            build: "corepack enable && yarn build",
            start_json: "[\"yarn\",\"start\"]",
        }
    } else if context.join("package-lock.json").is_file() {
        NodeInstall {
            manifests: "package.json package-lock.json",
            install: "npm ci",
            build: "npm run build",
            start_json: "[\"npm\",\"start\"]",
        }
    } else {
        NodeInstall {
            manifests: "package.json",
            install: "npm install",
            build: "npm run build",
            start_json: "[\"npm\",\"start\"]",
        }
    }
}

fn python_plan(context: &Path) -> Result<BuildPlan, String> {
    let requirements = context.join("requirements.txt");
    let pyproject = context.join("pyproject.toml");
    let manifest = if requirements.is_file() {
        read_small(&requirements)?
    } else {
        read_small(&pyproject)?
    };
    let lower = manifest.to_ascii_lowercase();
    let install = if requirements.is_file() {
        "COPY requirements.txt .\nRUN pip install --no-cache-dir -r requirements.txt\n"
    } else {
        "COPY . .\nRUN pip install --no-cache-dir .\n"
    };
    let copy = if requirements.is_file() {
        "COPY . .\n"
    } else {
        ""
    };

    let command = if lower.contains("fastapi") {
        let module = python_module(context)?;
        format!("CMD [\"uvicorn\",\"{module}:app\",\"--host\",\"0.0.0.0\",\"--port\",\"8000\"]\n")
    } else if lower.contains("flask") {
        let module = python_module(context)?;
        format!("RUN pip install --no-cache-dir gunicorn\nCMD [\"gunicorn\",\"--bind\",\"0.0.0.0:8000\",\"{module}:app\"]\n")
    } else if context.join("manage.py").is_file() {
        let module = django_module(context).ok_or_else(|| {
            "se detectó Django, pero no se encontró un archivo wsgi.py".to_owned()
        })?;
        format!("RUN pip install --no-cache-dir gunicorn\nCMD [\"gunicorn\",\"--bind\",\"0.0.0.0:8000\",\"{module}.wsgi:application\"]\n")
    } else if context.join("main.py").is_file() {
        "CMD [\"python\",\"main.py\"]\n".to_owned()
    } else if context.join("app.py").is_file() {
        "CMD [\"python\",\"app.py\"]\n".to_owned()
    } else {
        return Err(
            "se detectó Python, pero no se pudo determinar cómo iniciar la aplicación".to_owned(),
        );
    };

    Ok(generated(
        "python",
        Some(8000),
        &format!("FROM python:3.13-slim\nWORKDIR /app\n{install}{copy}EXPOSE 8000\n{command}"),
    ))
}

fn python_module(context: &Path) -> Result<&'static str, String> {
    if context.join("main.py").is_file() {
        Ok("main")
    } else if context.join("app.py").is_file() {
        Ok("app")
    } else {
        Err("se detectó el framework Python, pero falta main.py o app.py".to_owned())
    }
}

fn django_module(context: &Path) -> Option<String> {
    fs::read_dir(context)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let path = entry.path();
            (path.is_dir() && path.join("wsgi.py").is_file())
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
}

fn generated(runtime: &'static str, default_port: Option<u16>, dockerfile: &str) -> BuildPlan {
    BuildPlan {
        dockerfile_name: GENERATED_DOCKERFILE.to_owned(),
        dockerfile: Some(dockerfile.to_owned()),
        runtime,
        default_port,
    }
}

fn dockerfile_exposed_port(path: &Path) -> Result<Option<u16>, String> {
    let contents = read_small(path)?;
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(instruction) = parts.next() else {
            continue;
        };
        if !instruction.eq_ignore_ascii_case("EXPOSE") {
            continue;
        }
        for value in parts {
            let candidate = value.split('/').next().unwrap_or(value);
            if let Ok(port) = candidate.parse::<u16>() {
                if port > 0 {
                    return Ok(Some(port));
                }
            }
        }
    }
    Ok(None)
}

fn read_small(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("no se pudo leer {}: {error}", path.display()))?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{} es demasiado grande", path.display()));
    }
    fs::read_to_string(path).map_err(|error| format!("no se pudo leer {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("tdm-buildpack-{}-{suffix}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn prefers_a_repository_dockerfile() {
        let path = fixture();
        fs::write(path.join("Dockerfile"), "FROM scratch\nEXPOSE 3000/tcp\n").unwrap();
        let plan = detect(&path, "Dockerfile").unwrap();
        assert_eq!(plan.runtime, "docker");
        assert_eq!(plan.default_port, Some(3000));
        assert!(plan.dockerfile.is_none());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn generates_a_vite_multistage_image() {
        let path = fixture();
        fs::write(
            path.join("package.json"),
            r#"{"scripts":{"build":"vite build"},"devDependencies":{"vite":"latest"}}"#,
        )
        .unwrap();
        fs::write(path.join("package-lock.json"), "{}").unwrap();
        let plan = detect(&path, "Dockerfile").unwrap();
        let dockerfile = plan.dockerfile.unwrap();
        assert_eq!(plan.runtime, "static");
        assert_eq!(plan.default_port, Some(80));
        assert!(dockerfile.contains("npm ci"));
        assert!(dockerfile.contains("FROM nginx:1.27-alpine"));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn generates_a_fastapi_image() {
        let path = fixture();
        fs::write(path.join("requirements.txt"), "fastapi\nuvicorn\n").unwrap();
        fs::write(path.join("main.py"), "app = None\n").unwrap();
        let plan = detect(&path, "Dockerfile").unwrap();
        assert_eq!(plan.runtime, "python");
        assert!(plan.dockerfile.unwrap().contains("main:app"));
        fs::remove_dir_all(path).unwrap();
    }
}
