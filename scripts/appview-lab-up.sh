#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="$repo_root/deploy/appview-lab/.env"
compose_file="$repo_root/deploy/appview-lab/docker-compose.yml"
runtime_dir="$repo_root/.claude/appview-lab"

if [[ ! -f "$env_file" ]]; then
  cp "$repo_root/deploy/appview-lab/env.example" "$env_file"
fi

set -a
source "$env_file"
set +a

if [[ "${DIVINE_PDS_URL:-}" == "http://127.0.0.1:3000" ]]; then
  DIVINE_PDS_URL="http://127.0.0.1:2583"
fi

mkdir -p "$runtime_dir"

prepend_path() {
  local value="$1"
  local current="${2:-}"
  if [[ -n "$current" ]]; then
    printf '%s:%s' "$value" "$current"
  else
    printf '%s' "$value"
  fi
}

configure_libpq() {
  local lib_dir=""
  local include_dir=""
  local pkgconfig_dir=""
  local os_name
  os_name="$(uname -s)"

  if command -v pg_config >/dev/null 2>&1; then
    lib_dir="$(pg_config --libdir)"
    include_dir="$(pg_config --includedir)"
  elif [[ "$os_name" == "Darwin" ]] && command -v brew >/dev/null 2>&1; then
    local brew_prefix
    brew_prefix="$(brew --prefix libpq 2>/dev/null || true)"
    if [[ -n "$brew_prefix" && -d "$brew_prefix/lib" ]]; then
      export PATH="$(prepend_path "$brew_prefix/bin" "${PATH:-}")"
      lib_dir="$brew_prefix/lib"
      include_dir="$brew_prefix/include"
    fi
  fi

  if [[ -n "$lib_dir" && -d "$lib_dir" ]]; then
    export LIBRARY_PATH="$(prepend_path "$lib_dir" "${LIBRARY_PATH:-}")"
    if [[ "$os_name" == "Darwin" ]]; then
      export DYLD_FALLBACK_LIBRARY_PATH="$(prepend_path "$lib_dir" "${DYLD_FALLBACK_LIBRARY_PATH:-}")"
    else
      export LD_LIBRARY_PATH="$(prepend_path "$lib_dir" "${LD_LIBRARY_PATH:-}")"
    fi
    pkgconfig_dir="$lib_dir/pkgconfig"
    if [[ -d "$pkgconfig_dir" ]]; then
      export PKG_CONFIG_PATH="$(prepend_path "$pkgconfig_dir" "${PKG_CONFIG_PATH:-}")"
    fi
  fi

  if [[ -n "$include_dir" && -d "$include_dir" ]]; then
    export CPATH="$(prepend_path "$include_dir" "${CPATH:-}")"
  fi
}

wait_for_http() {
  local url="$1"
  local attempts="${2:-60}"
  local delay="${3:-2}"

  for ((i = 1; i <= attempts; i++)); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$delay"
  done

  echo "Timed out waiting for $url" >&2
  return 1
}

ensure_local_pds() {
  local default_local_pds="http://127.0.0.1:2583"

  if [[ "${DIVINE_PDS_URL}" == "$default_local_pds" ]]; then
    return 0
  fi

  wait_for_http "${DIVINE_PDS_URL}/xrpc/_health"
}

wait_for_appview_db() {
  for ((i = 1; i <= 60; i++)); do
    if docker compose --env-file "$env_file" -f "$compose_file" exec -T appview-db \
      pg_isready -U "${APPVIEW_DB_USER}" -d "${APPVIEW_DB_NAME}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "Timed out waiting for appview-db" >&2
  return 1
}

hydrate_media_views() {
  local rows
  rows="$(
    docker compose --env-file "$env_file" -f "$compose_file" exec -T appview-db \
      psql -U "${APPVIEW_DB_USER}" -d "${APPVIEW_DB_NAME}" -At -F '|' \
      -c "select did, blob_cid from appview_media_views where ready = false order by did, blob_cid"
  )"

  if [[ -z "$rows" ]]; then
    return 0
  fi

  while IFS='|' read -r did blob_cid; do
    [[ -n "$did" && -n "$blob_cid" ]] || continue
    DATABASE_URL="$DATABASE_URL" \
    APPVIEW_MEDIA_BASE_URL="$APPVIEW_MEDIA_BASE_URL" \
    APPVIEW_MEDIA_DID="$did" \
    APPVIEW_MEDIA_BLOB_CID="$blob_cid" \
      cargo run -p divine-video-worker >/dev/null
  done <<< "$rows"
}

run_bg() {
  local name="$1"
  shift
  (
    trap '' HUP
    exec "$@" >"$runtime_dir/${name}.log" 2>&1 </dev/null
  ) &
  echo $! >"$runtime_dir/${name}.pid"
}

configure_libpq
ensure_local_pds

compose_services=(appview-db)
export APPVIEW_MEDIA_PDS_URL="$DIVINE_PDS_URL"

compose_profiles=()
if [[ "${DIVINE_PDS_URL}" == "http://127.0.0.1:2583" ]]; then
  compose_profiles+=(--profile local-pds)
  compose_services+=(mock-pds)
  export APPVIEW_MEDIA_PDS_URL="http://mock-pds:2583"
fi

compose_services+=(media-view)
if [[ "${APPVIEW_ENABLE_RELAY:-false}" == "true" ]]; then
  compose_services+=(relay)
  compose_profiles+=(--profile relay)
fi
docker compose "${compose_profiles[@]}" --env-file "$env_file" -f "$compose_file" up -d "${compose_services[@]}"

wait_for_appview_db
wait_for_http "${APPVIEW_MEDIA_BASE_URL}/health"

if [[ "${DIVINE_PDS_URL}" == "http://127.0.0.1:2583" ]]; then
  wait_for_http "${DIVINE_PDS_URL}/xrpc/_health"
fi

for migration in "$repo_root"/migrations/*/up.sql; do
  docker compose --env-file "$env_file" -f "$compose_file" exec -T appview-db \
    psql -U "${APPVIEW_DB_USER}" -d "${APPVIEW_DB_NAME}" -f "/workspace/${migration#$repo_root/}"
done

export DATABASE_URL="postgres://${APPVIEW_DB_USER}:${APPVIEW_DB_PASSWORD}@127.0.0.1:${APPVIEW_DB_PORT}/${APPVIEW_DB_NAME}"
export APPVIEW_MEDIA_BASE_URL
export DIVINE_PDS_URL
export VIEWER_ORIGIN
export VITE_APPVIEW_BASE_URL="http://${APPVIEW_BIND_ADDR}"
export VITE_FEEDGEN_BASE_URL="http://${FEEDGEN_BIND_ADDR}"

env DATABASE_URL="$DATABASE_URL" DIVINE_PDS_URL="$DIVINE_PDS_URL" \
  cargo run -p divine-appview-indexer >"$runtime_dir/appview-indexer.log" 2>&1
hydrate_media_views
cargo build -p divine-appview -p divine-feedgen >"$runtime_dir/app-services-build.log" 2>&1
run_bg appview "$repo_root/target/debug/divine-appview"
run_bg feedgen "$repo_root/target/debug/divine-feedgen"

pushd "$repo_root/apps/divine-blacksky-viewer" >/dev/null
npm install --no-fund --no-audit
npm run build >"$runtime_dir/viewer-build.log" 2>&1
popd >/dev/null

docker compose "${compose_profiles[@]}" --env-file "$env_file" -f "$compose_file" up -d viewer
wait_for_http "http://127.0.0.1:${VIEWER_PORT}"

echo "Appview lab started. Logs: $runtime_dir"
