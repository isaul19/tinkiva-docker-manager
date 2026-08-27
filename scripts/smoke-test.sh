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

mkdir -p "$TMP/apps/demo" "$TMP/data"
cat > "$TMP/apps/demo/compose.yaml" <<'YAML'
services:
  api:
    image: ${APP_IMAGE}
YAML
printf 'APP_IMAGE=ghcr.io/example/app:zero\n' > "$TMP/apps/demo/.env"

TDM_BIND="127.0.0.1:$PORT" \
TDM_ADMIN_TOKEN="$TOKEN" \
TDM_ADMIN_USER="$ADMIN_USER" \
TDM_ADMIN_PASSWORD="$ADMIN_PASSWORD" \
TDM_STORE=sqlite \
TDM_SQLITE_PATH="$TMP/data/tinkiva.sqlite3" \
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
[[ -f "$TMP/data/tinkiva.sqlite3" ]]
[[ "$(head -c 15 "$TMP/data/tinkiva.sqlite3")" == 'SQLite format 3' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/info")" == '401' ]]

AUTH=(-H "Authorization: Bearer $TOKEN")
FORM=(-H 'Content-Type: application/x-www-form-urlencoded')

LOGIN_JSON=$(curl -fsS "${FORM[@]}" -X POST "$BASE/api/auth/login" \
  --data-urlencode "username=$ADMIN_USER" \
  --data-urlencode "password=$ADMIN_PASSWORD")
echo "$LOGIN_JSON" | jq -e '.must_change_password == true' >/dev/null
INITIAL_SESSION=$(echo "$LOGIN_JSON" | jq -r '.token')
[[ "$(curl -sS -o /dev/null -w '%{http_code}' \
  -H "Authorization: Bearer $INITIAL_SESSION" "$BASE/api/info")" == '401' ]]

SESSION_JSON=$(curl -fsS "${FORM[@]}" -H "Authorization: Bearer $INITIAL_SESSION" \
  -X POST "$BASE/api/auth/change-password" \
  --data-urlencode 'password=replacement-password-456')
echo "$SESSION_JSON" | jq -e '.must_change_password == false' >/dev/null
SESSION_TOKEN=$(echo "$SESSION_JSON" | jq -r '.token')
curl -fsS -H "Authorization: Bearer $SESSION_TOKEN" "$BASE/api/info" \
  | jq -e '.docker.available == true' >/dev/null

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

# Importación SQL: el archivo sube como cuerpo crudo, el panel lo escribe en un
# temporal según llega y lo conecta a la entrada estándar del cliente. El mock
# devuelve el tamaño que recibió, así que la prueba confirma el trayecto entero.
SQL=(-H 'Content-Type: application/sql')
printf 'CREATE TABLE demo(id int);\nINSERT INTO demo VALUES (1);\n' > "$TMP/import.sql"
IMPORT_BYTES=$(wc -c < "$TMP/import.sql")
curl -fsS "${AUTH[@]}" "${SQL[@]}" -X POST \
  "$BASE/api/containers/postgres/import?schema=storagia" \
  --data-binary "@$TMP/import.sql" \
  | jq -e --argjson bytes "$IMPORT_BYTES" \
    '.ok == true and .bytes == $bytes and (.output | contains("restaurado en storagia: \($bytes) bytes"))' >/dev/null

# El temporal de la subida no puede quedarse en el disco del servidor.
[[ -z "$(find "${TMPDIR:-/tmp}" -maxdepth 1 -name 'tdm-upload-*' 2>/dev/null)" ]]

# Un archivo vacío, un destino inválido, uno ausente y un contenedor que no es
# base de datos se rechazan antes de tocar Docker.
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${SQL[@]}" -X POST \
  "$BASE/api/containers/postgres/import?schema=storagia" --data-binary '')" == '400' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${SQL[@]}" -X POST \
  "$BASE/api/containers/postgres/import?schema=--host%3Devil" \
  --data-binary "@$TMP/import.sql")" == '400' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${SQL[@]}" -X POST \
  "$BASE/api/containers/postgres/import" --data-binary "@$TMP/import.sql")" == '400' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${SQL[@]}" -X POST \
  "$BASE/api/containers/app/import?schema=storagia" \
  --data-binary "@$TMP/import.sql")" == '422' ]]
# Sin token no se escribe nada en disco: la subida ni siquiera se acepta.
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${SQL[@]}" -X POST \
  "$BASE/api/containers/postgres/import?schema=storagia" \
  --data-binary "@$TMP/import.sql")" == '401' ]]

# Imágenes: se listan con su peso y solo se pueden borrar las que nadie usa.
curl -fsS "${AUTH[@]}" "$BASE/api/images" | jq -e 'length == 4' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/images" \
  | jq -e '[.[] | select(.reference == "nginx:1.27")][0] | .in_use == true and (.containers | index("app")) != null' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/images" \
  | jq -e '[.[] | select(.in_use == false)] | length == 2' >/dev/null
# Las más pesadas primero y con el tamaño exacto en bytes, no la cifra redondeada.
curl -fsS "${AUTH[@]}" "$BASE/api/images" \
  | jq -e '.[0].reference == "postgres:17-alpine" and .[0].size_bytes == 271000000' >/dev/null
curl -fsS "${AUTH[@]}" "$BASE/api/images" | jq -e '.[0].created_since == "2 days ago"' >/dev/null
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" -X DELETE "$BASE/api/images?reference=nginx:1.27")" == '409' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" -X DELETE "$BASE/api/images?reference=noexiste:1")" == '404' ]]
curl -fsS "${AUTH[@]}" -X DELETE "$BASE/api/images?reference=333333333333" | jq -e '.ok == true' >/dev/null

# `demo` desplegó ghcr.io/example/app:one y luego :two, así que :one es su
# destino de rollback y la limpieza no debe llevárselo por delante.
curl -fsS "${AUTH[@]}" "$BASE/api/images" \
  | jq -e '[.[] | select(.reference == "ghcr.io/example/app:one")][0].protected_by == "demo"' >/dev/null
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/images/prune" \
  | jq -e '.ok == true and .kept >= 1 and (.failed | length) == 0' >/dev/null

# La ruta histórica de PostgreSQL debe seguir funcionando igual que antes.
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/templates/postgres" \
  --data-urlencode 'slug=demo-db' \
  --data-urlencode 'name=Demo PostgreSQL' \
  --data-urlencode 'database=demo' \
  --data-urlencode 'username=demo' \
  --data-urlencode 'memory_mb=256' | jq -e '.project.slug == "demo-db" and (.password | length == 48)' >/dev/null

# El catálogo alimenta el diálogo «Añadir recurso» de la interfaz.
curl -fsS "${AUTH[@]}" "$BASE/api/catalog" \
  | jq -e '(.engines | length) == 5 and (.capabilities | has("curl")) and (has("popular_images") | not)' >/dev/null

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

# «Sin límite de RAM» debe dejar el Compose sin mem_limit y el .env sin la
# variable: Docker tampoco obliga a poner un techo.
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/database" \
  --data-urlencode 'engine=postgres' \
  --data-urlencode 'slug=demo-sin-limite' \
  --data-urlencode 'name=Demo sin limite' \
  --data-urlencode 'database=demo' \
  --data-urlencode 'username=demo' \
  --data-urlencode 'memory_mb=256' \
  --data-urlencode 'memory_unlimited=true' | jq -e '.project.slug == "demo-sin-limite"' >/dev/null
! grep -q 'mem_limit' "$TMP/apps/demo-sin-limite/compose.yaml"
! grep -q 'TDM_MEMORY_LIMIT' "$TMP/apps/demo-sin-limite/.env"
grep -q 'no-new-privileges:true' "$TMP/apps/demo-sin-limite/compose.yaml"
curl -fsS "${AUTH[@]}" -X DELETE "$BASE/api/projects/demo-sin-limite?remove=all" >/dev/null

# Sin ECR conectado el estado debe decirlo, y las credenciales inválidas se
# rechazan antes de tocar la red.
curl -fsS "${AUTH[@]}" "$BASE/api/ecr" | jq -e '.connected == false' >/dev/null
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/ecr" \
  --data-urlencode 'access_key_id=AKIA;rm -rf /' --data-urlencode 'secret_access_key=x' \
  --data-urlencode 'region=us-east-1')" == '422' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/ecr" \
  --data-urlencode 'access_key_id=AKIAIOSFODNN7EXAMPLE' --data-urlencode 'secret_access_key=x' \
  --data-urlencode 'region=US-EAST-1')" == '422' ]]
# Sin registro conectado no se puede listar ni crear un recurso de ECR.
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "$BASE/api/ecr/repositories")" == '502' ]]
[[ "$(curl -sS -o /dev/null -w '%{http_code}' "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/ecr" \
  --data-urlencode 'slug=demo-imagen' --data-urlencode 'name=Demo imagen' \
  --data-urlencode 'image=123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest')" == '422' ]]

# Un recurso Compose puede declarar la imagen que debe vigilar el watcher.
curl -fsS "${AUTH[@]}" "${FORM[@]}" -X POST "$BASE/api/resources/compose" \
  --data-urlencode 'slug=demo-ecr' \
  --data-urlencode 'name=Demo ECR' \
  --data-urlencode 'watch_image=123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest' \
  --data-urlencode 'compose=services:
  app:
    image: 123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest
' | jq -e '.current_image == "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest" and .auto_deploy == true' >/dev/null
curl -fsS "${AUTH[@]}" -X DELETE "$BASE/api/projects/demo-ecr?remove=all" >/dev/null

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
