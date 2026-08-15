#!/usr/bin/env bash
set -euo pipefail
if [[ $EUID -ne 0 ]]; then echo "Ejecuta con sudo." >&2; exit 1; fi
PURGE=${1:-}
systemctl disable --now tinkiva-docker-manager.service 2>/dev/null || true
rm -f /etc/systemd/system/tinkiva-docker-manager.service /usr/local/bin/tinkiva-docker-manager
rm -rf /usr/local/share/doc/tinkiva-docker-manager
systemctl daemon-reload
if [[ "$PURGE" == '--purge' ]]; then
  rm -rf /etc/tinkiva-docker-manager /var/lib/tinkiva-docker-manager
  userdel tinkiva-docker 2>/dev/null || true
  echo "Servicio, configuración, historial y usuario eliminados. /opt/tinkiva/apps se conservó para proteger tus proyectos."
else
  echo "Servicio eliminado. Configuración e historial se conservaron. Usa --purge para eliminarlos."
fi
