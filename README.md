# Tinkiva Deploy Lite

Panel de despliegue Docker de un solo nodo, escrito en Rust y diseñado para servidores pequeños. Su objetivo no es reemplazar Coolify, Dokploy o Portainer: cubre únicamente el flujo esencial de Tinkiva con el menor número posible de procesos permanentes.

## Qué incluye

- Un único binario Rust para API, panel web y métricas.
- Cero dependencias externas de Rust (`std` únicamente).
- Interfaz estática embebida: HTML, CSS y JavaScript nativos; no existe un proceso Node en producción.
- Métricas del host desde `/proc` y `df`: CPU, RAM, swap, disco, carga, uptime y RSS del propio panel.
- Lista y métricas de contenedores mediante Docker CLI.
- Logs de contenedores y proyectos Compose.
- Start, stop y restart de contenedores.
- Registro de proyectos Compose existentes.
- Deploy manual o por webhook desde GitHub Actions.
- Restricción opcional por rama.
- Imágenes inmutables por SHA.
- Historial persistente y rollback de imagen.
- Restauración automática del `.env` cuando un deploy falla.
- Plantilla PostgreSQL 17 con volumen, healthcheck, límite de RAM y red privada.
- Autenticación Bearer para el panel y token individual por webhook.
- Instalación como servicio systemd endurecido.

## Lo que deliberadamente no incluye

- Kubernetes, Docker Swarm o múltiples servidores.
- Compilación de aplicaciones dentro del servidor.
- Redis, PostgreSQL o SQLite para el propio panel.
- Prometheus, Grafana, cAdvisor o un agente adicional.
- Gestión automática de DNS o certificados TLS.
- Editor de secretos genérico.
- Terminal web dentro de los contenedores.
- Gestión multiusuario o RBAC.

Estas exclusiones mantienen pequeño el proceso permanente y reducen superficie de ataque.

## Arquitectura

```text
GitHub push (main / dev / uat)
        │
        ▼
GitHub Actions
  build + push GHCR
        │
        │ POST HTTPS + token + image SHA
        ▼
Tinkiva Deploy Lite (Rust, un proceso)
        │
        ├── actualiza APP_IMAGE en .env de forma atómica
        ├── docker compose pull
        ├── docker compose up -d --remove-orphans
        ├── guarda historial local
        └── si falla: restaura .env y redeploy anterior
```

El panel llama al ejecutable `docker`; no mantiene un cliente Docker, base de datos ni runtime pesado residentes. Los procesos `docker`, `docker compose` y `df` son transitorios.

## Requisitos

- Linux con systemd para el instalador incluido.
- Docker Engine y Docker Compose v2.
- Rust 1.85 o superior para compilar el panel. El repositorio y CI están fijados en Rust 1.97.1.
- `curl`, `jq` y Python 3 solo para ejecutar el smoke test; no son necesarios para el servicio.
- Para GitHub Actions: una URL HTTPS que llegue al panel.

Funciona en `x86_64` y `aarch64/arm64` al compilar nativamente para la arquitectura del servidor.

## Compilar

```bash
unzip tinkiva-deploy-lite.zip
cd tinkiva-deploy-lite
./scripts/build-release.sh
```

El binario quedará en:

```text
target/release/tinkiva-deploy-lite
```

El perfil release prioriza tamaño: `opt-level = "z"`, LTO completo, un codegen unit, símbolos removidos y `panic = "abort"`. El estado de las comprobaciones realizadas al generar el paquete está documentado en [`VALIDATION.md`](VALIDATION.md).

## Probar sin tocar Docker real

El repositorio incluye un Docker CLI simulado y un smoke test del ciclo completo:

```bash
cargo build --release
./scripts/smoke-test.sh target/release/tinkiva-deploy-lite
```

Valida:

1. Healthcheck y autenticación.
2. Métricas del host.
3. Listado de contenedores y logs.
4. Registro de proyecto.
5. Dos despliegues con imágenes inmutables.
6. Rollback a la imagen anterior.
7. Historial.
8. Creación de la plantilla PostgreSQL.

## Instalar con systemd

```bash
sudo ./scripts/install.sh target/release/tinkiva-deploy-lite
```

El script crea:

```text
/usr/local/bin/tinkiva-deploy-lite
/etc/tinkiva-deploy-lite/env
/etc/systemd/system/tinkiva-deploy-lite.service
/var/lib/tinkiva-deploy-lite/
/opt/tinkiva/apps/
```

Al terminar imprime el token administrador. También puedes verlo como root:

```bash
sudo sed -n 's/^TDL_ADMIN_TOKEN=//p' /etc/tinkiva-deploy-lite/env
```

Estado y logs del panel:

```bash
sudo systemctl status tinkiva-deploy-lite
sudo journalctl -u tinkiva-deploy-lite -f
```

## Acceder de forma privada

Por defecto escucha solo en `127.0.0.1:8787`. Para administrar el servidor sin publicar el panel:

```bash
ssh -L 8787:127.0.0.1:8787 ubuntu@IP_DEL_SERVIDOR
```

Luego abre:

```text
http://127.0.0.1:8787
```

El token se guarda en `sessionStorage`: se elimina al cerrar la pestaña y no se persiste en `localStorage`.

## Exponerlo para GitHub Actions

GitHub debe alcanzar el webhook por HTTPS. Mantén el backend en localhost y usa un reverse proxy existente. Se incluye `deploy/nginx.example.conf`.

No publiques el puerto directamente por HTTP: tanto el token administrador como el token del webhook son credenciales.

Una opción conservadora es publicar todo el panel por HTTPS y además restringirlo por firewall, VPN o allowlist. El webhook tiene token propio por proyecto y, si configuraste una rama, rechaza cualquier rama diferente.

## Preparar un proyecto Compose

Crea sus archivos bajo `/opt/tinkiva/apps`. El ejemplo incluido está en `examples/app`:

```bash
sudo mkdir -p /opt/tinkiva/apps/storagia
sudo cp examples/app/compose.yaml /opt/tinkiva/apps/storagia/compose.yaml
sudo cp examples/app/.env.example /opt/tinkiva/apps/storagia/.env
sudo cp examples/app/runtime.env.example /opt/tinkiva/apps/storagia/runtime.env
sudo chown -R tinkiva-deploy:docker /opt/tinkiva/apps/storagia
sudo chmod 750 /opt/tinkiva/apps/storagia
sudo chmod 600 /opt/tinkiva/apps/storagia/.env /opt/tinkiva/apps/storagia/runtime.env
```

El archivo `compose.yaml` referencia una variable:

```yaml
services:
  api:
    image: ${APP_IMAGE}
```

Y `.env` contiene únicamente la imagen desplegable:

```dotenv
APP_IMAGE=ghcr.io/isaul19/storagia-api:sha-inicial
```

Se recomienda separar los secretos de runtime en `runtime.env`. Tinkiva Deploy Lite solo necesita modificar `APP_IMAGE`.

Desde el panel registra:

```text
Slug:          storagia-api
Nombre:        Storagia API
Compose:       storagia/compose.yaml
.env:          storagia/.env
Variable:      APP_IMAGE
Rama:          main
```

Las rutas relativas se resuelven dentro de `TDL_ALLOWED_ROOT`. Las rutas canónicas fuera de esa raíz se rechazan, incluyendo escapes mediante enlaces simbólicos.

## GitHub Actions

Copia `examples/github-deploy.yml` a tu repositorio como:

```text
.github/workflows/deploy.yml
```

Configura dos secretos en GitHub:

```text
TDL_WEBHOOK_URL   = https://deploy.tudominio.com/hooks/deploy/storagia-api
TDL_WEBHOOK_TOKEN = token mostrado en la tarjeta del proyecto
```

El workflow:

1. Construye la imagen en GitHub Actions, no en tu EC2.
2. Publica una etiqueta inmutable igual a `${{ github.sha }}` en GHCR.
3. Llama al webhook con `image`, `branch` y `commit`.
4. El servidor solo descarga y reinicia el stack.

Para una EC2 Graviton/t4g usa `platforms: linux/arm64`. Para x86 usa `linux/amd64`. También puedes construir ambas separadas por coma, a costa de más tiempo de CI.

### Imágenes privadas de GHCR

Si el paquete de GHCR es privado, autentica una sola vez al usuario del servicio. Usa un token de GitHub con permiso mínimo de lectura de paquetes:

```bash
printf '%s' 'TU_GITHUB_PAT' | \
  sudo -u tinkiva-deploy env HOME=/var/lib/tinkiva-deploy-lite \
  docker login ghcr.io -u TU_USUARIO_GITHUB --password-stdin
```

Docker guardará la credencial en `/var/lib/tinkiva-deploy-lite/.docker/config.json`, dentro de una ruta accesible para el servicio. Restringe ese archivo a su propietario:

```bash
sudo chown -R tinkiva-deploy:tinkiva-deploy /var/lib/tinkiva-deploy-lite/.docker
sudo chmod 700 /var/lib/tinkiva-deploy-lite/.docker
sudo chmod 600 /var/lib/tinkiva-deploy-lite/.docker/config.json
```

Para evitar almacenar una credencial en el servidor, publica la imagen como paquete público. No incluyas el token de GitHub dentro del webhook ni del archivo Compose.

## Rollback

Cada despliegue exitoso conserva:

```text
imagen nueva
imagen anterior
rama
commit
fecha
duración
origen
resultado
```

El botón **Rollback** coloca la imagen anterior en el `.env` y ejecuta nuevamente Compose. Un segundo rollback alternará a la imagen previa del último despliegue exitoso.

Para que sea confiable, usa etiquetas inmutables por SHA. No uses `latest`.

## PostgreSQL

La sección PostgreSQL crea un proyecto como:

```text
/opt/tinkiva/apps/<slug>/compose.yaml
/opt/tinkiva/apps/<slug>/.env
```

Propiedades predeterminadas:

- PostgreSQL 17 Alpine.
- Volumen Docker persistente.
- `restart: unless-stopped`.
- Healthcheck con `pg_isready`.
- Red externa privada `tinkiva`.
- Sin puerto publicado por defecto.
- Límite predeterminado de 512 MB.
- `no-new-privileges`.

La URI devuelta usa el nombre del contenedor dentro de la red Docker. Cuando se publica un puerto, se enlaza únicamente a `127.0.0.1`.

La contraseña generada se muestra en la respuesta de creación. Guárdala inmediatamente. El panel no incorpora un gestor de secretos.

## Configuración

Archivo predeterminado: `/etc/tinkiva-deploy-lite/env`.

| Variable | Predeterminado | Descripción |
|---|---|---|
| `TDL_BIND` | `127.0.0.1:8787` | Dirección HTTP. |
| `TDL_ADMIN_TOKEN` | obligatorio | Token Bearer de 32 a 256 caracteres. |
| `TDL_DATA_DIR` | `/var/lib/tinkiva-deploy-lite` | Estado local. |
| `TDL_ALLOWED_ROOT` | `/opt/tinkiva/apps` | Única raíz aceptada para Compose y `.env`. |
| `TDL_DOCKER_BIN` | `docker` | Ruta del Docker CLI. |
| `TDL_WORKERS` | `2` | Workers HTTP fijos; rango 1–16. |
| `TDL_MAX_HISTORY` | `200` | Registros conservados; rango 10–10,000. |

Después de editar:

```bash
sudo systemctl restart tinkiva-deploy-lite
```

## API esencial

Todas las rutas `/api/*` requieren:

```http
Authorization: Bearer <TDL_ADMIN_TOKEN>
```

| Método | Ruta | Uso |
|---|---|---|
| `GET` | `/healthz` | Salud sin autenticación. |
| `GET` | `/api/info` | Versión, rutas y Docker. |
| `GET` | `/api/system` | Métricas del host y RSS. |
| `GET` | `/api/containers` | Contenedores y stats. |
| `GET` | `/api/containers/:id/logs?tail=300` | Logs. |
| `POST` | `/api/containers/:id/start` | Iniciar. |
| `POST` | `/api/containers/:id/stop` | Detener. |
| `POST` | `/api/containers/:id/restart` | Reiniciar. |
| `GET` | `/api/projects` | Proyectos. |
| `POST` | `/api/projects` | Registrar proyecto. |
| `DELETE` | `/api/projects/:slug` | Desregistrar sin borrar archivos. |
| `GET` | `/api/projects/:slug/logs` | Logs Compose. |
| `POST` | `/api/projects/:slug/deploy` | Desplegar. |
| `POST` | `/api/projects/:slug/rollback` | Rollback. |
| `GET` | `/api/history` | Historial. |
| `POST` | `/api/templates/postgres` | Crear PostgreSQL. |
| `POST` | `/hooks/deploy/:slug` | Webhook con `X-Tinkiva-Token`. |

Los cuerpos de escritura usan `application/x-www-form-urlencoded`. El servidor limita las cabeceras a 32 KiB y el cuerpo a 128 KiB.

## Consumo de RAM

El diseño busca que el proceso Rust en reposo quede holgadamente por debajo de 100 MB, pero no existe una cifra universal: depende de compilador, libc, arquitectura, workers y tráfico.

Mídelo en tu servidor real:

```bash
sudo ./scripts/measure-memory.sh
```

El panel muestra además su `VmRSS` en la tarjeta **Panel Rust**.

Importante: durante un despliegue, el cgroup del servicio también ejecuta temporalmente Docker CLI. `MemoryCurrent` de systemd puede subir aunque el proceso Rust siga pequeño. Docker Engine y los contenedores administrados no forman parte del RSS del panel.

Los valores predeterminados ya están ajustados para un servidor pequeño:

```dotenv
TDL_WORKERS=2
TDL_MAX_HISTORY=200
```

No se fija `MemoryMax=100M` en systemd porque podría matar el Docker CLI durante un pull. Primero mide y luego añade un límite solo si tus pruebas lo soportan.

## Seguridad y límites de confianza

El usuario del servicio pertenece al grupo `docker`. En Linux, controlar Docker equivale en la práctica a controlar el host. Por eso:

- Instálalo solo en servidores tuyos y para administradores de confianza.
- No es una plataforma multi-tenant.
- Protege el token y rota `/etc/tinkiva-deploy-lite/env` si se filtra.
- Usa HTTPS para cualquier acceso remoto.
- Mantén `TDL_ALLOWED_ROOT` dedicado únicamente a stacks administrados.
- No permitas que usuarios no confiables editen los archivos Compose.
- No introduzcas texto no confiable en nombres de imágenes, ramas o variables.
- Limita el acceso al panel con firewall, VPN o una red privada cuando sea posible.
- Haz backups de volúmenes y bases de datos por separado; el historial del panel no es un backup.

Consulta también [SECURITY.md](SECURITY.md).

## Desinstalar

Conservar configuración e historial:

```bash
sudo ./scripts/uninstall.sh
```

Eliminar también configuración, historial y usuario del servicio:

```bash
sudo ./scripts/uninstall.sh --purge
```

`/opt/tinkiva/apps` nunca se borra automáticamente para proteger tus proyectos y datos.

## Estructura

```text
src/                    servidor HTTP, Docker, métricas, store y dominio
web/                    interfaz embebida sin framework
deploy/                 systemd y ejemplo Nginx
examples/               Compose y GitHub Actions
scripts/                build, instalación, smoke test y medición
tests/mock-docker.sh     Docker simulado para pruebas
.github/workflows/ci.yml CI del propio proyecto
VALIDATION.md            comprobaciones ejecutadas y límite del entorno generador
```

## Estado del MVP

Versión `0.1.0`. El alcance está intencionalmente congelado en un solo host y un solo administrador. Antes de usarlo con datos críticos, prueba deploy, rollback, reinicio del host y restauración de backups en una EC2 de staging.
# tinkiva-docker-manager
