# Changelog

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
