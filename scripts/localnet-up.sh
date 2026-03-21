#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

pds_env_file="${PDS_ENV_FILE:-$repo_root/deploy/localnet/pds/env.example}"
jetstream_env_file="${JETSTREAM_ENV_FILE:-$repo_root/deploy/localnet/jetstream/env.example}"

run_up() {
  local label="$1"
  shift

  printf '== Starting %s ==\n' "$label"
  docker compose "$@" up -d
}

run_up "PLC" -f deploy/localnet/plc/docker-compose.yml
run_up "PDS" -f deploy/localnet/pds/docker-compose.yml --env-file "$pds_env_file"
run_up "Jetstream" -f deploy/localnet/jetstream/docker-compose.yml --env-file "$jetstream_env_file"
run_up "DNS" -f deploy/localnet/dns/docker-compose.yml

cat <<'EOF'
Localnet slices are starting.

Manual follow-up:
- Provide real TS_AUTHKEY values or authenticate the tailscale sidecars interactively.
- Mount or copy TLS certificates into the nginx cert volumes for plc, pds, jetstream, and dns.
- Verify the expected tailnet hostnames resolve before running the smoke script.
- Register username.divine.test mappings through the handle-admin service once DNS is reachable.
EOF
