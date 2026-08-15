#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID -ne 0 ]]; then echo "Ejecuta con sudo." >&2; exit 1; fi
PURGE=${1:-}
systemctl disable --now tinkiva-deploy-lite.service 2>/dev/null || true
rm -f /etc/systemd/system/tinkiva-deploy-lite.service /usr/local/bin/tinkiva-deploy-lite
rm -rf /usr/local/share/doc/tinkiva-deploy-lite
systemctl daemon-reload
if [[ "$PURGE" == '--purge' ]]; then
  rm -rf /etc/tinkiva-deploy-lite /var/lib/tinkiva-deploy-lite
  userdel tinkiva-deploy 2>/dev/null || true
  echo "Servicio, configuración, historial y usuario eliminados. /opt/tinkiva/apps se conservó para proteger tus proyectos."
else
  echo "Servicio eliminado. Configuración e historial se conservaron. Usa --purge para eliminarlos."
fi
