#!/usr/bin/env bash
# Reproduce the staging rsky-pds DID resolution flow end to end.
#
# Required environment:
#   PDS_URL
#   PDS_ADMIN_PASSWORD
#   CANARY_HANDLE
#
# Required when CANARY_DID is NOT set (PLC-minting path):
#   CANARY_EMAIL
#   CANARY_PASSWORD
#
# Optional:
#   CANARY_DID  — when set, uses admin DID-import path (no email/password needed)

set -euo pipefail

: "${PDS_URL:?PDS_URL must be set}"
: "${PDS_ADMIN_PASSWORD:?PDS_ADMIN_PASSWORD must be set}"
: "${CANARY_HANDLE:?CANARY_HANDLE must be set}"

# Email and password are only required for the PLC-minting path (no DID supplied)
if [[ -z "${CANARY_DID:-}" ]]; then
  : "${CANARY_EMAIL:?CANARY_EMAIL must be set when CANARY_DID is not provided}"
  : "${CANARY_PASSWORD:?CANARY_PASSWORD must be set when CANARY_DID is not provided}"
fi

# Admin auth uses HTTP Basic (admin:<password>)
BASIC_AUTH="$(printf 'admin:%s' "$PDS_ADMIN_PASSWORD" | base64)"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

last_body_file=""
last_status=""

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
  last_body_file="$body_file"
  last_status="${status:-unknown}"

  if [[ -z "${status}" || ! "${status}" =~ ^2[0-9][0-9]$ ]]; then
    printf 'Step failed: %s\n' "$label" >&2
    exit 1
  fi
}

extract_json_field() {
  local field="$1"
  local body_file="$2"

  if command -v jq >/dev/null 2>&1; then
    jq -r --arg field "$field" '.[$field] // empty' "$body_file"
    return
  fi

  sed -n "s/.*\"${field}\":\"\\([^\"]*\\)\".*/\\1/p" "$body_file" | head -n 1
}

if [[ -n "${CANARY_DID:-}" ]]; then
  # Admin DID-import path: only handle and DID needed (server generates placeholder email/password)
  create_account_payload="$(
    printf '{"handle":"%s","did":"%s"}' \
      "$CANARY_HANDLE" \
      "$CANARY_DID"
  )"
else
  # PLC-minting path: full payload required
  create_account_payload="$(
    printf '{"email":"%s","password":"%s","handle":"%s"}' \
      "$CANARY_EMAIL" \
      "$CANARY_PASSWORD" \
      "$CANARY_HANDLE"
  )"
fi

run_step \
  "PDS health" \
  "$PDS_URL/xrpc/_health"

run_step \
  "Create account" \
  -X POST "$PDS_URL/xrpc/com.atproto.server.createAccount" \
  -H "Authorization: Basic $BASIC_AUTH" \
  -H "Content-Type: application/json" \
  -d "$create_account_payload"

resolved_did="${CANARY_DID:-}"
if [[ -z "$resolved_did" ]]; then
  resolved_did="$(extract_json_field did "$last_body_file")"
fi

if [[ -z "$resolved_did" ]]; then
  printf 'Create account response did not contain a DID\n' >&2
  exit 1
fi

run_step \
  "Describe repo" \
  "$PDS_URL/xrpc/com.atproto.repo.describeRepo?repo=$resolved_did" \
  -H "Authorization: Basic $BASIC_AUTH"

printf 'Smoke flow passed for %s (%s)\n' "$CANARY_HANDLE" "$resolved_did"
