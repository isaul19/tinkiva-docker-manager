# Arquitectura interna

## Proceso permanente

Un único proceso contiene:

- Listener TCP HTTP/1.1.
- Pool fijo de workers con stack de 512 KiB.
- Router y autenticación.
- Archivos web incluidos en el binario mediante `include_str!`.
- Store en memoria protegido por `Mutex`.
- Bloqueo global de deployment.

No se usa runtime asíncrono. Para el nivel de concurrencia esperado en un panel administrativo, los threads fijos evitan incorporar Tokio y mantienen predecible la memoria.

## Persistencia

`state.db` es un formato de líneas `TDM1`, no una base de datos. Cada campo textual se codifica por porcentaje, y cada modificación reescribe el archivo mediante:

1. Archivo temporal `0600` en el mismo directorio.
2. `write_all`.
3. `sync_all`.
4. `rename` atómico.
5. Sincronización del directorio cuando es posible.

El historial está acotado por `TDM_MAX_HISTORY`.

## Docker

El proceso invoca Docker CLI con argumentos separados, nunca mediante `sh -c`. Las entradas se validan antes de convertirse en argumentos. stdout y stderr se redirigen a archivos temporales para evitar deadlocks por pipes llenos y se eliminan al terminar.

Timeouts:

- Información: 10 s.
- Listado/logs: 15–20 s.
- Acciones de contenedor: 45 s.
- Pull/up: 300 s cada uno.

## Deployment

El deployment global es exclusivo:

1. Verifica rama.
2. Obtiene imagen anterior.
3. Actualiza `image_env` de forma atómica.
4. Ejecuta `docker compose pull --quiet`.
5. Ejecuta `docker compose up -d --remove-orphans`.
6. Actualiza estado e historial.
7. Ante fallo, restaura el `.env` y ejecuta Compose con la imagen anterior.

La respuesta HTTP es síncrona. GitHub Actions recibe el resultado real del deploy y puede fallar el job.

## Métricas

- CPU: dos muestras de `/proc/stat` separadas por 150 ms.
- RAM/swap: `/proc/meminfo`.
- Load average: `/proc/loadavg`.
- Uptime: `/proc/uptime`.
- RSS: `/proc/self/status`.
- Disco: `df -B1`.
- Contenedores: `docker ps` y `docker stats --no-stream`.

No se conserva una serie temporal.
