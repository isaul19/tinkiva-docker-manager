#!/usr/bin/env bash
set -euo pipefail
sep=$'\037'
args=" $* "

if [[ "$args" == *" version --format "* ]]; then
  printf '27.5.1\n'
elif [[ "$args" == *" compose version "* ]]; then
  printf 'Docker Compose version v2.32.4\n'
elif [[ "$args" == *" compose "*" ps -a "* ]]; then
  printf 'running%shealthy%s0\n' "$sep" "$sep"
elif [[ "$args" == *" ps -a "* ]]; then
  printf 'abc123%sapp%sghcr.io/example/app:one%sUp 10 minutes%srunning%s127.0.0.1:3000->3000/tcp%s2026-08-14 10:00:00 -0500 -05\n' "$sep" "$sep" "$sep" "$sep" "$sep" "$sep"
  printf 'def456%spostgres%spostgres:17-alpine%sUp 10 minutes (healthy)%srunning%s5432/tcp%s2026-08-14 10:00:00 -0500 -05\n' "$sep" "$sep" "$sep" "$sep" "$sep" "$sep"
elif [[ "$args" == *" stats --no-stream "* ]]; then
  printf 'app%s1.20%%%s48MiB / 384MiB%s12.50%%%s1kB / 2kB%s0B / 0B%s8\n' "$sep" "$sep" "$sep" "$sep" "$sep" "$sep"
  printf 'postgres%s0.20%%%s92MiB / 512MiB%s17.97%%%s2kB / 3kB%s0B / 0B%s12\n' "$sep" "$sep" "$sep" "$sep" "$sep" "$sep"
elif [[ "$args" == *" inspect --format "* ]]; then
  # Solo `postgres` se comporta como base de datos; el resto no debe superar la
  # detección y por tanto no puede exportarse.
  container="${*: -1}"
  format=''
  previous=''
  for arg in "$@"; do
    if [[ "$previous" == '--format' ]]; then format="$arg"; fi
    previous="$arg"
  done
  if [[ "$format" == *'.Config.Env'* ]]; then
    if [[ "$container" == 'postgres' ]]; then
      printf 'POSTGRES_USER=postgres\nPOSTGRES_PASSWORD=secreto\nPOSTGRES_DB=storagia\n'
    else
      printf 'NODE_ENV=production\n'
    fi
  elif [[ "$container" == 'postgres' ]]; then
    printf 'postgres:17-alpine%s%spostgres%s{"5432/tcp":null}\n' "$sep" "$sep" "$sep"
  else
    printf 'ghcr.io/example/app:one%s%snode server.js%s{"3000/tcp":null}\n' "$sep" "$sep" "$sep"
  fi
elif [[ "$args" == *" exec "* ]]; then
  # El sondeo de clientes se comprueba antes que los scripts de exportación:
  # el propio sondeo menciona psql y pg_dump.
  if [[ "$args" == *'id -un'* ]]; then
    if [[ "$2" == 'postgres' ]]; then
      printf 'USER=postgres\npsql=1\n'
    else
      printf 'USER=node\n'
    fi
  elif [[ "$args" == *'pg_database'* ]]; then
    printf 'storagia\npostgres\n'
  elif [[ "$args" == *'pg_dump'* ]]; then
    for database in "${@:7}"; do
      printf -- '-- tinkiva: %s\nCREATE TABLE demo();\n' "$database"
    done
  else
    printf 'mock docker: exec no implementado: %s\n' "$*" >&2
    exit 2
  fi
elif [[ "$args" == *" logs "* ]] || [[ "$args" == *" compose "*" logs "* ]]; then
  printf '2026-08-14T10:00:00Z servicio iniciado\n'
elif [[ "$args" == *" network inspect "* ]]; then
  printf '[{"Name":"tinkiva"}]\n'
elif [[ "$args" == *" network create "* ]]; then
  printf 'mock-network-id\n'
elif [[ "$args" == *" compose "*" config --quiet "* ]]; then
  exit 0
elif [[ "$args" == *" compose "*" pull --quiet "* ]]; then
  printf 'imagen descargada\n'
elif [[ "$args" == *" compose "*" down "* ]]; then
  printf 'contenedores detenidos\n'
elif [[ "$args" == *" compose "*" up -d "* ]]; then
  compose_file=''
  previous=''
  for arg in "$@"; do
    if [[ "$previous" == '-f' ]]; then compose_file="$arg"; fi
    previous="$arg"
  done
  if [[ -n "$compose_file" && -f "$(dirname "$compose_file")/.env" ]] && grep -q 'fail-image' "$(dirname "$compose_file")/.env"; then
    printf 'imagen simulada inválida\n' >&2
    exit 1
  fi
  printf 'contenedores actualizados\n'
elif [[ "$args" == *" start "* ]] || [[ "$args" == *" stop "* ]] || [[ "$args" == *" restart "* ]]; then
  printf '%s\n' "${*: -1}"
else
  printf 'mock docker: comando no implementado: %s\n' "$*" >&2
  exit 2
fi
