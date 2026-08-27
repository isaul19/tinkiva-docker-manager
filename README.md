# Tinkiva Docker Manager — TinkivaCreateApp Monitor

Edición estable y de solo lectura para observar las aplicaciones que TinkivaCreateApp u otro
sistema despliega en un servidor Docker.

Esta línea no incluye CI/CD. No clona repositorios, no descarga imágenes, no ejecuta Compose,
no recibe webhooks y no puede arrancar, detener, reiniciar ni borrar contenedores. Su trabajo es
mostrar qué está ocurriendo en el host.

## Qué muestra

- CPU, RAM, swap, disco, carga y uptime del servidor.
- Contenedores Docker activos y detenidos.
- Imagen, estado, puertos, CPU, memoria, red, disco y procesos de cada contenedor.
- Logs recientes con actualización manual o seguimiento cada cuatro segundos.
- Estado del acceso al daemon Docker y consumo de memoria del propio panel.

Los contenedores aparecen automáticamente. TinkivaCreateApp no necesita registrar proyectos en
el panel: basta con desplegarlos en el mismo daemon Docker.

## Líneas de producto

| Línea | Tags | Propósito |
|---|---|---|
| Manager clásico | `v0.14.x` | Despliegue sencillo por SSH y administración de apps |
| Panel preview | `v0.15.0-panel.x` | Panel general con persistencia SQLite |
| TinkivaCreateApp | `createapp-v0.15.x` | Observabilidad estable, externa y de solo lectura |

`tmanager update` en esta edición solo busca tags `createapp-v*`; nunca cambia a la edición clásica ni al panel preview.

## Requisitos

- Linux x86_64 o ARM64.
- Docker Engine accesible para el usuario del servicio.
- `df`, usado para medir el disco raíz.
- `curl` y `sha256sum`, únicamente para `tmanager update`.

Docker Compose, Git, GitHub y credenciales de registros no son necesarios.

## Instalación

Descarga el binario correspondiente desde la release `createapp-v0.15.0`:

```bash
chmod +x tinkiva-docker-manager-linux-amd64
sudo ./scripts/install.sh ./tinkiva-docker-manager-linux-amd64
```

El instalador crea el usuario `tinkiva-docker`, le concede acceso al grupo `docker`, instala el
servicio systemd y genera las credenciales iniciales.

Acceso recomendado mediante túnel SSH:

```bash
ssh -L 8787:127.0.0.1:8787 usuario@servidor
```

Después abre `http://127.0.0.1:8787`.

## Configuración

La instalación del sistema guarda `/etc/tinkiva-docker-manager/env` con permisos `0600`:

```dotenv
TDM_EDITION=createapp
TDM_BIND=127.0.0.1:8787
TDM_ADMIN_TOKEN=un-token-aleatorio-de-al-menos-32-caracteres
TDM_ADMIN_USER=admin
TDM_ADMIN_PASSWORD=contraseña-inicial
TDM_DATA_DIR=/var/lib/tinkiva-docker-manager
TDM_DOCKER_BIN=/usr/bin/docker
TDM_WORKERS=2
```

El primer acceso obliga a cambiar la contraseña. El token administrador sigue disponible para
consultas automatizadas de la API.

## API de solo lectura

Salvo `/healthz` y el inicio de sesión, los endpoints requieren `Authorization: Bearer TOKEN`.

```text
GET  /healthz
POST /api/auth/login
POST /api/auth/change-password
GET  /api/info
GET  /api/system
GET  /api/containers
GET  /api/containers/:nombre/logs?tail=300
```

No existen endpoints de despliegue ni endpoints de escritura sobre Docker.

## Operación

```bash
tmanager start
tmanager stop
tmanager status
tmanager logs 100
tmanager logs -f
tmanager config
tmanager update
tmanager uninstall
```

Para actualizar a una versión específica de esta misma edición:

```bash
tmanager update createapp-v0.15.1
```

## Desarrollo y validación

```bash
cd web
npm ci
npm test

cd ..
cargo fmt --check
cargo clippy --all-targets
cargo test
cargo build --release
./scripts/smoke-test.sh target/release/tmanager
```

El backend solo soporta Linux porque lee `/proc`, ejecuta `df` y consulta Docker.

## Seguridad

Aunque la aplicación solo ejecuta comandos Docker de lectura, pertenecer al grupo `docker`
equivale técnicamente a tener privilegios elevados sobre el host. Mantén el panel ligado a
localhost o detrás de HTTPS y autenticación adicional. Consulta [SECURITY.md](SECURITY.md).

## Licencia

MIT.
