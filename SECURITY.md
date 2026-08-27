# Seguridad

## Modelo de acceso

- El panel escucha en `127.0.0.1:8787` por defecto.
- El acceso humano usa usuario, contraseña Argon2 y una sesión opaca de doce horas.
- El primer acceso obliga a cambiar la contraseña inicial.
- Un fallo bloquea la IP durante un minuto; tres fallos consecutivos la bloquean un día.
- `TDM_ADMIN_TOKEN` permite consultas automatizadas y debe tratarse como secreto.

## Docker

La edición TinkivaCreateApp solo ejecuta:

```text
docker version
docker ps -a
docker stats --no-stream --all
docker logs --timestamps --tail N CONTAINER
```

No ejecuta `pull`, `run`, `exec`, `start`, `stop`, `restart`, `rm`, `rmi`, `compose` ni login de
registros. Tampoco acepta webhooks o archivos.

El grupo `docker` concede capacidades equivalentes a root aunque esta aplicación limite sus
comandos. Protege la cuenta del servicio y no expongas el socket Docker a la red.

## Exposición del panel

Prefiere un túnel SSH. Si necesitas acceso remoto permanente, usa un proxy HTTPS, limita las
redes de origen y conserva el panel detrás de una capa de identidad adicional.

## Datos persistentes

`TDM_DATA_DIR` se crea con modo `0700`; los archivos de autenticación usan `0600`. No se guardan
credenciales de GitHub, ECR, repositorios, variables de aplicaciones ni contenido de logs.

## Reporte de vulnerabilidades

No publiques secretos ni detalles explotables en un issue abierto. Contacta al mantenedor del
repositorio de forma privada e incluye versión, impacto y pasos mínimos de reproducción.
