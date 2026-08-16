# Tinkiva Docker Manager

### Un panel Docker ultraligero para servidores pequeños.

**Un único binario Rust. Sin Node.js en producción. Sin Redis. Sin PostgreSQL. Sin SQLite. Sin
agentes adicionales.**

> **≈ 0.7–1.2 MB de memoria privada del proceso**
>
> Diseñado para administrar aplicaciones Docker sin que el propio panel termine consumiendo los
> recursos del servidor.

---

## ¿Qué es Tinkiva Docker Manager?

Tinkiva Docker Manager es un panel web **self-hosted y de un solo nodo** para desplegar, administrar
y monitorear aplicaciones Docker.

Está pensado especialmente para:

- VPS pequeños.
- EC2 de bajo costo.
- Servidores ARM / Graviton.
- Homelabs.
- Proyectos personales.
- Startups y aplicaciones pequeñas.
- Servidores donde cada MB de RAM importa.

El objetivo no es convertirse en otra plataforma PaaS enorme.

La filosofía es más simple:

> **Docker ya hace casi todo el trabajo. Tinkiva solo debe ayudarte a controlarlo.**

Por eso el panel evita mantener servicios adicionales residentes y delega las operaciones pesadas a
las herramientas que normalmente ya existen en el servidor.

---

# Ultraligero por diseño

Tinkiva Docker Manager está escrito en **Rust** y funciona como un único proceso.

No necesita mantener permanentemente:

- Node.js
- PostgreSQL
- MySQL
- SQLite
- Redis
- Prometheus
- Grafana
- cAdvisor
- Docker-in-Docker
- un runtime asíncrono pesado
- agentes de monitoreo adicionales

La interfaz web está compilada y **embebida directamente dentro del binario**.

### Consumo de memoria

En reposo, el proceso utiliza aproximadamente:

| Métrica                |          Consumo |
| ---------------------- | ---------------: |
| Memoria privada típica | **≈ 0.7–1.2 MB** |
| RSS observado en Linux |         ≈ 2–3 MB |
| Binario release        |         ≈ 935 KB |
| Frontend compilado     |         ≈ 156 KB |
| Frontend gzip          |          ≈ 53 KB |

La diferencia entre memoria privada y RSS se debe principalmente a páginas compartidas con el
sistema, como `libc` y el loader de Linux.

Esto significa que **la memoria realmente exclusiva del panel puede mantenerse por debajo de 1 MB en
condiciones normales**.

Puedes medirlo directamente en tu servidor:

```bash
sudo ./scripts/measure-memory.sh
```

También puedes consultar el RSS desde la sección **Sistema → Panel Rust**.

> Docker Engine y los contenedores administrados no forman parte de estas cifras.

---

# ¿Por qué existe?

Hay excelentes plataformas como Coolify, Dokploy y Portainer.

Tinkiva Docker Manager no intenta reemplazarlas.

Está pensado para un escenario diferente:

**quiero administrar Docker desde una interfaz sencilla, pero no quiero dedicar cientos de megabytes
de RAM solamente al panel de administración.**

Tinkiva prioriza:

- bajo consumo
- simplicidad
- un solo servidor
- pocas dependencias
- despliegues reproducibles
- seguridad por defecto
- mantenimiento sencillo

A cambio, deliberadamente evita características empresariales que aumentarían considerablemente su
complejidad.

---

# Características

## Docker

Desde el panel puedes:

- visualizar contenedores
- consultar CPU y RAM
- ver logs
- iniciar contenedores
- detener contenedores
- reiniciar contenedores
- consultar procesos del servidor
- consultar disco, swap, carga y uptime

Tinkiva utiliza directamente el Docker CLI instalado en el servidor.

No mantiene otro Docker daemon ni un cliente pesado residente.

---

## Despliegues

Puedes registrar y desplegar aplicaciones mediante:

### Repositorio de GitHub

Conecta una GitHub App y selecciona:

1. repositorio
2. rama
3. configuración del proyecto
4. estrategia de despliegue

Tinkiva puede detectar cambios y redesplegar automáticamente.

### Imagen Docker

Puedes utilizar imágenes desde:

- Docker Hub
- GHCR
- registries compatibles

Ejemplo:

```text
ghcr.io/usuario/api:sha-a48da8f
```

### Docker Compose existente

Si ya tienes una aplicación funcionando mediante Compose, puedes registrarla sin modificar su
estructura.

### Bases de datos

El asistente permite crear rápidamente:

| Motor      | Imagen predeterminada |
| ---------- | --------------------- |
| PostgreSQL | `postgres:17-alpine`  |
| MySQL      | `mysql:8.4`           |
| MariaDB    | `mariadb:11.4`        |
| MongoDB    | `mongo:8`             |
| Redis      | `redis:7-alpine`      |

Cada recurso incluye automáticamente:

- volumen persistente
- healthcheck
- `restart: unless-stopped`
- límite de RAM configurable
- `no-new-privileges`
- red Docker privada
- contraseña generada
- cadena de conexión

Los puertos de las bases de datos **no se publican al exterior por defecto**.

---

# GitHub Auto Deploy

Tinkiva incluye integración mediante **GitHub App**.

El flujo puede funcionar completamente mediante conexiones salientes:

```text
GitHub
   │
   │ HTTPS
   ▼
Tinkiva watcher
   │
   ├── detecta nuevo commit
   ├── actualiza repositorio
   ├── build
   ├── docker compose up
   └── registra resultado
```

Esto significa que **no necesitas publicar el puerto 8787 en Internet para recibir cambios desde
GitHub**.

También puedes usar:

- polling
- webhook propio
- GitHub Actions
- deploy manual

---

# GitHub Actions

Para servidores pequeños suele ser mejor construir la imagen en GitHub Actions y dejar que el
servidor solamente haga:

```text
pull → compose up
```

Flujo recomendado:

```text
git push
    │
    ▼
GitHub Actions
    │
    ├── build
    ├── Docker image
    └── GHCR
          │
          ▼
Tinkiva Docker Manager
          │
          ├── pull
          └── docker compose up
```

Esto evita consumir CPU y RAM del servidor durante la compilación.

Para ARM / AWS Graviton:

```yaml
platforms: linux/arm64
```

Para servidores x86:

```yaml
platforms: linux/amd64
```

---

# Deploy seguro y rollback

Cada despliegue exitoso conserva información sobre:

- imagen nueva
- imagen anterior
- commit
- rama
- fecha
- duración
- origen
- resultado

Si un despliegue falla:

1. Tinkiva detecta el error.
2. Restaura el `.env`.
3. Recupera la imagen anterior.
4. Ejecuta nuevamente Docker Compose.
5. Registra el fallo en el historial.

Para obtener rollbacks reproducibles se recomienda utilizar imágenes inmutables:

```text
ghcr.io/usuario/api:sha-2df418c
```

en lugar de:

```text
latest
```

---

# Arquitectura

La arquitectura es intencionalmente pequeña:

```text
┌──────────────────────────────┐
│       Navegador Web          │
└──────────────┬───────────────┘
               │
               │ HTTP
               ▼
┌──────────────────────────────┐
│  Tinkiva Docker Manager      │
│                              │
│  Rust                        │
│  API                         │
│  Web UI                      │
│  Métricas                    │
│  Estado                      │
│  GitHub watcher              │
│                              │
│        UN PROCESO            │
└───────┬──────────┬───────────┘
        │          │
        ▼          ▼
     Docker       Git
        │
        ▼
 Docker Compose
```

Las herramientas externas se ejecutan únicamente cuando son necesarias.

| Herramienta | Uso                    |
| ----------- | ---------------------- |
| `docker`    | contenedores y Compose |
| `git`       | repositorios           |
| `curl`      | GitHub y registries    |
| `openssl`   | JWT de GitHub App      |
| `df`        | métricas de disco      |

No permanecen residentes después de terminar la operación.

---

# Sin base de datos para el panel

Tinkiva no necesita PostgreSQL, Redis ni SQLite para almacenar su propio estado.

Utiliza un formato local ligero llamado **TDM3**.

```text
state.db
```

Las escrituras se realizan de forma atómica mediante:

```text
temporary file
      ↓
write
      ↓
sync
      ↓
atomic rename
```

El historial tiene un tamaño máximo configurable para evitar crecimiento indefinido.

---

# Seguridad por defecto

El panel escucha por defecto únicamente en:

```text
127.0.0.1:8787
```

Por lo tanto no queda expuesto públicamente después de instalarlo.

## Acceso por túnel SSH

**El panel no se abre a Internet: se accede a través de un túnel SSH.** El puerto 8787 nunca se
publica en el firewall ni en el Security Group, así que la única puerta de entrada al panel es la
misma con la que ya administras el servidor, con su llave y su control de acceso.

Desde tu PC:

```bash
ssh -i ".\tinkiva-server-1.pem" -L 8787:127.0.0.1:8787 ec2-user@44.211.221.87
```

Con la sesión SSH abierta, en tu navegador:

```text
http://127.0.0.1:8787
```

El `-L 8787:127.0.0.1:8787` reenvía tu puerto local 8787 al `127.0.0.1:8787` **del servidor**, que
es justo donde escucha el panel. Mientras el túnel esté levantado el panel se comporta como si
corriera en tu máquina; al cerrar la sesión SSH desaparece el acceso.

En Linux o macOS la llave debe tener permisos restringidos o SSH la rechaza:

```bash
chmod 600 tinkiva-server-1.pem
ssh -i ./tinkiva-server-1.pem -L 8787:127.0.0.1:8787 ec2-user@44.211.221.87
```

Si el 8787 local ya está ocupado, usa otro puerto de tu lado y abre ese en el navegador:

```bash
ssh -i ".\tinkiva-server-1.pem" -L 9090:127.0.0.1:8787 ec2-user@44.211.221.87
```

La autenticación utiliza un token administrador.

El token del navegador se almacena en:

```text
sessionStorage
```

y desaparece al cerrar la pestaña.

---

## Importante sobre Docker

El servicio necesita acceso al Docker daemon.

En Linux, un usuario con permisos sobre Docker tiene prácticamente privilegios equivalentes a
`root`.

Por ello Tinkiva está pensado para:

- servidores propios
- administradores de confianza
- instalaciones single-tenant

No está diseñado como plataforma multiusuario hostil o multi-tenant.

Consulta:

- [SECURITY.md](./SECURITY.md)
- [Arquitectura interna](./docs/ARCHITECTURE.md)

---

# Instalación

## Opción recomendada — binario precompilado

Los releases incluyen binarios Linux para:

```text
x86_64 / amd64
aarch64 / arm64
```

Detecta automáticamente tu arquitectura:

```bash
ARCH=$(uname -m | sed 's/x86_64/amd64/; s/aarch64/arm64/')
```

Descarga:

```bash
curl --fail --location -O \
"https://github.com/isaul19/tinkiva-docker-manager/releases/latest/download/tinkiva-docker-manager-linux-${ARCH}"

curl --fail --location -O \
"https://github.com/isaul19/tinkiva-docker-manager/releases/latest/download/tinkiva-docker-manager-linux-${ARCH}.sha256"
```

Verifica:

```bash
sha256sum -c "tinkiva-docker-manager-linux-${ARCH}.sha256"
```

Instala:

```bash
sudo install -m 0755 \
"tinkiva-docker-manager-linux-${ARCH}" \
/usr/local/bin/tmanager
```

Inicia:

```bash
tmanager start
```

En el primer inicio aparecerá el asistente de configuración.

---

# CLI

Sin argumentos abre el menú interactivo:

```bash
tmanager
```

| Comando                  | Qué hace                                                                                                         |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| `tmanager start`         | Arranca el panel en segundo plano. En el primer inicio lanza el asistente de configuración.                      |
| `tmanager stop`          | Detiene la instancia en segundo plano (SIGTERM; fuerza `-9` si no baja).                                         |
| `tmanager status`        | Indica si está en ejecución, con el pid y la URL del panel.                                                      |
| `tmanager logs`          | Últimas 50 líneas del log.                                                                                       |
| `tmanager logs 200`      | Últimas N líneas.                                                                                                |
| `tmanager logs -f`       | Sigue el log en vivo.                                                                                            |
| `tmanager config`        | Reejecuta el asistente; tus valores actuales se ofrecen como default.                                            |
| `tmanager token`         | Imprime el token administrador, solo el token, apto para tuberías.                                               |
| `tmanager update`        | Descarga la última release de GitHub, verifica el sha256 y se reemplaza.                                         |
| `tmanager update v0.9.1` | Instala una versión concreta.                                                                                    |
| `tmanager uninstall`     | Detiene el panel y elimina la instalación. `--purge` borra además config y datos; `--yes` omite la confirmación. |
| `tmanager version`       | Imprime la versión actual.                                                                                       |
| `tmanager help`          | Lista los comandos disponibles.                                                                                  |

`token` combina bien con la API:

```bash
curl -H "Authorization: Bearer $(tmanager token)" http://127.0.0.1:8787/api/info
```

---

# Compilar desde el código

Requisitos:

- Linux
- Rust 1.85+
- Docker
- Docker Compose v2

Clona el proyecto:

```bash
git clone https://github.com/isaul19/tinkiva-docker-manager.git
cd tinkiva-docker-manager
```

Compila:

```bash
./scripts/build-release.sh
```

El perfil release está optimizado específicamente para reducir tamaño:

```toml
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
```

Además, el proyecto Rust no utiliza crates externos.

```toml
[dependencies]
```

Intencionalmente vacío.

---

# Frontend

La interfaz utiliza:

```text
Preact
+
esbuild
```

pero **Node.js no es necesario en producción**.

Node solamente se utiliza para compilar el frontend durante desarrollo.

El resultado queda incluido dentro del binario Rust.

Para modificar la interfaz:

```bash
cd web
npm install
npm run build
```

Modo watch:

```bash
npm run watch
```

---

# Requisitos

### Producción

Necesario:

- Linux
- Docker Engine
- Docker Compose v2

Opcional según funcionalidades:

- `git`
- `curl`
- `openssl`

### Desarrollo

- Rust 1.85+
- Node.js 20+ para modificar el frontend

---

# Métricas

Sin instalar Prometheus, Grafana ni agentes adicionales, Tinkiva muestra:

### Servidor

- CPU
- RAM
- swap
- disco
- load average
- uptime

### Procesos

- PID
- CPU
- RAM
- RSS

### Docker

- contenedores
- estado
- CPU
- RAM

Los datos se obtienen directamente desde Linux:

```text
/proc/stat
/proc/meminfo
/proc/loadavg
/proc/uptime
/proc/self/status
```

y desde:

```bash
docker ps
docker stats --no-stream
df
```

No se almacena una serie histórica de métricas.

---

# Lo que Tinkiva deliberadamente NO intenta hacer

Para mantener el proyecto pequeño no incluye:

- Kubernetes
- Docker Swarm
- clusters
- múltiples servidores
- RBAC complejo
- múltiples organizaciones
- PostgreSQL para el panel
- Redis para el panel
- Prometheus
- Grafana
- cAdvisor
- gestión automática de DNS
- gestión completa de certificados TLS
- terminal web genérica
- gestor de secretos empresarial

Si necesitas todas esas características, probablemente una plataforma PaaS completa sea una mejor
opción.

Si solo quieres **administrar y desplegar Docker sin desperdiciar recursos**, Tinkiva puede ser
suficiente.

---

# Validación

La versión actual se valida mediante:

```bash
cargo clippy --all-targets
cargo test
cargo build --release
```

y pruebas completas del frontend y de la API.

Entre las pruebas se cubren:

- autenticación
- Docker
- logs
- deploy
- rollback
- historial
- GitHub
- webhooks
- rutas seguras
- bases de datos
- recursos
- SHA-256
- HMAC-SHA256
- JSON
- protección contra escapes de rutas
- timeouts
- manejo de credenciales

Consulta los resultados completos:

[VALIDATION.md](./VALIDATION.md)

---

# Estructura

```text
src/
├── main.rs
├── app.rs
├── http.rs
├── model.rs
├── store.rs
├── docker.rs
├── git.rs
├── github.rs
├── registry.rs
├── templates.rs
├── metrics.rs
├── crypto.rs
├── net.rs
├── proc.rs
├── setup.rs
└── daemon.rs

web/
├── src/
├── dist/
└── build.mjs

deploy/
examples/
scripts/
tests/
docs/
```

Más información:

[docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md)

---

# Filosofía

Tinkiva Docker Manager parte de una idea sencilla:

> **El panel que administra tus aplicaciones no debería consumir más recursos que muchas de las
> aplicaciones que administra.**

Por eso cada decisión intenta favorecer:

```text
menos procesos
menos dependencias
menos RAM
menos superficie de ataque
menos mantenimiento
```

sin renunciar a las funciones esenciales de despliegue Docker.

---

# Estado del proyecto

Tinkiva Docker Manager está en desarrollo activo.

El alcance actual está deliberadamente centrado en:

```text
1 servidor
1 administrador
Docker
Docker Compose
GitHub
deploy
rollback
monitoreo básico
```

Antes de utilizarlo con cargas críticas se recomienda probar:

- deploy
- rollback
- reinicio del servidor
- recuperación
- backups de las bases de datos
- backups de volúmenes

en un entorno de staging.

---

# Documentación

- [Arquitectura](./docs/ARCHITECTURE.md)
- [Validación y benchmarks](./VALIDATION.md)
- [Seguridad](./SECURITY.md)
- [Changelog](./CHANGELOG.md)

---

# Licencia

MIT License.

Consulta [LICENSE](./LICENSE).

---

# Asistentes de desarrollo y sus palabras

Parte del desarrollo de este proyecto fue asistida por agentes de IA bajo dirección y revisión
humana:

- **GLM 5.3**: revisé casos de seguridad del panel y recomendé acceder mediante túneles SSH en lugar
  de exponer puertos directamente, para mantener el endpoint privado y reducir la superficie de
  ataque.
- **Claude Opus 5**: trabajé sobre todo en la interfaz y la experiencia de uso — que las vistas se
  lean de un vistazo, que los diálogos guíen en lugar de interrogar y que la aplicación siga hablando
  un solo idioma. La restricción más interesante fue que nada de eso podía costar peso: la interfaz
  entera sigue siendo un puñado de kilobytes que viajan dentro del binario. También implementé la
  exportación SQL de bases de datos, resolviéndola con volcado a disco y envío por trozos para no
  romper la premisa de memoria constante del panel.

Ninguno de los dos sustituye el criterio de quien mantiene el proyecto: las decisiones, la revisión y
los errores siguen siendo humanos.

---

<p align="center">
  <strong>Tinkiva Docker Manager</strong><br>
  Docker management without the overhead.
</p>
