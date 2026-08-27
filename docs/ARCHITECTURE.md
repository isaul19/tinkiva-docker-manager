# Arquitectura de la edición TinkivaCreateApp

## Alcance

El proceso es un servidor HTTP de Rust con un pool fijo de workers. Sirve un bundle Preact
embebido y consulta el host bajo demanda. No mantiene proyectos ni un historial de despliegues.

```text
TinkivaCreateApp / sistema externo
        │ despliega
        ▼
   Docker Engine
        ▲
        │ version, ps, stats, logs (solo lectura)
        │
TinkivaCreateApp Monitor ── HTTP ── navegador autenticado
```

## Backend

- `src/app.rs`: configuración, autenticación y las cuatro rutas de observación.
- `src/docker.rs`: `docker version`, `docker ps -a`, `docker stats --no-stream` y `docker logs`.
- `src/metrics.rs`: `/proc`, `/etc/hostname` y `df` para métricas del host.
- `src/auth.rs`: hash Argon2, sesiones opacas y bloqueo persistente de intentos.
- `src/http.rs`: servidor HTTP sin framework.
- `src/daemon.rs` y `src/setup.rs`: ciclo de vida, configuración y actualización por edición.

No hay watcher, polling de registros, webhook, cliente GitHub/ECR, SQLite, Git ni Compose.

## API

La superficie autenticada es deliberadamente pequeña:

- `GET /api/info`
- `GET /api/system`
- `GET /api/containers`
- `GET /api/containers/:name/logs`

Todo método o ruta adicional responde `404`. Los identificadores de contenedor se validan antes
de pasarlos como argumentos separados al proceso Docker.

## Estado

El único estado persistente es la autenticación en `TDM_DATA_DIR`:

- `auth.conf`: usuario, hash Argon2 y obligación de cambiar la contraseña inicial.
- `auth.attempts.conf`: bloqueos por intentos fallidos.

Las sesiones viven en memoria y expiran después de doce horas.

## Actualizaciones

Los releases de esta edición usan `createapp-vMAJOR.MINOR.PATCH`. El actualizador consulta las
releases del repositorio y descarta cualquier tag que no empiece por `createapp-v`, evitando saltos
entre líneas de producto.
