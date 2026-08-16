# Tinkiva Docker Manager

Panel de despliegue Docker de un solo nodo, escrito en Rust y diseñado para servidores pequeños. Su objetivo no es reemplazar Coolify, Dokploy o Portainer: cubre únicamente el flujo esencial de Tinkiva con el menor número posible de procesos permanentes.

## Qué incluye

- Un único binario Rust para API, panel web y métricas.
- Cero dependencias externas de Rust (`std` únicamente).
- Interfaz Preact embebida en el binario; no existe un proceso Node en producción.
- Métricas del host desde `/proc` y `df`: CPU, RAM, swap, disco, carga, uptime y RSS del propio panel.
- Lista y métricas de contenedores mediante Docker CLI.
- Logs de contenedores y proyectos Compose.
- Start, stop y restart de contenedores.
- Alta de recursos en un diálogo guiado, con cuatro orígenes:
  - **Bases de datos**: PostgreSQL, MySQL, MariaDB, MongoDB y Redis, con volumen,
    healthcheck, límite de RAM y red privada.
  - **Imágenes de Docker Hub**: buscador con autocompletado y selector de etiquetas.
  - **Repositorios de GitHub**: clonado, build y redespliegue automático en cada `push`.
  - **Compose existente**: registro de un stack que ya vive en el servidor.
- Integración con GitHub App de un clic: el panel te lleva a GitHub, GitHub crea la App
  y vuelve con las credenciales; después eliges en qué repositorios instalarla.
- Deploy manual, por polling saliente o por webhook propio opcional.
- Restricción opcional por rama.
- Imágenes inmutables por SHA.
- Historial persistente y rollback de imagen.
- Restauración automática del `.env` cuando un deploy falla.
- Autenticación Bearer para el panel y token individual por webhook.
- Instalación como servicio systemd endurecido.

## Lo que deliberadamente no incluye

- Kubernetes, Docker Swarm o múltiples servidores.
- Redis, PostgreSQL o SQLite para el propio panel.
- Prometheus, Grafana, cAdvisor o un agente adicional.
- Gestión automática de DNS o certificados TLS.
- Editor de secretos genérico.
- Terminal web dentro de los contenedores.
- Gestión multiusuario o RBAC.

Estas exclusiones mantienen pequeño el proceso permanente y reducen superficie de ataque.

## Arquitectura

```text
GitHub API                     Registry
 commit SHA                   image digest
        │                       │
        └────── watcher saliente ─────┘
                    │
                    ▼
Tinkiva Docker Manager (Rust, un proceso)
        │
        ├── actualiza APP_IMAGE en .env de forma atómica
        ├── docker compose pull
        ├── docker compose up -d --remove-orphans
        ├── guarda historial local
        └── si falla: restaura .env y redeploy anterior
```

El panel llama al ejecutable `docker`; no mantiene un cliente Docker, base de datos ni runtime pesado residentes. Los procesos `docker`, `docker compose` y `df` son transitorios.

La misma decisión se aplica a todo lo demás que necesita salir de la máquina. En vez de
enlazar una pila TLS, un cliente HTTP y una librería de criptografía —lo que multiplicaría
el tamaño del binario y su consumo—, el panel invoca herramientas que ya están en cualquier
servidor Linux:

| Herramienta | Para qué | Si falta |
| --- | --- | --- |
| `docker` | Todo el ciclo de contenedores | El panel arranca pero avisa |
| `curl` | Docker Hub y API de GitHub | Se desactiva el buscador de imágenes y GitHub |
| `openssl` | Firmar los JWT de la GitHub App | Se desactiva GitHub |
| `git` | Clonar y actualizar repositorios | Se desactivan los recursos de repositorio |

Ninguna queda residente: viven milisegundos y mueren. La página **Sistema** del panel
muestra cuáles están disponibles.

## Requisitos

- Linux con systemd para el instalador incluido.
- Docker Engine y Docker Compose v2.
- `curl`, `openssl` y `git` para el buscador de Docker Hub y la integración con GitHub.
  Son opcionales: sin ellos el resto del panel funciona igual.
- Rust 1.85 o superior para compilar el panel. El repositorio y CI están fijados en Rust 1.97.1.
- Node 20 o superior **solo si vas a modificar la interfaz**; el bundle ya viene compilado
  en `web/dist/`, así que `cargo build` no necesita Node.
- `curl`, `jq` y Python 3 solo para ejecutar el smoke test; no son necesarios para el servicio.
- Para GitHub Actions: una URL HTTPS que llegue al panel.

Funciona en `x86_64` y `aarch64/arm64` al compilar nativamente para la arquitectura del servidor.

## Instalación

Hay dos rutas; ambas terminan con el binario `tmanager` listo para usar. Si solo quieres correr el panel, usa la **Opción A** (sin compilar). Si vas a modificar el código o prefieres construir tu propio binario, usa la **Opción B**.

### Opción A — Descargar el binario (recomendado)

Los binarios publicados en GitHub Releases son estáticos (musl): funcionan en Ubuntu, Debian y Amazon Linux, en `x86_64` y `arm64`, sin dependencias. La detección de arquitectura es automática (`amd64` o `arm64` según tu CPU):

```bash
ARCH=$(uname -m | sed 's/x86_64/amd64/; s/aarch64/arm64/')
curl --fail --location -O \
  "https://github.com/isaul19/tinkiva-docker-manager/releases/latest/download/tinkiva-docker-manager-linux-${ARCH}.sha256"
curl --fail --location -O \
  "https://github.com/isaul19/tinkiva-docker-manager/releases/latest/download/tinkiva-docker-manager-linux-${ARCH}"
sha256sum -c "tinkiva-docker-manager-linux-${ARCH}.sha256" \
  && sudo install -m 0755 "tinkiva-docker-manager-linux-${ARCH}" /usr/local/bin/tmanager
tmanager start
```

`tmanager start` abre el asistente de primera ejecución (token, directorios y puerto) y deja el panel corriendo en segundo plano.

### Opción B — Compilar desde el código

Requiere Rust 1.85+ (el repositorio está fijado en 1.97.1):

```bash
git clone https://github.com/isaul19/tinkiva-docker-manager.git
cd tinkiva-docker-manager
./scripts/build-release.sh
```

`build-release.sh` ejecuta clippy, tests y el build release; el binario queda en `target/release/tmanager`. El perfil release prioriza tamaño: `opt-level = "z"`, LTO completo, un codegen unit, símbolos removidos y `panic = "abort"`. El detalle de las comprobaciones está en [`VALIDATION.md`](VALIDATION.md).

Desde aquí tienes dos alternativas:

```bash
# B1 — Uso directo con el asistente (igual que la Opción A)
./target/release/tmanager start

# B2 — Instalar como servicio systemd (usuario dedicado, arranque automático)
sudo ./scripts/install.sh target/release/tmanager
```

### Primer arranque con asistente

Al ejecutar `tmanager` sin `TDM_ADMIN_TOKEN`, sin variables de entorno y sin archivo `tinkiva.env`, se abre un asistente interactivo en la terminal:

```text
? Token administrador:
    1) Generar automáticamente (recomendado)
    2) Ingresar mi propio token (32–256 caracteres)
  Selección [default: 1]:
? Directorio de datos (estado local) [default: ./tinkiva/data]:
? Raíz permitida para apps Compose [default: ./tinkiva/apps]:
? Puerto [default: 8787]:
```

Enter acepta el valor por defecto. Al terminar, el asistente escribe `tinkiva.env` (permisos `0600`) con `TDM_BIND`, `TDM_ADMIN_TOKEN`, `TDM_DATA_DIR` y `TDM_ALLOWED_ROOT`, muestra el token una sola vez y arranca el servidor. En los siguientes arranques el archivo se lee automáticamente; las variables de entorno tienen prioridad sobre él. La ruta del archivo puede cambiarse con `TDM_CONFIG_FILE`. En modo no interactivo (sin stdin, p. ej. systemd sin `EnvironmentFile`) el asistente se omite y se exige `TDM_ADMIN_TOKEN`.

Si ya existe configuración, ejecutar `tmanager` a secas muestra un menú: iniciar en segundo plano / iniciar en primer plano / volver a configurar / actualizar / salir.

### Comandos del binario

| Comando | Uso |
|---|---|
| `tmanager` | Menú interactivo con config existente; primer plano en el primer uso. |
| `tmanager start` | Arranca el panel en segundo plano (asistente si no hay config). Logs en `tinkiva-docker-manager/tinkiva.log`. |
| `tmanager stop` | Detiene la instancia en segundo plano (SIGTERM; fuerza `-9` si no baja). |
| `tmanager status` | Muestra si está en ejecución, el pid y la URL del panel. |
| `tmanager logs [N] [-f]` | Últimas N líneas del log (default 50); `-f` lo sigue en vivo. |
| `tmanager config` | Reejecuta el asistente; tus valores actuales se ofrecen como default. |
| `tmanager token` | Imprime el token administrador (solo el token, apto para tuberías). |
| `tmanager update [versión]` | Descarga una release de GitHub, verifica sha256 y se reemplaza. |
| `tmanager uninstall [--purge] [--yes]` | Detiene el panel y elimina la instalación. Ver abajo. |
| `tmanager version` | Imprime la versión actual. |
| `tmanager help` | Muestra todos los comandos. |

Sin systemd, el binario gestiona su propio demonio. Todo vive dentro de `tinkiva-docker-manager/` en el directorio donde lo ejecutes: `tinkiva-docker-manager/tinkiva.env` (config), `tinkiva-docker-manager/tinkiva.pid` (proceso) y `tinkiva-docker-manager/tinkiva.log` (salida), junto a `data` y `apps`. Al terminar el asistente se ofrecen a borrar los binarios descargados residuales (`tinkiva-docker-manager-linux-*`), y una carpeta `tinkiva/` de versiones anteriores se renombra automáticamente a `tinkiva-docker-manager/`.

### Servicio systemd (Opción B2)

`install.sh` crea usuario dedicado, directorios endurecidos y la unidad systemd. Queda:

```text
/usr/local/bin/tmanager
/etc/tinkiva-docker-manager/env
/etc/systemd/system/tinkiva-docker-manager.service
/var/lib/tinkiva-docker-manager/
/opt/tinkiva/apps/
```

Al terminar imprime el token administrador. También puedes verlo como root:

```bash
sudo sed -n 's/^TDM_ADMIN_TOKEN=//p' /etc/tinkiva-docker-manager/env
```

Estado y logs del panel:

```bash
sudo systemctl status tinkiva-docker-manager
sudo journalctl -u tinkiva-docker-manager -f
```

Con systemd no uses `start`/`stop`: el servicio gestiona el proceso. Esos comandos son para instalaciones directas (Opción A / B1).

### Probar sin tocar Docker real

El repositorio incluye un Docker CLI simulado y un smoke test del ciclo completo:

```bash
cargo build --release
./scripts/smoke-test.sh target/release/tmanager
```

Valida healthcheck y autenticación, métricas del host, listado de contenedores y logs, registro de proyecto, dos despliegues con imágenes inmutables, rollback, historial y creación de la plantilla PostgreSQL.

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

## Webhook propio opcional

El auto-deploy normal no requiere exponer el panel. Si eliges integrar un sistema externo
con `/hooks/deploy/:slug`, ese sistema sí debe poder alcanzar el endpoint por HTTPS. Mantén
el backend en localhost y usa un reverse proxy existente; se incluye `deploy/nginx.example.conf`.

No publiques el puerto directamente por HTTP: tanto el token administrador como el token del webhook son credenciales.

Una opción conservadora es publicar todo el panel por HTTPS y además restringirlo por firewall, VPN o allowlist. El webhook tiene token propio por proyecto y, si configuraste una rama, rechaza cualquier rama diferente.

## Preparar un proyecto Compose

Crea sus archivos bajo `/opt/tinkiva/apps`. El ejemplo incluido está en `examples/app`:

```bash
sudo mkdir -p /opt/tinkiva/apps/storagia
sudo cp examples/app/compose.yaml /opt/tinkiva/apps/storagia/compose.yaml
sudo cp examples/app/.env.example /opt/tinkiva/apps/storagia/.env
sudo cp examples/app/runtime.env.example /opt/tinkiva/apps/storagia/runtime.env
sudo chown -R tinkiva-docker:docker /opt/tinkiva/apps/storagia
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

Se recomienda separar los secretos de runtime en `runtime.env`. Tinkiva Docker Manager solo necesita modificar `APP_IMAGE`.

Desde el panel registra:

```text
Slug:          storagia-api
Nombre:        Storagia API
Compose:       storagia/compose.yaml
.env:          storagia/.env
Variable:      APP_IMAGE
Rama:          main
```

Las rutas relativas se resuelven dentro de `TDM_ALLOWED_ROOT`. Las rutas canónicas fuera de esa raíz se rechazan, incluyendo escapes mediante enlaces simbólicos.

## GitHub Actions

Copia `examples/github-deploy.yml` a tu repositorio como:

```text
.github/workflows/deploy.yml
```

Configura dos secretos en GitHub:

```text
TDM_WEBHOOK_URL   = https://deploy.tudominio.com/hooks/deploy/storagia-api
TDM_WEBHOOK_TOKEN = token mostrado en la tarjeta del proyecto
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
  sudo -u tinkiva-docker env HOME=/var/lib/tinkiva-docker-manager \
  docker login ghcr.io -u TU_USUARIO_GITHUB --password-stdin
```

Docker guardará la credencial en `/var/lib/tinkiva-docker-manager/.docker/config.json`, dentro de una ruta accesible para el servicio. Restringe ese archivo a su propietario:

```bash
sudo chown -R tinkiva-docker:tinkiva-docker /var/lib/tinkiva-docker-manager/.docker
sudo chmod 700 /var/lib/tinkiva-docker-manager/.docker
sudo chmod 600 /var/lib/tinkiva-docker-manager/.docker/config.json
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

## Bases de datos

El diálogo **Añadir recurso → Base de datos** crea un proyecto completo:

```text
/opt/tinkiva/apps/<slug>/compose.yaml
/opt/tinkiva/apps/<slug>/.env
```

| Motor | Imagen | Puerto interno | RAM predeterminada |
|---|---|---|---|
| PostgreSQL | `postgres:17-alpine` | 5432 | 512 MB |
| MySQL | `mysql:8.4` | 3306 | 768 MB |
| MariaDB | `mariadb:11.4` | 3306 | 640 MB |
| MongoDB | `mongo:8` | 27017 | 768 MB |
| Redis | `redis:7-alpine` | 6379 | 256 MB |

Todos comparten las mismas garantías:

- Volumen Docker persistente.
- `restart: unless-stopped`.
- Healthcheck propio de cada motor.
- Red externa privada `tinkiva`.
- Sin puerto publicado salvo que lo pidas; y si lo pides, solo en `127.0.0.1`.
- `mem_limit` configurable desde el formulario.
- `no-new-privileges`.

La cadena de conexión devuelta usa el nombre del contenedor dentro de la red Docker, de
modo que otros contenedores de la red `tinkiva` llegan a la base de datos sin exponer
ningún puerto al exterior.

La contraseña generada se muestra **una sola vez**, al crear el recurso. Guárdala
inmediatamente: queda escrita en el `.env` del recurso pero el panel no la vuelve a
mostrar ni incorpora un gestor de secretos.

## Imágenes de registry

**Añadir recurso → Imagen de Docker Hub** busca en Docker Hub mientras escribes, pero
también acepta una referencia exacta de otro registry, por ejemplo
`ghcr.io/isaul19/storagia:main`. Para imágenes privadas usa antes `docker login` en el
servidor. La imagen se guarda como `APP_IMAGE` en el
`.env`, así que el rollback y el deploy por imagen funcionan igual que en un proyecto
Compose registrado a mano.

Con **Auto Deploy** activo, el watcher ejecuta un pull ligero y compara el digest aplicado;
solo recrea el servicio cuando aparece una imagen nueva.

La búsqueda la resuelve el servidor con `curl`; el navegador nunca habla con Docker Hub,
de modo que la CSP del panel sigue siendo `connect-src 'self'`.

## GitHub

**Añadir recurso → Repositorio de GitHub** necesita conectar antes una GitHub App desde la
sección **GitHub** del panel:

1. Pulsa **Conectar con GitHub**. El panel envía un manifiesto sin webhook ni eventos, con
   los permisos mínimos (`contents: read`, `metadata: read`).
2. GitHub vuelve al panel con un código que se canjea por el App ID y la clave privada.
   Todo queda en `<TDM_DATA_DIR>/github.json` con permisos `0600`.
3. Pulsa **Instalar en repositorios** y elige todos o solo algunos.

A partir de ahí, crear un recurso desde un repositorio clona la rama elegida en
`<slug>/repo`, genera un Compose con `build:` y construye la imagen en el servidor. Si hay
un `Dockerfile`, lo utiliza. En caso contrario puede generar una receta interna para:

- Node.js con npm, pnpm o Yarn.
- Frontends Vite o Create React App, compilados y servidos por Nginx con fallback SPA.
- Python con FastAPI, Flask, Django o un `main.py`/`app.py` convencional.
- Sitios estáticos que tengan `index.html`.

En monorepos se puede indicar la carpeta de la aplicación como contexto. El Dockerfile
generado se guarda fuera del clon y se restaura después de cada sincronización, sin modificar
el repositorio remoto. El watcher compara periódicamente el SHA remoto con el último commit
desplegado y solo construye cuando cambia.

### Si entras por un túnel SSH o localhost

Funciona sin configuración adicional. Los callbacks los sigue el navegador y el watcher
solo realiza conexiones HTTPS salientes a GitHub y a los registries. No necesitas dominio,
TLS ni publicar el puerto `8787`.

Ten en cuenta que construir imágenes consume CPU y RAM del propio servidor. En máquinas
muy pequeñas suele salir más barato construir en GitHub Actions y desplegar por imagen.

Si prefieres crear la App a mano, **Ya tengo una GitHub App** acepta App ID, slug y clave
privada PEM.

## Configuración

Archivo predeterminado: `/etc/tinkiva-docker-manager/env`.

| Variable | Predeterminado | Descripción |
|---|---|---|
| `TDM_BIND` | `127.0.0.1:8787` | Dirección HTTP. |
| `TDM_ADMIN_TOKEN` | obligatorio | Token Bearer de 32 a 256 caracteres. |
| `TDM_DATA_DIR` | `/var/lib/tinkiva-docker-manager` | Estado local. |
| `TDM_ALLOWED_ROOT` | `/opt/tinkiva/apps` | Única raíz aceptada para Compose y `.env`. |
| `TDM_DOCKER_BIN` | `docker` | Ruta del Docker CLI. |
| `TDM_GIT_BIN` | `git` | Ruta de git, usada para los recursos de repositorio. |
| `TDM_WORKERS` | `2` | Workers HTTP fijos; rango 1–16. |
| `TDM_MAX_HISTORY` | `200` | Registros conservados; rango 10–10,000. |
| `TDM_POLL_INTERVAL_SECONDS` | `60` | Intervalo del watcher; rango 30–86,400 segundos. |

Después de editar:

```bash
sudo systemctl restart tinkiva-docker-manager
```

## API esencial

Todas las rutas `/api/*` requieren:

```http
Authorization: Bearer <TDM_ADMIN_TOKEN>
```

| Método | Ruta | Uso |
|---|---|---|
| `GET` | `/healthz` | Salud sin autenticación. |
| `GET` | `/api/info` | Versión, rutas, Docker y herramientas disponibles. |
| `GET` | `/api/catalog` | Motores de base de datos e imágenes sugeridas. |
| `GET` | `/api/system` | Métricas del host y RSS. |
| `GET` | `/api/processes` | Top procesos del host por CPU y RAM. |
| `GET` | `/api/containers` | Contenedores y stats. |
| `GET` | `/api/containers/:id/logs?tail=300` | Logs. |
| `POST` | `/api/containers/:id/start` | Iniciar. |
| `POST` | `/api/containers/:id/stop` | Detener. |
| `POST` | `/api/containers/:id/restart` | Reiniciar. |
| `GET` | `/api/projects` | Proyectos. |
| `POST` | `/api/projects` | Registrar proyecto. |
| `DELETE` | `/api/projects/:slug` | Desregistrar. `?remove=stack` detiene contenedores; `?remove=all` borra además volúmenes y archivos. |
| `GET` | `/api/projects/:slug/logs` | Logs Compose. |
| `POST` | `/api/projects/:slug/deploy` | Desplegar. |
| `POST` | `/api/projects/:slug/rollback` | Rollback. |
| `GET` | `/api/history` | Historial. |
| `POST` | `/api/resources/database` | Crear base de datos (`engine=postgres\|mysql\|mariadb\|mongodb\|redis`). |
| `POST` | `/api/resources/image` | Crear servicio desde una imagen publicada. |
| `POST` | `/api/resources/repository` | Crear servicio desde un repositorio de GitHub. |
| `POST` | `/api/templates/postgres` | Alias histórico de `resources/database` con `engine=postgres`. |
| `GET` | `/api/registry/search?q=` | Buscar imágenes en Docker Hub. |
| `GET` | `/api/registry/tags?image=` | Etiquetas de una imagen. |
| `GET` | `/api/github` | Estado de la GitHub App. |
| `POST` | `/api/github/manifest` | Manifiesto para el alta de un clic. |
| `POST` | `/api/github/manual` | Alta manual con App ID y clave privada. |
| `POST` | `/api/github/install` | URL de instalación con estado de un solo uso. |
| `DELETE` | `/api/github` | Desconectar la App del panel. |
| `GET` | `/api/github/installations` | Cuentas donde está instalada. |
| `GET` | `/api/github/repositories?installation_id=` | Repositorios accesibles. |
| `GET` | `/api/github/branches?installation_id=&repository=` | Ramas de un repositorio. |
| `POST` | `/hooks/deploy/:slug` | Webhook propio con `X-Tinkiva-Token`. |

Los cuerpos de escritura usan `application/x-www-form-urlencoded`. El servidor limita las
cabeceras a 32 KiB y el cuerpo a 512 KiB.

Los retornos del navegador desde GitHub (`/github/callback` y `/github/installed`) no
llevan cabecera `Authorization` porque son navegaciones, no llamadas de la interfaz; se
validan con un nonce de un solo uso que caduca a los 15 minutos.

## Consumo de RAM

El diseño busca que el proceso Rust en reposo quede holgadamente por debajo de 100 MB, pero no existe una cifra universal: depende de compilador, libc, arquitectura, workers y tráfico.

Medición sobre el smoke test (Linux x86_64, glibc, 2 workers), desglosando `smaps_rollup`:

| | 0.1.5 | 0.2.0 |
|---|---|---|
| RSS total | 2.34 MB | 2.49 MB |
| Compartido con el sistema (libc, ld.so) | 1.68 MB | 1.75 MB |
| Privado limpio (código y datos estáticos, desalojable) | 504 KB | 580 KB |
| **Privado sucio (heap y pilas reales)** | **164 KB** | **164 KB** |

La interfaz Preact no aumentó la memoria privada del proceso: el bundle vive en la
sección de solo lectura del binario y se sirve prestado desde ahí, sin copiarse por
petición. Bajo 900 descargas seguidas del bundle el RSS no se movió ni un kilobyte.

Mídelo en tu servidor real:

```bash
sudo ./scripts/measure-memory.sh
```

El panel muestra además su `VmRSS` en la tarjeta **Panel Rust**.

Importante: durante un despliegue, el cgroup del servicio también ejecuta temporalmente Docker CLI. `MemoryCurrent` de systemd puede subir aunque el proceso Rust siga pequeño. Docker Engine y los contenedores administrados no forman parte del RSS del panel.

Los valores predeterminados ya están ajustados para un servidor pequeño:

```dotenv
TDM_WORKERS=2
TDM_MAX_HISTORY=200
```

No se fija `MemoryMax=100M` en systemd porque podría matar el Docker CLI durante un pull. Primero mide y luego añade un límite solo si tus pruebas lo soportan.

## Seguridad y límites de confianza

El usuario del servicio pertenece al grupo `docker`. En Linux, controlar Docker equivale en la práctica a controlar el host. Por eso:

- Instálalo solo en servidores tuyos y para administradores de confianza.
- No es una plataforma multi-tenant.
- Protege el token y rota `/etc/tinkiva-docker-manager/env` si se filtra.
- Usa HTTPS para cualquier acceso remoto.
- Mantén `TDM_ALLOWED_ROOT` dedicado únicamente a stacks administrados.
- No permitas que usuarios no confiables editen los archivos Compose.
- No introduzcas texto no confiable en nombres de imágenes, ramas o variables.
- Limita el acceso al panel con firewall, VPN o una red privada cuando sea posible.
- Haz backups de volúmenes y bases de datos por separado; el historial del panel no es un backup.

Consulta también [SECURITY.md](SECURITY.md).

## Actualizar

El binario incluye un actualizador que descarga la última release de GitHub, verifica su suma sha256 y se reemplaza a sí mismo (requiere `curl` y `sha256sum`):

```bash
sudo tmanager update        # última versión
sudo tmanager update v0.1.2 # versión concreta
sudo systemctl restart tinkiva-docker-manager   # solo si usas systemd
```

El repositorio de origen se puede cambiar con `TDM_UPDATE_REPO` (predeterminado: `isaul19/tinkiva-docker-manager`).

## Desinstalar

El propio binario desinstala cualquiera de las dos modalidades; detecta si hay servicio systemd y actúa en consecuencia:

```bash
sudo tmanager uninstall           # servicio, binario y docs; conserva config, datos e historial
sudo tmanager uninstall --purge   # elimina también config, datos, historial y el usuario del sistema
tmanager uninstall --yes          # sin confirmación (scripts y CI)
```

Antes de tocar nada imprime la lista exacta de rutas a eliminar y las que conserva, y pide confirmación (`--yes` la omite). Sin terminal interactiva y sin `--yes` aborta sin borrar nada. Si la instalación es del sistema y no eres root, avisa para repetir con `sudo`.

Qué hace cada modo:

| | Por omisión | `--purge` |
|---|---|---|
| Servicio systemd, binario, documentación | se eliminan | se eliminan |
| `tinkiva.pid`, `tinkiva.log` | se eliminan | se eliminan |
| Config (`tinkiva.env` o `/etc/tinkiva-docker-manager`) | se conserva | se elimina |
| Datos e historial (`TDM_DATA_DIR`, `/var/lib/tinkiva-docker-manager`) | se conservan | se eliminan |
| Usuario `tinkiva-docker` | se conserva | se elimina |
| Apps Compose (`TDM_ALLOWED_ROOT`, `/opt/tinkiva/apps`) | **se conservan** | **se conservan** |

Las apps nunca se borran, ni siquiera con `--purge`, aunque vivan dentro del directorio de estado (`tinkiva-docker-manager/apps`): en ese caso el directorio se vacía entrada por entrada y la carpeta `apps` queda intacta. Los contenedores y volúmenes ya desplegados tampoco se tocan; bájalos con `docker compose down` desde cada proyecto si también los quieres fuera. Un binario compilado por cargo (`target/release/tmanager`) no se elimina a sí mismo.

Alternativa equivalente para instalaciones con systemd desde el repositorio: `sudo ./scripts/uninstall.sh [--purge]`.

## Estructura

```text
src/
  main.rs                arranque, listener y pool de workers
  app.rs                 enrutado HTTP y reglas de negocio
  http.rs                parser de peticiones y respuestas
  model.rs / store.rs    dominio y estado persistente (formato TDM3)
  docker.rs / git.rs     integración con los CLI externos
  proc.rs                lanzador de subprocesos con timeout
  net.rs                 cliente HTTPS sobre curl con lista blanca
  json.rs / crypto.rs    parser JSON y SHA-256/HMAC/Base64URL
  github.rs              GitHub App: manifiesto, JWT, repos y polling
  registry.rs            búsqueda y etiquetas de Docker Hub
  templates.rs           generadores de Compose por tipo de recurso
  metrics.rs             métricas del host desde /proc y df
  setup.rs / daemon.rs   asistente, autoactualización y modo demonio
web/
  index.html             cascarón servido por el binario
  src/                   interfaz Preact (vistas, componentes y estilos)
  dist/                  bundle compilado y versionado (lo embebe cargo)
  build.mjs              empaquetado con esbuild
deploy/                  systemd y ejemplo Nginx
examples/                Compose y GitHub Actions
scripts/                 build, instalación, smoke test y medición
tests/mock-docker.sh     Docker simulado para pruebas
.github/workflows/ci.yml CI del propio proyecto
VALIDATION.md            comprobaciones ejecutadas y límite del entorno generador
```

### Trabajar en la interfaz

```bash
cd web
npm install
npm run build     # genera web/dist/app.js y web/dist/app.css
npm run watch     # reconstruye al guardar, sin minificar
```

El resultado se commitea a propósito: así `cargo build` y el CI no necesitan Node. Si
tocas `web/src/`, recuerda ejecutar `npm run build` antes de commitear.

## Estado del proyecto

Versión `0.2.0`. El alcance sigue intencionalmente congelado en un solo host y un solo administrador. Antes de usarlo con datos críticos, prueba deploy, rollback, reinicio del host y restauración de backups en una EC2 de staging.
