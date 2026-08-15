# Changelog

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
