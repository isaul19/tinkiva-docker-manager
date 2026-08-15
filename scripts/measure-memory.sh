#!/usr/bin/env bash
set -euo pipefail
SERVICE=${1:-tinkiva-deploy-lite.service}
PID=$(systemctl show "$SERVICE" -p MainPID --value)
if [[ -z "$PID" || "$PID" == 0 ]]; then
  echo "El servicio no está activo." >&2
  exit 1
fi
printf 'Proceso principal:\n'
ps -p "$PID" -o pid,comm,rss,vsz,%mem,etime
printf '\nCgroup systemd:\n'
systemctl show "$SERVICE" -p MemoryCurrent -p MemoryPeak -p TasksCurrent --no-pager
