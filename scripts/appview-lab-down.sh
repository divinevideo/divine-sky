#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="$repo_root/deploy/appview-lab/.env"
compose_file="$repo_root/deploy/appview-lab/docker-compose.yml"
runtime_dir="$repo_root/.claude/appview-lab"

if [[ -f "$env_file" ]]; then
  set -a
  source "$env_file"
  set +a
fi

if [[ -d "$runtime_dir" ]]; then
  for pid_file in "$runtime_dir"/*.pid; do
    [[ -f "$pid_file" ]] || continue
    pid="$(cat "$pid_file")"
    kill "$pid" 2>/dev/null || true
    rm -f "$pid_file"
  done
fi

docker compose --profile local-pds --profile relay --env-file "$env_file" -f "$compose_file" down
