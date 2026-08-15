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

Los archivos web se sirven **prestados** desde la sección de solo lectura del binario
(`Cow::Borrowed`). El bundle Preact pesa unos 115 KB; copiarlo a un `Vec` en cada petición
haría que el consumo creciera con el tráfico, mientras que prestarlo lo deja constante.

## Interfaz

La interfaz es Preact compilada con esbuild a `web/dist/{app.js,app.css}`. El bundle está
versionado en el repositorio, de modo que `cargo build` nunca necesita Node; el CI
reconstruye y falla si el resultado difiere de lo commiteado.

No hay runtime de framework en el servidor: el binario solo devuelve dos archivos
estáticos y JSON. La política de seguridad de contenido sigue siendo `connect-src 'self'`,
así que el navegador nunca habla directamente con Docker Hub ni con GitHub; todo pasa por
la API del panel.

## Salidas a internet

El panel no enlaza ninguna pila TLS. Cuando necesita hablar con Docker Hub o GitHub invoca
`curl` como subproceso, con estas restricciones aplicadas antes de ejecutarlo:

- solo `https://` y solo hacia una lista blanca de cuatro hosts;
- sin seguir redirecciones, para que una respuesta no pueda sacarnos de la lista blanca;
- sin puerto explícito ni credenciales en la URL;
- cabeceras y cuerpo por stdin (formato de configuración de curl), nunca por `argv`, para
  que los tokens no aparezcan en `ps`;
- respuesta acotada en bytes antes de analizarla.

La firma RS256 de los JWT de la GitHub App se delega en `openssl`; implementar RSA a mano
habría significado escribir aritmética de precisión múltiple sin auditar. SHA-256, HMAC y
Base64URL sí están implementados en el propio crate porque son pequeños, deterministas y
verificables contra los vectores de prueba de FIPS 180-4 y RFC 4231.

## Persistencia

`state.db` es un formato de líneas `TDM2`, no una base de datos. Cada campo textual se codifica por porcentaje, y cada modificación reescribe el archivo mediante:

1. Archivo temporal `0600` en el mismo directorio.
2. `write_all`.
3. `sync_all`.
4. `rename` atómico.
5. Sincronización del directorio cuando es posible.

El historial está acotado por `TDM_MAX_HISTORY`.

El formato anterior `TDM1` se sigue leyendo: a las líneas de proyecto les faltan los cuatro
campos de tipo y origen, que se rellenan como proyecto Compose sin repositorio. El archivo
queda reescrito en `TDM2` en el primer guardado.

Las credenciales de la GitHub App viven aparte, en `<TDM_DATA_DIR>/github.json` con
permisos `0600`. El endpoint de estado nunca las devuelve: hay una prueba que comprueba que
ni la clave privada ni los secretos aparecen en la respuesta.

## Docker y git

El proceso invoca los CLI con argumentos separados, nunca mediante `sh -c`. Las entradas se
validan antes de convertirse en argumentos. stdout y stderr se redirigen a archivos
temporales para evitar deadlocks por pipes llenos y se eliminan al terminar.

El token de instalación de GitHub no viaja por `argv` ni queda en `remote.origin.url`: se
entrega a git mediante un archivo de credenciales temporal `0600` que se borra en el `Drop`
del guardián. Si un comando falla, el token se sustituye por `***` antes de que el mensaje
llegue al historial.

Timeouts:

- Información: 10 s.
- Listado/logs: 15–20 s.
- Acciones de contenedor: 45 s.
- Pull/up: 300 s cada uno.
- Build desde repositorio: 1800 s.
- Clonado: 900 s; `fetch`: 300 s.

## Deployment

El deployment global es exclusivo:

1. Verifica rama. Si no se indica ninguna se usa la configurada, salvo en webhooks, donde
   el emisor debe declararla.
2. En proyectos de repositorio, sincroniza el clon con `fetch --depth 1` + `reset --hard` y
   toma el commit resultante.
3. Obtiene imagen anterior.
4. Actualiza `image_env` de forma atómica.
5. Ejecuta `docker compose pull --quiet` y `docker compose up -d --remove-orphans`; en los
   proyectos de repositorio se sustituye por un único `up -d --build`, porque no hay nada
   que descargar de un registro.
6. Actualiza estado e historial.
7. Ante fallo, restaura el `.env` y ejecuta Compose con la imagen anterior.

La respuesta HTTP es síncrona. GitHub Actions recibe el resultado real del deploy y puede fallar el job.

## Webhooks

Hay dos entradas, ambas fuera de la autenticación Bearer:

- `/hooks/deploy/:slug`, con un token por proyecto comparado en tiempo constante.
- `/hooks/github`, validado con HMAC-SHA256 sobre el cuerpo crudo contra el secreto que
  GitHub generó al crear la App. Solo actúa sobre eventos `push`, y despliega los proyectos
  cuyo repositorio y rama coincidan.

Los retornos del navegador desde GitHub (`/github/callback`, `/github/installed`) tampoco
llevan `Authorization`, porque son navegaciones y no llamadas de la interfaz. Se validan
con un nonce de un solo uso, emitido por el panel, con caducidad de 15 minutos y comparado
en tiempo constante.

## Métricas

- CPU: dos muestras de `/proc/stat` separadas por 150 ms.
- RAM/swap: `/proc/meminfo`.
- Load average: `/proc/loadavg`.
- Uptime: `/proc/uptime`.
- RSS: `/proc/self/status`.
- Disco: `df -B1`.
- Contenedores: `docker ps` y `docker stats --no-stream`.

No se conserva una serie temporal.
