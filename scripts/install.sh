#!/usr/bin/env bash
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "Ejecuta este script con sudo." >&2
  exit 1
fi

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BINARY=${1:-"$ROOT/target/release/tinkiva-deploy-lite"}
BIND=${TDL_INSTALL_BIND:-127.0.0.1:8787}
USER_NAME=tinkiva-deploy

[[ -x "$BINARY" ]] || { echo "No existe un binario ejecutable en $BINARY" >&2; exit 1; }
command -v docker >/dev/null || { echo "Docker no está instalado." >&2; exit 1; }
docker compose version >/dev/null || { echo "Docker Compose v2 no está disponible." >&2; exit 1; }
getent group docker >/dev/null || { echo "No existe el grupo docker." >&2; exit 1; }

if ! getent group "$USER_NAME" >/dev/null 2>&1; then
  groupadd --system "$USER_NAME"
fi
if ! id "$USER_NAME" >/dev/null 2>&1; then
  useradd --system --gid "$USER_NAME" --home-dir /var/lib/tinkiva-deploy-lite --shell /usr/sbin/nologin "$USER_NAME"
fi
usermod -aG docker "$USER_NAME"

install -d -m 0700 -o "$USER_NAME" -g "$USER_NAME" /var/lib/tinkiva-deploy-lite
install -d -m 0700 -o "$USER_NAME" -g "$USER_NAME" /var/lib/tinkiva-deploy-lite/.docker
install -d -m 0750 -o "$USER_NAME" -g docker /opt/tinkiva/apps
install -d -m 0750 -o root -g root /etc/tinkiva-deploy-lite
install -d -m 0755 -o root -g root /usr/local/share/doc/tinkiva-deploy-lite
install -m 0755 -o root -g root "$BINARY" /usr/local/bin/tinkiva-deploy-lite
install -m 0644 -o root -g root "$ROOT/deploy/tinkiva-deploy-lite.service" /etc/systemd/system/tinkiva-deploy-lite.service
install -m 0644 -o root -g root "$ROOT/README.md" /usr/local/share/doc/tinkiva-deploy-lite/README.md

if [[ ! -f /etc/tinkiva-deploy-lite/env ]]; then
  TOKEN=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
  cat > /etc/tinkiva-deploy-lite/env <<EOF
TDL_BIND=$BIND
TDL_ADMIN_TOKEN=$TOKEN
TDL_DATA_DIR=/var/lib/tinkiva-deploy-lite
TDL_ALLOWED_ROOT=/opt/tinkiva/apps
TDL_DOCKER_BIN=/usr/bin/docker
TDL_WORKERS=2
TDL_MAX_HISTORY=200
EOF
  chmod 0600 /etc/tinkiva-deploy-lite/env
else
  TOKEN=$(sed -n 's/^TDL_ADMIN_TOKEN=//p' /etc/tinkiva-deploy-lite/env | head -n1)
fi

systemctl daemon-reload
systemctl enable --now tinkiva-deploy-lite.service
sleep 1
systemctl --no-pager --full status tinkiva-deploy-lite.service || true

cat <<EOF

Instalación completada.
Panel local: http://$BIND
Token administrador: $TOKEN

Acceso seguro sin publicar el puerto:
  ssh -L 8787:127.0.0.1:8787 USUARIO@SERVIDOR
  abre http://127.0.0.1:8787

El token también quedó en /etc/tinkiva-deploy-lite/env (modo 0600).
Para GitHub Actions necesitas publicar el endpoint por HTTPS; revisa deploy/nginx.example.conf.
EOF
