# Seguridad

## Modelo de confianza

Tinkiva Docker Manager es una herramienta administrativa de un solo host. El proceso necesita acceso a Docker y, por tanto, debe tratarse como software con privilegios equivalentes a root. No está diseñado para recibir usuarios, archivos Compose o nombres de imagen de terceros no confiables.

## Controles incluidos

- Token administrador obligatorio y comparación de tiempo constante.
- Token individual por proyecto para webhooks.
- Restricción opcional por rama.
- Validaciones conservadoras para slug, rama, referencia de imagen, contenedor y variables de entorno.
- Rutas canónicas confinadas a `TDM_ALLOWED_ROOT`.
- Escritura atómica del estado y archivos `.env` con permisos restrictivos.
- Límite de 32 KiB para cabeceras y 128 KiB para cuerpos HTTP.
- Límite de 2 MiB para logs devueltos.
- Pool fijo de workers y un único deployment simultáneo.
- CSP, `nosniff`, `DENY`, `no-referrer` y `no-store` en respuestas.
- Servicio systemd sin capabilities, con filesystem protegido salvo los directorios necesarios.
- El token del webhook se envía en cabecera, no en la URL.

## Operación segura

1. Mantén `TDM_BIND=127.0.0.1:8787`.
2. Usa un reverse proxy HTTPS o un túnel SSH.
3. Restringe el origen por firewall, VPN o allowlist cuando sea posible.
4. Usa imágenes inmutables y privadas en GHCR.
5. No uses `latest` para producción.
6. Da permisos `0600` a `.env` y archivos de secretos.
7. No permitas edición de Compose a usuarios no administradores.
8. Rota tokens tras una sospecha de filtración.
9. Actualiza Docker, Rust y el sistema operativo.
10. Realiza backups independientes de PostgreSQL y volúmenes.

## Rotar el token administrador

```bash
TOKEN=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
sudo sed -i "s/^TDM_ADMIN_TOKEN=.*/TDM_ADMIN_TOKEN=$TOKEN/" /etc/tinkiva-docker-manager/env
sudo systemctl restart tinkiva-docker-manager
printf '%s\n' "$TOKEN"
```

## Rotar un webhook

La versión 0.1 no edita proyectos. Desregistra el proyecto y vuelve a registrarlo con un token nuevo. Esto no elimina archivos ni contenedores.

## Reporte de vulnerabilidades

No publiques tokens, archivos `.env`, URIs de base de datos ni logs con secretos en un issue. Revoca primero las credenciales comprometidas y conserva solo un caso mínimo reproducible sin datos sensibles.
