#!/usr/bin/env bash
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "Ejecuta este script con sudo." >&2
  exit 1
fi

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BINARY=${1:-"$ROOT/target/release/tinkivadm"}
BIND=${TDM_INSTALL_BIND:-127.0.0.1:8787}
USER_NAME=tinkiva-docker

[[ -x "$BINARY" ]] || { echo "No existe un binario ejecutable en $BINARY" >&2; exit 1; }
command -v docker >/dev/null || { echo "Docker no está instalado." >&2; exit 1; }
docker compose version >/dev/null || { echo "Docker Compose v2 no está disponible." >&2; exit 1; }
getent group docker >/dev/null || { echo "No existe el grupo docker." >&2; exit 1; }

if ! getent group "$USER_NAME" >/dev/null 2>&1; then
  groupadd --system "$USER_NAME"
fi
if ! id "$USER_NAME" >/dev/null 2>&1; then
  useradd --system --gid "$USER_NAME" --home-dir /var/lib/tinkiva-docker-manager --shell /usr/sbin/nologin "$USER_NAME"
fi
usermod -aG docker "$USER_NAME"

install -d -m 0700 -o "$USER_NAME" -g "$USER_NAME" /var/lib/tinkiva-docker-manager
install -d -m 0700 -o "$USER_NAME" -g "$USER_NAME" /var/lib/tinkiva-docker-manager/.docker
install -d -m 0750 -o "$USER_NAME" -g docker /opt/tinkiva/apps
install -d -m 0750 -o root -g root /etc/tinkiva-docker-manager
install -d -m 0755 -o root -g root /usr/local/share/doc/tinkiva-docker-manager
install -m 0755 -o root -g root "$BINARY" /usr/local/bin/tinkivadm
install -m 0644 -o root -g root "$ROOT/deploy/tinkiva-docker-manager.service" /etc/systemd/system/tinkiva-docker-manager.service
install -m 0644 -o root -g root "$ROOT/README.md" /usr/local/share/doc/tinkiva-docker-manager/README.md

if [[ ! -f /etc/tinkiva-docker-manager/env ]]; then
  TOKEN=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
  cat > /etc/tinkiva-docker-manager/env <<EOF
TDM_BIND=$BIND
TDM_ADMIN_TOKEN=$TOKEN
TDM_DATA_DIR=/var/lib/tinkiva-docker-manager
TDM_ALLOWED_ROOT=/opt/tinkiva/apps
TDM_DOCKER_BIN=/usr/bin/docker
TDM_WORKERS=2
TDM_MAX_HISTORY=200
EOF
  chmod 0600 /etc/tinkiva-docker-manager/env
else
  TOKEN=$(sed -n 's/^TDM_ADMIN_TOKEN=//p' /etc/tinkiva-docker-manager/env | head -n1)
fi

systemctl daemon-reload
systemctl enable --now tinkiva-docker-manager.service
sleep 1
systemctl --no-pager --full status tinkiva-docker-manager.service || true

cat <<EOF

Instalación completada.
Panel local: http://$BIND
Token administrador: $TOKEN

Acceso seguro sin publicar el puerto:
  ssh -L 8787:127.0.0.1:8787 USUARIO@SERVIDOR
  abre http://127.0.0.1:8787

El token también quedó en /etc/tinkiva-docker-manager/env (modo 0600).
Para GitHub Actions necesitas publicar el endpoint por HTTPS; revisa deploy/nginx.example.conf.
EOF
