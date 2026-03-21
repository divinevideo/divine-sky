#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

pds_env_file="${PDS_ENV_FILE:-$repo_root/deploy/localnet/pds/env.example}"
jetstream_env_file="${JETSTREAM_ENV_FILE:-$repo_root/deploy/localnet/jetstream/env.example}"

run_down() {
  local label="$1"
  shift

  printf '== Stopping %s ==\n' "$label"
  docker compose "$@" down
}

run_down "DNS" -f deploy/localnet/dns/docker-compose.yml
run_down "Jetstream" -f deploy/localnet/jetstream/docker-compose.yml --env-file "$jetstream_env_file"
run_down "PDS" -f deploy/localnet/pds/docker-compose.yml --env-file "$pds_env_file"
run_down "PLC" -f deploy/localnet/plc/docker-compose.yml
