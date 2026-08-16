#!/usr/bin/env bash
set -euo pipefail
BINARY=${1:-target/release/tmanager}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [[ ! -x "$BINARY" ]]; then
  echo "No existe el binario ejecutable '$BINARY'. Compila con 'cargo build --release' o pasa la ruta correcta." >&2
  exit 1
fi
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
  if curl -fsS "$BASE/healthz" 2>/dev/null >/dev/null; then break; fi
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "El servidor terminó antes de responder a /healthz:" >&2
    cat "$TMP/server.log" >&2
    exit 1
  fi
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

# Exportación SQL: el panel detecta el motor, lista las bases y entrega el
# volcado como descarga con nombre de archivo propio.
curl -fsS "${AUTH[@]}" "$BASE/api/containers/postgres/export" \
  | jq -e '.database == "postgres" and .database_label == "PostgreSQL" and (.schemas | index("storagia")) != null' >/dev/null
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/containers/postgres/export" \
  --data-urlencode 'mode=structure' \
  --data-urlencode 'schemas=storagia,postgres' \
  -D "$TMP/export.headers" -o "$TMP/export.sql"
grep -qi 'content-disposition: attachment; filename="postgres.sql"' "$TMP/export.headers"
grep -q 'CREATE TABLE demo' "$TMP/export.sql"
grep -q -- '-- tinkiva: storagia' "$TMP/export.sql"

# Un contenedor que no es base de datos, un modo desconocido y una selección
# vacía deben rechazarse antes de tocar Docker.
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$BASE/api/containers/app/export")" == '422' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST \
  "$BASE/api/containers/postgres/export" -d 'mode=todo&schemas=storagia')" == '400' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST \
  "$BASE/api/containers/postgres/export" -d 'mode=all&schemas=')" == '400' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST \
  "$BASE/api/containers/postgres/export" -d 'mode=all&schemas=--host%3Devil')" == '400' ]]

# La ruta histórica de PostgreSQL debe seguir funcionando igual que antes.
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/templates/postgres" \
  --data-urlencode 'slug=demo-db' \
  --data-urlencode 'name=Demo PostgreSQL' \
  --data-urlencode 'database=demo' \
  --data-urlencode 'username=demo' \
  --data-urlencode 'memory_mb=256' | jq -e '.project.slug == "demo-db" and (.password | length == 48)' >/dev/null

# El catálogo alimenta el diálogo «Añadir recurso» de la interfaz.
curl -fsS "${AUTH[@]}" "$BASE/api/catalog" \
  | jq -e '(.engines | length) == 5 and (.popular_images | length) > 0 and (.capabilities | has("curl"))' >/dev/null

# Cada motor debe generar su Compose endurecido y su cadena de conexión.
for ENGINE in mysql mariadb mongodb redis; do
  RESULT=$(curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/database" \
    --data-urlencode "engine=$ENGINE" \
    --data-urlencode "slug=demo-$ENGINE" \
    --data-urlencode "name=Demo $ENGINE" \
    --data-urlencode 'database=demo' \
    --data-urlencode 'username=demo' \
    --data-urlencode 'memory_mb=128')
  echo "$RESULT" | jq -e --arg slug "demo-$ENGINE" \
    '.project.slug == $slug and .project.kind == "database" and (.connection_uri | length) > 0' >/dev/null
  grep -q 'no-new-privileges:true' "$TMP/apps/demo-$ENGINE/compose.yaml"
  grep -q 'TDM_MEMORY_LIMIT=128m' "$TMP/apps/demo-$ENGINE/.env"
  # Sin puerto pedido no se publica nada al exterior.
  ! grep -q 'ports:' "$TMP/apps/demo-$ENGINE/compose.yaml"
done

# Un servicio desde imagen queda con APP_IMAGE en .env, así el rollback funciona.
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/image" \
  --data-urlencode 'slug=demo-proxy' \
  --data-urlencode 'name=Demo proxy' \
  --data-urlencode 'image=nginx:1.27-alpine' \
  --data-urlencode 'container_port=80' \
  --data-urlencode 'published_port=8099' \
  --data-urlencode 'environment=LOG_LEVEL=info' \
  | jq -e '.project.kind == "image" and .project.image_env == "APP_IMAGE"' >/dev/null
grep -qx 'APP_IMAGE=nginx:1.27-alpine' "$TMP/apps/demo-proxy/.env"
grep -qx 'LOG_LEVEL=info' "$TMP/apps/demo-proxy/.env"
grep -q '"127.0.0.1:8099:80"' "$TMP/apps/demo-proxy/compose.yaml"

# Sin puerto local se publica en el mismo puerto que el contenedor.
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/image" \
  --data-urlencode 'slug=demo-same-port' \
  --data-urlencode 'name=Same port' \
  --data-urlencode 'image=nginx:1.27-alpine' \
  --data-urlencode 'container_port=3000' \
  | jq -e '.deployment.status == "success"' >/dev/null
grep -q '"127.0.0.1:3000:3000"' "$TMP/apps/demo-same-port/compose.yaml"
curl -fsS "${AUTH[@]}" -X DELETE "$BASE/api/projects/demo-same-port?remove=all" >/dev/null

# Variables reservadas y rutas de escape deben rechazarse.
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/image" \
  --data-urlencode 'slug=demo-bad' --data-urlencode 'name=Malo' \
  --data-urlencode 'image=nginx:1.27-alpine' \
  --data-urlencode 'environment=APP_IMAGE=otra:1')" == '422' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/image" \
  --data-urlencode 'slug=demo-bad' --data-urlencode 'name=Malo' \
  --data-urlencode 'image=nginx:1.27-alpine' \
  --data-urlencode 'volume_path=/data/../etc')" == '422' ]]

# Sin GitHub App conectada el estado debe decirlo sin romperse.
curl -fsS "${AUTH[@]}" "$BASE/api/github" | jq -e '.connected == false' >/dev/null
# El webhook entrante de GitHub se retiró: el auto-deploy es por polling saliente.
[[ "$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$BASE/hooks/github" \
  -H 'X-GitHub-Event: push' -H 'Content-Type: application/json' --data '{}')" == '404' ]]

# El borrado completo se lleva contenedores y archivos del recurso.
curl -fsS "${AUTH[@]}" -X DELETE "$BASE/api/projects/demo-redis?remove=all" | jq -e '.ok == true' >/dev/null
[[ ! -d "$TMP/apps/demo-redis" ]]
# El borrado por omisión no toca el disco.
curl -fsS "${AUTH[@]}" -X DELETE "$BASE/api/projects/demo-mongodb" | jq -e '.ok == true' >/dev/null
[[ -d "$TMP/apps/demo-mongodb" ]]

echo "Smoke test OK; RSS del proceso: $(ps -o rss= -p "$PID" | awk '{print $1 " KiB"}')"
