#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

: "${PDS_URL:?PDS_URL must be set}"
: "${HANDLE_ADMIN_URL:?HANDLE_ADMIN_URL must be set}"
: "${HANDLE_NAME:?HANDLE_NAME must be set}"
: "${HANDLE_DID:?HANDLE_DID must be set}"
: "${ATBRIDGE_PROVISION_URL:?ATBRIDGE_PROVISION_URL must be set}"
: "${ATPROTO_PROVISIONING_TOKEN:?ATPROTO_PROVISIONING_TOKEN must be set}"
: "${NOSTR_PUBKEY:?NOSTR_PUBKEY must be set}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

print_response() {
  local label="$1"
  local status="$2"
  local body_file="$3"

  printf '== %s ==\n' "$label"
  printf 'Status: %s\n' "$status"
  printf 'Body:\n'
  cat "$body_file"
  printf '\n\n'
}

run_step() {
  local label="$1"
  shift

  local headers_file="${tmpdir}/headers.$RANDOM"
  local body_file="${tmpdir}/body.$RANDOM"
  local status

  if ! curl -sS -D "$headers_file" -o "$body_file" "$@"; then
    print_response "$label" "curl-failed" "$body_file"
    printf 'Step failed: %s\n' "$label" >&2
    exit 1
  fi

  status="$(awk 'toupper($1) ~ /^HTTP/ { code = $2 } END { print code }' "$headers_file")"
  print_response "$label" "${status:-unknown}" "$body_file"

  if [[ -z "${status}" || ! "${status}" =~ ^2[0-9][0-9]$ ]]; then
    printf 'Step failed: %s\n' "$label" >&2
    exit 1
  fi
}

handle="${HANDLE_NAME}.divine.test"

run_step \
  "PDS health" \
  "$PDS_URL/xrpc/_health"

run_step \
  "Handle admin health" \
  "$HANDLE_ADMIN_URL/health"

run_step \
  "Create handle mapping" \
  -X POST "$HANDLE_ADMIN_URL/api/handles" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"$HANDLE_NAME\",\"did\":\"$HANDLE_DID\"}"

run_step \
  "Read handle mapping" \
  "$HANDLE_ADMIN_URL/api/handles/$HANDLE_NAME"

run_step \
  "Bridge provisioning" \
  -X POST "$ATBRIDGE_PROVISION_URL" \
  -H "Authorization: Bearer $ATPROTO_PROVISIONING_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"nostr_pubkey\":\"$NOSTR_PUBKEY\",\"handle\":\"$handle\"}"

printf 'Localnet smoke flow passed for %s (%s)\n' "$handle" "$NOSTR_PUBKEY"
