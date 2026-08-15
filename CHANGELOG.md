# Changelog

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
