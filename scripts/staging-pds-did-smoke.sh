#!/usr/bin/env bash
# Reproduce the staging rsky-pds DID resolution flow end to end.
#
# Required environment:
#   PDS_URL
#   PDS_ADMIN_PASSWORD
#   CANARY_HANDLE
#   CANARY_DID

set -euo pipefail

: "${PDS_URL:?PDS_URL must be set}"
: "${PDS_ADMIN_PASSWORD:?PDS_ADMIN_PASSWORD must be set}"
: "${CANARY_HANDLE:?CANARY_HANDLE must be set}"
: "${CANARY_DID:?CANARY_DID must be set}"

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

create_account_payload="$(printf '{"did":"%s","handle":"%s"}' "$CANARY_DID" "$CANARY_HANDLE")"

run_step \
  "PDS health" \
  "$PDS_URL/xrpc/_health"

run_step \
  "Create account" \
  -X POST "$PDS_URL/xrpc/com.atproto.server.createAccount" \
  -H "Authorization: Bearer $PDS_ADMIN_PASSWORD" \
  -H "Content-Type: application/json" \
  -d "$create_account_payload"

run_step \
  "Describe repo" \
  "$PDS_URL/xrpc/com.atproto.repo.describeRepo?repo=$CANARY_DID" \
  -H "Authorization: Bearer $PDS_ADMIN_PASSWORD"

printf 'Smoke flow passed for %s (%s)\n' "$CANARY_HANDLE" "$CANARY_DID"
