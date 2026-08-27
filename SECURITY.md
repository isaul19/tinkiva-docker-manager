# Seguridad

## Modelo de confianza

Tinkiva Docker Manager es una herramienta administrativa de un solo host. El proceso necesita acceso a Docker y, por tanto, debe tratarse como software con privilegios equivalentes a root. No está diseñado para recibir usuarios, archivos Compose o nombres de imagen de terceros no confiables.

## Controles incluidos

- Acceso al panel mediante usuario, hash Argon2 y sesión opaca de 12 horas.
- Cambio obligatorio de la contraseña inicial y bloqueo persistente por IP tras fallos.
- Token administrador separado para automatizaciones y comparación de tiempo constante.
- Token individual por proyecto para webhooks.
- Restricción opcional por rama.
- Validaciones conservadoras para slug, rama, referencia de imagen, contenedor y variables de entorno.
- Rutas canónicas confinadas a `TDM_ALLOWED_ROOT`.
- Escritura atómica del estado y archivos `.env` con permisos restrictivos.
- Límite de 32 KiB para cabeceras y 512 KiB para cuerpos HTTP.
- Límite de 2 MiB para logs devueltos.
- Pool fijo de workers y un único deployment simultáneo.
- CSP, `nosniff`, `DENY`, `no-referrer` y `no-store` en respuestas.
- Servicio systemd sin capabilities, con filesystem protegido salvo los directorios necesarios.
- El token del webhook se envía en cabecera, no en la URL.

### Salidas a internet

- Lista blanca de cuatro hosts (`api.github.com`, `github.com`, `hub.docker.com`,
  `registry.hub.docker.com`); solo `https://`, sin puerto explícito, sin credenciales en la
  URL y sin seguir redirecciones.
- Las cabeceras de autorización y los cuerpos con secretos viajan por stdin de `curl`,
  nunca por `argv`, para que no aparezcan en `ps`.
- Las respuestas remotas se acotan en bytes antes de analizarse.

### GitHub

- Las credenciales de la App (App ID, secreto de webhook y clave privada) se guardan en
  `<TDM_DATA_DIR>/github.json` con permisos `0600` y **nunca** se devuelven por la API.
- El manifiesto pide los permisos mínimos: `contents: read` y `metadata: read`.
- Los webhooks se validan con HMAC-SHA256 sobre el cuerpo crudo, comparado en tiempo
  constante. Un webhook sin firma válida recibe 401 y no dispara nada.
- Los retornos del navegador desde GitHub se validan con un nonce de un solo uso que caduca
  a los 15 minutos.
- El token de instalación se entrega a `git` en un archivo de credenciales temporal `0600`
  que se borra al terminar; no queda en `remote.origin.url` ni en `argv`, y se redacta de
  cualquier mensaje de error antes de guardarlo en el historial.
- La CSP añade `form-action https://github.com` únicamente porque el alta de un clic es un
  POST del navegador al formulario de manifiesto de GitHub. El resto sigue en `'self'`.

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

El panel no edita proyectos. Desregistra el proyecto y vuelve a registrarlo con un token
nuevo; hazlo con la opción «Solo desregistrar del panel» para no tocar archivos ni
contenedores.

## Revocar el acceso a GitHub

Desconectar la App desde el panel borra las credenciales locales, pero **no** elimina la
App en GitHub. Para cortar el acceso por completo:

1. Panel → GitHub → **Desconectar**.
2. En GitHub, `Settings → Developer settings → GitHub Apps`, elimina la App o revoca su
   instalación en los repositorios afectados.

## Reporte de vulnerabilidades

No publiques tokens, archivos `.env`, URIs de base de datos ni logs con secretos en un issue. Revoca primero las credenciales comprometidas y conserva solo un caso mínimo reproducible sin datos sensibles.
