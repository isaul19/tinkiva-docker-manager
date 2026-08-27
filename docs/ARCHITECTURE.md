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

El estado vive en SQLite, de forma predeterminada en
`<TDM_DATA_DIR>/tinkiva.sqlite3`. El binario enlaza SQLite estáticamente: no necesita instalar
un servidor ni una biblioteca del sistema. La conexión usa transacciones, `journal_mode=WAL`,
`synchronous=FULL`, espera acotada ante bloqueos y esquema versionado mediante `user_version`.
El archivo se protege con permisos `0600`.

`projects` mantiene unicidad tanto por slug como por ruta Compose. `deployments` usa ids
autoincrementales e índice por proyecto; la retención `TDM_MAX_HISTORY` se aplica dentro de la
misma transacción que inserta un despliegue.

Las versiones anteriores usaban `state.db` en formato textual TDM3. No se importa
automáticamente: el archivo antiguo queda intacto y SQLite usa `tinkiva.sqlite3`. Si se apunta
`TDM_SQLITE_PATH` por error al archivo TDM, el arranque se detiene antes de sobrescribirlo.

Las credenciales de la GitHub App viven aparte, en `<TDM_DATA_DIR>/github.json` con
permisos `0600`. El endpoint de estado nunca las devuelve: hay una prueba que comprueba que
ni la clave privada ni los secretos aparecen en la respuesta.

Las credenciales del panel viven en `<TDM_DATA_DIR>/auth.conf`, también con permisos `0600`.
La contraseña se deriva con Argon2 y una sal aleatoria; el navegador conserva solamente un
token de sesión opaco. El primer acceso debe reemplazar la contraseña inicial. Los fallos se
limitan por IP: un minuto después de cada fallo y un día después del tercero consecutivo. Los
bloqueos sobreviven reinicios en `<TDM_DATA_DIR>/auth.attempts.conf`.

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
   toma el commit resultante. Si el proyecto usa detección automática, restaura el
   `.tinkiva.Dockerfile` generado dentro del contexto antes del build.
3. Obtiene imagen anterior.
4. Actualiza `image_env` de forma atómica.
5. Ejecuta `docker compose pull --quiet` y `docker compose up -d --remove-orphans`; en los
   proyectos de repositorio se sustituye por un único `up -d --build`, porque no hay nada
   que descargar de un registro.
6. Actualiza estado e historial.
7. Ante fallo, restaura el `.env` y ejecuta Compose con la imagen anterior.

La respuesta HTTP es síncrona. GitHub Actions recibe el resultado real del deploy y puede fallar el job.

## Polling y webhook propio

Un único watcher secuencial consulta GitHub y los registries en el intervalo configurado.
Para repositorios compara el SHA remoto con la última revisión desplegada. Para imágenes
ejecuta un pull y compara el digest con la última revisión aplicada. No hay webhook de
GitHub ni puerto público obligatorio.

Se conserva `/hooks/deploy/:slug` como integración opcional, con un token por proyecto
comparado en tiempo constante.

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
