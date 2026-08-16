# Changelog

## Sin publicar

### Nuevas funcionalidades

- **`tmanager token`**: imprime el token administrador leído de `TDM_ADMIN_TOKEN` o
  del archivo de configuración. Solo el token, sin adornos, para usarlo directamente
  en `curl -H "Authorization: Bearer $(tmanager token)"`.
- **`tmanager uninstall`**: desinstala desde el propio binario, tanto la instalación
  con systemd como la local. Detiene el panel, elimina servicio, binario, documentación,
  pid y log; con `--purge` también configuración, datos, historial y el usuario
  `tinkiva-docker`. Muestra el plan y pide confirmación (`--yes` la omite). Las apps
  Compose y los contenedores desplegados nunca se borran.

### Correcciones

- El workflow de CI invocaba el smoke test con el nombre viejo del binario
  (`tinkivadm`), por lo que el servidor nunca arrancaba. El script ahora también
  falla de inmediato si el binario no existe o si el proceso muere antes de responder.

## 0.9.0 — 2026-08-15

### Cambio de nombre del binario

- El comando pasa a llamarse `tmanager` (antes `tinkivadm`). El instalador, el
  servicio systemd y los scripts de build se actualizaron; el workflow de release
  ahora empaqueta el binario `tmanager`.

### Nuevas funcionalidades

- **Consola en Contenedores**: terminal integrada en la vista de contenedores.
- **VPC externo en bases de datos**: permite conectar una base de datos a un VPC
  externo desde el formulario de creación.
- **Acciones unificadas**: los botones de acción de cada recurso se agrupan en un
  solo botón.

### Correcciones

- Paginación corregida y estados del panel localizados.
- El token administrador se muestra una sola vez al generarlo, también cuando se
  regenera con configuración existente (antes solo aparecía en la primera configuración).
- Sin desbordamiento horizontal en la salida de consola.
- Ajustes de estilos, inputs y distribución del Resumen.

## 0.8.1 — 2026-08-15

- Memoria del host: `usada` y `disponible` ahora derivan ambas de `MemAvailable`
  (`usada = total − disponible`), de modo que las cifras del Resumen siempre suman
  el total. Antes la usada se calculaba con una fórmula propia y la disponible con
  otra, y podían contradecirse.

## 0.8.0 — 2026-08-15

### Editor de variables de entorno

- Botón **Variables** en cada tarjeta de Recurso: lee el `.env` actual y permite modificar,
  añadir o eliminar variables.
- Las variables internas que gestiona Tinkiva (`APP_IMAGE`, `TDM_MEMORY_LIMIT`) quedan
  protegidas y ocultas del editor.
- El `.env` se guarda atómicamente con permisos `0600`; si el Compose queda inválido o
  Docker no puede aplicar, se restaura el archivo anterior.
- Al guardar se recrea el servicio, y los **valores de las variables nunca se escriben**
  en el historial de despliegues.

### Paginación

- Despliegues, Recursos, Contenedores y Procesos pagan de 10 en 10 con un componente
  reutilizable, sin librerías nuevas.
- Despliegues pagina desde el backend (`/api/history/page`): con cientos o miles de
  registros el navegador solo recibe la página visible.
- El filtro de Despliegues usa el mismo componente `Select` con chevron que el resto
  del panel.

### Memoria del host

- La memoria usada ya no cuenta la caché y buffers reclamables de Linux como RAM
  consumida (`total − free − buffers − reclaimable`).
- El Resumen muestra la memoria disponible junto al total.

### Correcciones

- Una actualización únicamente de variables de entorno ya no se interpreta como una
  imagen anterior disponible para rollback.
- Claves repetidas en el editor de variables se rechazan.

## 0.7.0 — 2026-08-15

### Acceso externo opcional

- Nuevo interruptor **«Permitir acceso externo al VPS»** en recursos de imagen y
  repositorio: desactivado publica en `127.0.0.1` (como siempre), activado escucha en
  `0.0.0.0` con aviso de que el firewall o Security Group debe abrirse a mano.
- Las bases de datos siguen siempre en `127.0.0.1`; exponer un PostgreSQL o Redis
  público por accidente no es un clic.
- El panel aclara que solo cambia el bind de Docker: no toca el Security Group de AWS.

### Detección de puertos

- El Dockerfile del repositorio ahora se lee buscando `EXPOSE`: si declara el puerto,
  los campos pueden quedar vacíos.
- Corregido el orden de autodetección en repositorios: el puerto del VPS por defecto se
  resuelve después de detectar el runtime; antes un flujo automático podía dejar el
  servicio como `3000/tcp` sin publicar en el host.

### Interfaz

- «Puerto local» pasa a llamarse **«Puerto del VPS»** y los selectores muestran un
  chevron propio en vez de la flecha nativa.

## 0.6.1 — 2026-08-15

- En recursos de imagen y repositorio, dejar el **puerto local vacío** publica el
  servicio en el mismo puerto que escucha el contenedor (`127.0.0.1:3000:3000`). Antes
  el servicio quedaba solo en la red interna de Docker, inalcanzable desde el host.
- Ambos puertos vacíos siguen dejando el servicio en la red interna.

## 0.6.0 — 2026-08-15

### Rollback para recursos de repositorio

- Los builds de repositorio etiquetan la imagen por commit (`tinkiva/<slug>:<sha>`) a
  través de `APP_IMAGE`: cada versión queda preservada en el Docker local, igual que en
  los recursos de imagen.
- El rollback restaura la versión anterior y aplica `compose up -d` **sin reconstruir**,
  así que es inmediato.
- Los recursos de repositorio existentes migran solos en su próximo despliegue: el
  Compose pasa a resolver la imagen desde `APP_IMAGE` y el rollback queda habilitado.
- La restauración automática tras un despliegue fallido tampoco reconstruye.

### Interfaz

- Tarjetas de recurso más anchas y nueva fila «Último despliegue» bajo «Creado», con
  indicación en rojo cuando el último despliegue falló.

## 0.5.0 — 2026-08-15

### Estado en vivo de los recursos

- La vista Recursos refresca cada 15 s y muestra una insignia agregada por recurso —
  **Corriendo**, **Apagado** o **Error · se detuvo** — calculada con `docker compose ps`
  sobre el archivo del proyecto, de modo que los stacks multisericio informan bien
  independientemente del nombre que Compose deduzca para el stack.
- El botón principal dice **Redesplegar** cuando el recurso está corriendo.

### Rollback honesto

- El rollback responde 409 cuando el recurso no usa imagen configurable por `.env`,
  cuando aún no hay imagen anterior, o cuando la anterior coincide con la actual.
- La interfaz deshabilita el botón con el motivo en lugar de descubrirlo al fallar.

### Formularios

- El slug se deriva automáticamente del nombre mientras no se edite a mano.

## 0.4.1 — 2026-08-15

- `tinkivadm update` prepara el reemplazo junto al ejecutable antes del cambio atómico;
  ya no falla con `Cross-device link` cuando `/tmp` y el binario están en volúmenes distintos.

## 0.4.0 — 2026-08-15

### Detección automática de aplicaciones

- Los repositorios sin `Dockerfile` ya se pueden desplegar: el panel detecta el tipo de
  aplicación y genera la receta. Soporta Node con npm, pnpm o Yarn; frontends Vite y
  Create React App compilados y servidos por Nginx con fallback SPA; Python con FastAPI,
  Flask, Django o un `main.py`/`app.py` convencional; y sitios estáticos con `index.html`.
- El Dockerfile generado se guarda fuera del clon (`<slug>/.tinkiva.Dockerfile`) y se
  restaura dentro del contexto después de cada sincronización, sin tocar el repositorio.
- El selector «Tipo de aplicación» permite detección automática o exigir el Dockerfile;
  el puerto del contenedor se deduce del runtime (80 estático, 3000 Node, 8000 Python).
- En monorepos, el contexto de build se valida con rutas canónicas contra la raíz del clon.

### Salud de Docker y Compose

- El alta de cualquier recurso falla pronto con 503 si Docker o Compose no responden,
  en vez de fallar a mitad de operación.
- La página Sistema y el diálogo «Añadir recurso» distinguen entre Docker caído y
  Compose ausente, cada uno con su mensaje.

## 0.3.0 — 2026-08-15

### Auto-deploy sin exponer el panel

- El redespliegue automático pasa de webhook entrante a **polling saliente**: un watcher
  consulta el SHA de la rama en la API de GitHub y el digest de la imagen en el registry,
  y solo recrea el servicio cuando cambia lo aplicado. El panel puede quedarse en
  `localhost`: no necesita dominio, TLS ni puerto público.
- El manifiesto de la GitHub App ya no pide webhook ni el evento `push`; el alta manual
  deja de pedir el secreto de webhook.
- Intervalo configurable con `TDM_POLL_INTERVAL_SECONDS` (30 s – 24 h, por defecto 60).
  `TDM_PUBLIC_URL` desaparece porque ya no hace falta.
- Las imágenes admiten referencias exactas de otros registries (`ghcr.io/owner/repo:tag`),
  con Auto Deploy opcional comparando el digest.

### Interno

- Formato de estado `TDM3` con `auto_deploy` y revisión aplicada; `TDM1` y `TDM2` se
  siguen leyendo y se migran al primer guardado.
- 44 pruebas unitarias y smoke test actualizados al nuevo flujo.

## 0.2.0 — 2026-08-15

### Alta de recursos

- Diálogo «Añadir recurso» con cuatro orígenes: base de datos, imagen de Docker Hub,
  repositorio de GitHub y Compose ya existente.
- Cinco motores de base de datos en lugar de solo PostgreSQL: **PostgreSQL, MySQL,
  MariaDB, MongoDB y Redis**. Cada plantilla genera Compose con volumen persistente,
  healthcheck, `mem_limit`, `no-new-privileges` y red interna; los puertos solo se
  publican si se piden, y siempre contra `127.0.0.1`.
- Autocompletado de Docker Hub: búsqueda de imágenes y listado de etiquetas, con
  sugerencias populares mientras no se escribe nada.
- Servicios desde una imagen suelta: el panel escribe el Compose y deja la imagen en
  `APP_IMAGE`, de modo que el rollback funciona igual que en los proyectos Compose.

### GitHub

- Integración con GitHub App mediante el flujo de manifiesto de un clic: el panel te
  lleva a GitHub, GitHub crea la App y devuelve las credenciales, y después eliges en
  qué repositorios instalarla (todos o algunos).
- La URL del navegador y la del webhook se tratan por separado: acceder por `localhost` o
  por un túnel SSH ya no rompe el alta. Cuando no hay una dirección pública, la App se crea
  sin webhook y el panel explica cómo añadirlo después, en lugar de que GitHub rechace el
  manifiesto con «Hook url is not supported».
- Alta manual alternativa para quien ya tenga una App creada.
- Recursos desde repositorio: clonado superficial, build de la imagen y redespliegue
  automático en cada `push` a la rama elegida, validado con HMAC-SHA256.

### Interfaz

- Interfaz reescrita en **Preact** con esbuild; el código pasa de tres archivos sueltos
  a componentes por vista. El bundle (115 KB, 43 KB gzip) se versiona en `web/dist/`,
  así que `cargo build` sigue sin necesitar Node.
- Iconografía con `lucide-preact` y logos de marca con `simple-icons`.
- Rediseño completo: nueva escala tipográfica y de espaciado, navegación agrupada,
  tarjetas de recurso, avisos flotantes, diálogos accesibles y diseño adaptable.
- La pantalla de acceso, el pie del menú y la página Sistema indican que el panel está
  construido con Rust y Preact.

### Interno

- Nuevos módulos sin dependencias externas: analizador JSON, SHA-256/HMAC/Base64URL,
  cliente HTTPS sobre `curl` con lista blanca de hosts, y lanzador de subprocesos común.
- Los secretos (tokens de GitHub, cabeceras de autorización) nunca viajan por `argv`:
  van por stdin de `curl` o por un archivo de credenciales efímero de `git`.
- Los archivos estáticos se sirven prestados desde `.rodata` en vez de copiarse por
  petición, así el consumo no crece con el tráfico.
- Formato de estado `TDM2` con tipo de recurso y origen; el formato `TDM1` anterior se
  sigue leyendo y se migra al primer guardado.
- Borrado de recursos con tres niveles: desregistrar, detener contenedores, o borrar
  también volúmenes y archivos.
- 44 pruebas unitarias (antes 8) y smoke test ampliado a los nuevos endpoints.

### Requisitos nuevos

- `curl` para Docker Hub y GitHub, `openssl` para firmar los JWT de la GitHub App y
  `git` para clonar repositorios. Si falta alguno, el panel lo indica en Sistema y
  desactiva solo esa función; el resto sigue funcionando igual.

## 0.1.5 — 2026-08-14

- Carpeta de estado renombrada a `tinkiva-docker-manager/` (migración automática desde `tinkiva/`).
- Limpieza de `tinkiva.pid` residual y de binarios de instalación descargados tras el asistente.

## 0.1.4 — 2026-08-14

- Comando `tinkivadm logs [N] [-f]`: últimas N líneas del log y seguimiento en vivo.
- Comando `tinkivadm help` con la lista completa de comandos.

## 0.1.3 — 2026-08-14

- Todo el estado local (config, pid, log, datos, apps) vive dentro de `tinkiva/`.
- Mensaje de parada literal `tinkivadm stop` y errores de arranque con la última línea del log.
- Compatible con `tinkiva.env` previo en el directorio actual.

## 0.1.2 — 2026-08-14

- Nueva página de Procesos del host: top por CPU y RAM con ordenamiento.
- Iconos en el sidebar y rediseño del menú de navegación.
- Asistente de primera ejecución (`tinkivadm config`) que genera `tinkiva.env`.
- Gestión de demonio sin systemd: `tinkivadm start`, `stop` y `status` con pid file y log.
- Auto-actualización: `tinkivadm update [versión]` descarga de GitHub Releases y verifica sha256.
- Binario renombrado a `tinkivadm`; binarios de release estáticos (musl) compatibles con Amazon Linux.
- README reestructurado con guía de instalación de extremo a extremo.

## 0.1.1 — 2026-08-14

- Renombrado del proyecto a Tinkiva Docker Manager: binario, servicio systemd, usuario `tinkiva-docker`, rutas y variables `TDM_*`.
- Corregido el fallo de `gh release create` al no detectar el repositorio en el job de release.

## 0.1.0 — 2026-08-14

- MVP de un solo nodo y un solo administrador.
- Panel web embebido.
- Métricas del host y contenedores.
- Logs y acciones start/stop/restart.
- Registro de proyectos Compose.
- Webhook por proyecto, rama y token.
- Deploy con imagen inmutable, historial y rollback.
- Restauración automática al fallar.
- Plantilla PostgreSQL.
- Instalador systemd, CI y smoke test con Docker simulado.
