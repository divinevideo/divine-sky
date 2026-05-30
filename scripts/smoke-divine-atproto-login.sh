#!/usr/bin/env bash
set -euo pipefail

show_help() {
  cat <<'EOF'
Usage: smoke-divine-atproto-login.sh [--help]

Validates the public Divine ATProto login contract for the configured handle.

Environment:
  HANDLE   Handle to probe. Defaults to rabble.divine.video.
EOF
}

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

case "${1:-}" in
  -h|--help)
    show_help
    exit 0
    ;;
  "")
    ;;
  *)
    fail "unknown argument: $1"
    ;;
esac

require_cmd curl
require_cmd python3

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

json_path() {
  local file="$1"
  local path="$2"

  python3 - "$file" "$path" <<'PY'
import json
import sys

file = sys.argv[1]
path = sys.argv[2].split('.')

with open(file, 'r', encoding='utf-8') as fh:
    value = json.load(fh)

for key in path:
    if isinstance(value, list):
        if not key.isdigit():
            sys.exit(1)
        index = int(key)
        if index >= len(value):
            sys.exit(1)
        value = value[index]
        continue

    if not isinstance(value, dict) or key not in value:
        sys.exit(1)
    value = value[key]

if isinstance(value, (dict, list)):
    print(json.dumps(value))
else:
    print(value)
PY
}

assert_json() {
  local file="$1"
  local description="$2"

  python3 - "$file" "$description" <<'PY'
import json
import sys

file, description = sys.argv[1:3]
with open(file, 'r', encoding='utf-8') as fh:
    try:
        json.load(fh)
    except Exception as exc:
        sys.stderr.write(f"FAIL: {description} did not return valid JSON: {exc}\n")
        sys.exit(1)
PY
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local description="$3"

  if [ "$actual" != "$expected" ]; then
    fail "$description expected $expected but got $actual"
  fi
}

assert_array_contains() {
  local file="$1"
  local path="$2"
  local expected="$3"
  local description="$4"

  python3 - "$file" "$path" "$expected" "$description" <<'PY'
import json
import sys

file, path, expected, description = sys.argv[1:5]
parts = path.split('.')

with open(file, 'r', encoding='utf-8') as fh:
    value = json.load(fh)

for key in parts:
    if isinstance(value, list):
        if not key.isdigit():
            sys.stderr.write(f"FAIL: {description} missing expected field: {path}\n")
            sys.exit(1)
        index = int(key)
        if index >= len(value):
            sys.stderr.write(f"FAIL: {description} missing expected field: {path}\n")
            sys.exit(1)
        value = value[index]
        continue

    if not isinstance(value, dict) or key not in value:
        sys.stderr.write(f"FAIL: {description} missing expected field: {path}\n")
        sys.exit(1)
    value = value[key]

if not isinstance(value, list) or expected not in value:
    sys.stderr.write(f"FAIL: {description} missing expected value: {expected}\n")
    sys.exit(1)
PY
}

assert_json_string() {
  local file="$1"
  local path="$2"
  local expected="$3"
  local description="$4"

  actual="$(json_path "$file" "$path")" || fail "$description missing expected field: $path"
  assert_eq "$expected" "$actual" "$description"
}

assert_no_staging() {
  local file="$1"
  local description="$2"

  python3 - "$file" "$description" <<'PY'
import sys

file, description = sys.argv[1:3]
with open(file, 'r', encoding='utf-8') as fh:
    body = fh.read()

if 'pds.staging.dvines.org' in body:
    sys.exit(0)

sys.stderr.write(f"FAIL: {description} still points at staging\n")
sys.exit(1)
PY
}

HANDLE_BODY="$tmpdir/handle.json"
HANDLE_HEADERS="$tmpdir/handle.headers"
fetch "$HANDLE_RESOLVE_URL" "$HANDLE_BODY" "$HANDLE_HEADERS"
assert_json "$HANDLE_BODY" "handle resolution"

resolved_did="$(json_path "$HANDLE_BODY" did)" || fail "could not parse did from handle resolution response"

SUBDOMAIN_BODY="$tmpdir/subdomain.did"
SUBDOMAIN_HEADERS="$tmpdir/subdomain.headers"
fetch "$SUBDOMAIN_DID_URL" "$SUBDOMAIN_BODY" "$SUBDOMAIN_HEADERS"
subdomain_did="$(tr -d '\r\n' < "$SUBDOMAIN_BODY")"
assert_eq "$resolved_did" "$subdomain_did" "subdomain DID resolution"

PLC_BODY="$tmpdir/plc.json"
PLC_HEADERS="$tmpdir/plc.headers"
fetch "https://plc.directory/${resolved_did}" "$PLC_BODY" "$PLC_HEADERS"
assert_json "$PLC_BODY" "PLC DID document"
assert_json_string "$PLC_BODY" id "$resolved_did" "PLC DID document"
assert_json_string "$PLC_BODY" service.0.serviceEndpoint "https://pds.divine.video" "PLC DID document"
assert_no_staging "$PLC_BODY" "PLC DID document"

# The public PDS must answer as a real resource server, not an HTML landing page.
PDS_DESCRIBE_BODY="$tmpdir/pds-describe.json"
PDS_DESCRIBE_HEADERS="$tmpdir/pds-describe.headers"
fetch "https://pds.divine.video/xrpc/com.atproto.server.describeServer" "$PDS_DESCRIBE_BODY" "$PDS_DESCRIBE_HEADERS"
assert_json "$PDS_DESCRIBE_BODY" "pds.divine.video describeServer"
assert_json_string "$PDS_DESCRIBE_BODY" did "did:web:pds.divine.video" "pds.divine.video describeServer"

PDS_PROTECTED_BODY="$tmpdir/pds-protected.json"
PDS_PROTECTED_HEADERS="$tmpdir/pds-protected.headers"
fetch "https://pds.divine.video/.well-known/oauth-protected-resource" "$PDS_PROTECTED_BODY" "$PDS_PROTECTED_HEADERS"
assert_json "$PDS_PROTECTED_BODY" "pds.divine.video protected-resource metadata"
assert_array_contains "$PDS_PROTECTED_BODY" authorization_servers "https://entryway.divine.video" "pds.divine.video protected-resource metadata"

# The entryway must expose OAuth metadata for client discovery.
ENTRYWAY_AUTHZ_BODY="$tmpdir/entryway-authz.json"
ENTRYWAY_AUTHZ_HEADERS="$tmpdir/entryway-authz.headers"
fetch "https://entryway.divine.video/.well-known/oauth-authorization-server" "$ENTRYWAY_AUTHZ_BODY" "$ENTRYWAY_AUTHZ_HEADERS"
assert_json "$ENTRYWAY_AUTHZ_BODY" "entryway.divine.video authorization-server metadata"
assert_json_string "$ENTRYWAY_AUTHZ_BODY" issuer "https://entryway.divine.video" "entryway.divine.video authorization-server metadata"
assert_json_string "$ENTRYWAY_AUTHZ_BODY" authorization_endpoint "https://entryway.divine.video/api/oauth/authorize" "entryway.divine.video authorization-server metadata"
assert_json_string "$ENTRYWAY_AUTHZ_BODY" pushed_authorization_request_endpoint "https://entryway.divine.video/api/oauth/par" "entryway.divine.video authorization-server metadata"

printf 'PASS: Divine ATProto login contract is healthy for %s\n' "$HANDLE"
