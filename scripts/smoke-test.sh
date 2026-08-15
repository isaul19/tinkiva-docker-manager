#!/usr/bin/env bash
set -euo pipefail
BINARY=${1:-target/release/tinkivadm}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BINARY=$(realpath "$BINARY")
TMP=$(mktemp -d)
TOKEN='0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
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

mkdir -p "$TMP/apps/demo" "$TMP/data"
cat > "$TMP/apps/demo/compose.yaml" <<'YAML'
services:
  api:
    image: ${APP_IMAGE}
YAML
printf 'APP_IMAGE=ghcr.io/example/app:zero\n' > "$TMP/apps/demo/.env"

TDM_BIND="127.0.0.1:$PORT" \
TDM_ADMIN_TOKEN="$TOKEN" \
TDM_DATA_DIR="$TMP/data" \
TDM_ALLOWED_ROOT="$TMP/apps" \
TDM_DOCKER_BIN="$ROOT/tests/mock-docker.sh" \
TDM_WORKERS=2 \
"$BINARY" >"$TMP/server.log" 2>&1 &
PID=$!

for _ in $(seq 1 80); do
  if curl -fsS "$BASE/healthz" >/dev/null; then break; fi
  sleep .1
done
curl -fsS "$BASE/healthz" | jq -e '.ok == true' >/dev/null
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/info")" == '401' ]]

AUTH=(-H "Authorization: Bearer $TOKEN")
FORM=(-H 'Content-Type: application/x-www-form-urlencoded')
curl -fsS "${AUTH[@]}" "$BASE/api/info" | jq -e '.docker.available == true' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/system" | jq -e '.process_rss >= 0' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/containers" | jq -e 'length == 2' >/dev/null

PROJECT_JSON=$(curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/projects" \
  --data-urlencode 'slug=demo' \
  --data-urlencode 'name=Demo API' \
  --data-urlencode 'compose_file=demo/compose.yaml' \
  --data-urlencode 'env_file=demo/.env' \
  --data-urlencode 'image_env=APP_IMAGE' \
  --data-urlencode 'branch=main')
echo "$PROJECT_JSON" | jq -e '.slug == "demo"' >/dev/null
WEBHOOK_TOKEN=$(echo "$PROJECT_JSON" | jq -r '.webhook_token')

curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/projects/demo/deploy" \
  --data-urlencode 'image=ghcr.io/example/app:one' \
  --data-urlencode 'branch=main' \
  --data-urlencode 'commit=1111111' | jq -e '.status == "success"' >/dev/null

# Un deploy fallido debe responder 502, restaurar .env y volver a levantar la imagen anterior.
FAIL_CODE=$(curl -sS -o "$TMP/failure.json" -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" \
  -X POST "$BASE/api/projects/demo/deploy" \
  --data-urlencode 'image=ghcr.io/example/app:fail-image' \
  --data-urlencode 'branch=main' \
  --data-urlencode 'commit=deadbee')
[[ "$FAIL_CODE" == '502' ]]
jq -e '.status == "failed"' "$TMP/failure.json" >/dev/null
grep -qx 'APP_IMAGE=ghcr.io/example/app:one' "$TMP/apps/demo/.env"

# El segundo deploy exitoso entra por el mismo webhook que usará GitHub Actions.
curl -fsS "${FORM[@]}" -H "X-Tinkiva-Token: $WEBHOOK_TOKEN" \
  -X POST "$BASE/hooks/deploy/demo" \
  --data-urlencode 'image=ghcr.io/example/app:two' \
  --data-urlencode 'branch=main' \
  --data-urlencode 'commit=2222222' | jq -e '.status == "success" and .trigger == "webhook"' >/dev/null

grep -qx 'APP_IMAGE=ghcr.io/example/app:two' "$TMP/apps/demo/.env"
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/projects/demo/rollback" | jq -e '.status == "success"' >/dev/null
grep -qx 'APP_IMAGE=ghcr.io/example/app:one' "$TMP/apps/demo/.env"
curl -fsS "${AUTH[@]}" "$BASE/api/history?project=demo&limit=10" | jq -e 'length == 4' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/projects/demo/logs?tail=50" | grep -q 'servicio iniciado'

curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/templates/postgres" \
  --data-urlencode 'slug=demo-db' \
  --data-urlencode 'name=Demo PostgreSQL' \
  --data-urlencode 'database=demo' \
  --data-urlencode 'username=demo' \
  --data-urlencode 'memory_mb=256' | jq -e '.project.slug == "demo-db" and (.password | length == 48)' >/dev/null

echo "Smoke test OK; RSS del proceso: $(ps -o rss= -p "$PID" | awk '{print $1 " KiB"}')"
