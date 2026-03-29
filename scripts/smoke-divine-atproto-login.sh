#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

require_cmd curl
require_cmd grep
require_cmd sed

HANDLE="${HANDLE:-rabble.divine.video}"
HANDLE_RESOLVE_URL="https://public.api.bsky.app/xrpc/com.atproto.identity.resolveHandle?handle=${HANDLE}"
SUBDOMAIN_DID_URL="https://${HANDLE}/.well-known/atproto-did"

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

fetch() {
  local url="$1"
  local body="$2"
  local headers="$3"

  if ! curl -fsS -D "$headers" "$url" -o "$body"; then
    fail "request failed for $url"
  fi
}

extract_json_string() {
  local key="$1"
  local file="$2"
  sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$file" | head -n1
}

assert_json_response() {
  local url="$1"
  local body="$2"
  local headers="$3"
  local description="$4"

  if ! grep -qi '^content-type:.*application/.*json' "$headers"; then
    fail "$description did not return JSON: $url"
  fi

  if grep -qi '<!doctype html>' "$body" || grep -qi '<html' "$body"; then
    fail "$description returned HTML"
  fi
}

assert_regex() {
  local pattern="$1"
  local file="$2"
  local description="$3"

  if ! grep -Eq "$pattern" "$file"; then
    fail "$description missing expected value matching: $pattern"
  fi
}

handle_body="$tmpdir/handle.json"
handle_headers="$tmpdir/handle.headers"
fetch "$HANDLE_RESOLVE_URL" "$handle_body" "$handle_headers"
assert_json_response "$HANDLE_RESOLVE_URL" "$handle_body" "$handle_headers" "handle resolution"

resolved_did="$(extract_json_string did "$handle_body")"
if [ -z "$resolved_did" ]; then
  fail "could not parse did from handle resolution response"
fi

subdomain_body="$tmpdir/subdomain.did"
subdomain_headers="$tmpdir/subdomain.headers"
fetch "$SUBDOMAIN_DID_URL" "$subdomain_body" "$subdomain_headers"
if ! grep -qi '^content-type:.*text/plain' "$subdomain_headers"; then
  fail "subdomain DID resolution did not return text/plain"
fi

subdomain_did="$(tr -d '\r\n' < "$subdomain_body")"
if [ -z "$subdomain_did" ]; then
  fail "subdomain DID resolution returned an empty body"
fi

if [ "$resolved_did" != "$subdomain_did" ]; then
  fail "handle resolution and subdomain DID resolution disagree: $resolved_did vs $subdomain_did"
fi

PLCDOC_URL="https://plc.directory/${resolved_did}"
plc_body="$tmpdir/plc.json"
plc_headers="$tmpdir/plc.headers"
fetch "$PLCDOC_URL" "$plc_body" "$plc_headers"
assert_json_response "$PLCDOC_URL" "$plc_body" "$plc_headers" "PLC DID document"

assert_regex "\"id\"[[:space:]]*:[[:space:]]*\"${resolved_did}\"" "$plc_body" "PLC DID document"
assert_regex "\"serviceEndpoint\"[[:space:]]*:[[:space:]]*\"https://pds\\.divine\\.video\"" "$plc_body" "PLC DID document"
if grep -qF 'pds.staging.dvines.org' "$plc_body"; then
  fail "PLC DID document still points at staging"
fi

pds_describe_body="$tmpdir/pds-describe.json"
pds_describe_headers="$tmpdir/pds-describe.headers"
fetch "https://pds.divine.video/xrpc/com.atproto.server.describeServer" "$pds_describe_body" "$pds_describe_headers"
assert_json_response "https://pds.divine.video/xrpc/com.atproto.server.describeServer" "$pds_describe_body" "$pds_describe_headers" "pds.divine.video describeServer"
assert_regex "\"did\"[[:space:]]*:[[:space:]]*\"did:web:pds\\.divine\\.video\"" "$pds_describe_body" "pds.divine.video describeServer"

pds_protected_body="$tmpdir/pds-protected.json"
pds_protected_headers="$tmpdir/pds-protected.headers"
fetch "https://pds.divine.video/.well-known/oauth-protected-resource" "$pds_protected_body" "$pds_protected_headers"
assert_json_response "https://pds.divine.video/.well-known/oauth-protected-resource" "$pds_protected_body" "$pds_protected_headers" "pds.divine.video protected-resource metadata"
assert_regex "\"authorization_servers\"" "$pds_protected_body" "pds.divine.video protected-resource metadata"
assert_regex "https://entryway\\.divine\\.video" "$pds_protected_body" "pds.divine.video protected-resource metadata"

entryway_authz_body="$tmpdir/entryway-authz.json"
entryway_authz_headers="$tmpdir/entryway-authz.headers"
fetch "https://entryway.divine.video/.well-known/oauth-authorization-server" "$entryway_authz_body" "$entryway_authz_headers"
assert_json_response "https://entryway.divine.video/.well-known/oauth-authorization-server" "$entryway_authz_body" "$entryway_authz_headers" "entryway.divine.video authorization-server metadata"
assert_regex "\"issuer\"[[:space:]]*:[[:space:]]*\"https://entryway\\.divine\\.video\"" "$entryway_authz_body" "entryway.divine.video authorization-server metadata"
assert_regex "\"authorization_endpoint\"" "$entryway_authz_body" "entryway.divine.video authorization-server metadata"
assert_regex "\"pushed_authorization_request_endpoint\"" "$entryway_authz_body" "entryway.divine.video authorization-server metadata"

printf 'PASS: Divine ATProto login contract is healthy for %s\n' "$HANDLE"
