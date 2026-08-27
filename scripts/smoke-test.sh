#!/usr/bin/env bash
set -euo pipefail

BINARY=${1:-target/release/tmanager}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
[[ -x "$BINARY" ]] || { echo "No existe el binario ejecutable '$BINARY'." >&2; exit 1; }
BINARY=$(realpath "$BINARY")
TMP=$(mktemp -d)
TOKEN='0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
ADMIN_USER='admin'
ADMIN_PASSWORD='initial-password-123'
PORT=$(python3 - <<'PY'
import socket
s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()
PY
)
BASE="http://127.0.0.1:$PORT"
PID=''
cleanup() {
  if [[ -n "$PID" ]]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
  rm -rf "$TMP"
}
trap cleanup EXIT
mkdir -p "$TMP/data"

TDM_BIND="127.0.0.1:$PORT" \
TDM_EDITION=createapp \
TDM_ADMIN_TOKEN="$TOKEN" \
TDM_ADMIN_USER="$ADMIN_USER" \
TDM_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
TDM_DATA_DIR="$TMP/data" \
TDM_DOCKER_BIN="$ROOT/tests/mock-docker.sh" \
TDM_WORKERS=2 \
"$BINARY" >"$TMP/server.log" 2>&1 &
PID=$!

for _ in $(seq 1 80); do
  curl -fsS "$BASE/healthz" >/dev/null 2>&1 && break
  if ! kill -0 "$PID" 2>/dev/null; then cat "$TMP/server.log" >&2; exit 1; fi
  sleep .1
done

curl -fsS "$BASE/healthz" | jq -e '.ok == true and .edition == "createapp"' >/dev/null
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/info")" == '401' ]]
AUTH=(-H "Authorization: Bearer $TOKEN")
FORM=(-H 'Content-Type: application/x-www-form-urlencoded')

LOGIN=$(curl -fsS "${FORM[@]}" -X POST "$BASE/api/auth/login" \
  --data-urlencode "username=$ADMIN_USER" --data-urlencode "password=$ADMIN_PASSWORD")
echo "$LOGIN" | jq -e '.must_change_password == true' >/dev/null
INITIAL_SESSION=$(echo "$LOGIN" | jq -r '.token')
SESSION=$(curl -fsS "${FORM[@]}" -H "Authorization: Bearer $INITIAL_SESSION" \
  -X POST "$BASE/api/auth/change-password" --data-urlencode 'password=replacement-password-456')
SESSION_TOKEN=$(echo "$SESSION" | jq -r '.token')

curl -fsS -H "Authorization: Bearer $SESSION_TOKEN" "$BASE/api/info" \
  | jq -e '.edition == "createapp" and .mode == "read-only" and .docker.available == true' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/system" \
  | jq -e '.cpu_percent >= 0 and .memory_total > 0 and .disk_total > 0' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/containers" \
  | jq -e 'length == 2 and .[0].cpu != null and .[1].memory != null' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/containers/postgres/logs?tail=100" \
  | grep -q 'servicio iniciado'

# Las rutas históricas de CI/CD y las acciones de escritura no existen.
for endpoint in \
  /api/projects /api/history /api/images /api/github /api/ecr /api/catalog \
  /api/containers/app/restart /hooks/deploy/demo; do
  code=$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" -X POST "$BASE$endpoint")
  [[ "$code" == '404' ]]
done

echo "Smoke test panel OK; RSS: $(ps -o rss= -p "$PID" | awk '{print $1 " KiB"}')"
