#!/usr/bin/env bash
set -euo pipefail

# The Docker workflow skips its rust-gate job on pull requests, so a green pull
# request never exercises the gate. These tests do, against a stubbed `gh`.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
waiter="$repo_root/scripts/wait-for-rust-run.sh"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

if ! command -v jq >/dev/null 2>&1; then
  echo "missing required command: jq" >&2
  exit 1
fi

mkdir -p "$tmp_dir/bin"
cat >"$tmp_dir/bin/gh" <<'STUB'
#!/usr/bin/env bash
# Emits one line of GH_STUB_SCRIPT per invocation, then repeats the last line.
# A line is either a `gh run list --json` response or the token `error`.
attempt="$(cat "$GH_STUB_STATE" 2>/dev/null || echo 0)"
attempt=$((attempt + 1))
echo "$attempt" >"$GH_STUB_STATE"
response="$(printf '%s\n' "$GH_STUB_SCRIPT" | sed -n "${attempt}p")"
if [[ -z "$response" ]]; then
  response="$(printf '%s\n' "$GH_STUB_SCRIPT" | tail -n 1)"
fi
if [[ "$response" == "error" ]]; then
  echo "stubbed gh failure" >&2
  exit 1
fi
printf '%s\n' "$response"
STUB
chmod +x "$tmp_dir/bin/gh"

export PATH="$tmp_dir/bin:$PATH"
export GITHUB_REPOSITORY="divinevideo/divine-sky"
export HEAD_SHA="0123456789abcdef0123456789abcdef01234567"
export WAIT_INTERVAL_SECONDS=0

completed_success='[{"conclusion":"success","status":"completed"}]'
completed_failure='[{"conclusion":"failure","status":"completed"}]'
completed_neutral='[{"conclusion":"neutral","status":"completed"}]'
in_progress='[{"conclusion":"","status":"in_progress"}]'
no_runs='[]'

# run_waiter <name> <attempts> <missing-limit> <stub-script>
run_waiter() {
  local name="$1"
  export WAIT_ATTEMPTS="$2"
  export MISSING_RUN_ATTEMPTS="$3"
  export GH_STUB_SCRIPT="$4"
  export GH_STUB_STATE="$tmp_dir/state-$name"
  rm -f "$GH_STUB_STATE"
  set +e
  "$waiter" >"$tmp_dir/out-$name" 2>&1
  local status=$?
  set -e
  return "$status"
}

# expect_pass <name> <attempts> <missing-limit> <stub-script>
expect_pass() {
  if ! run_waiter "$@"; then
    echo "expected $1 to pass:" >&2
    cat "$tmp_dir/out-$1" >&2
    exit 1
  fi
}

# expect_fail_matching <name> <attempts> <missing-limit> <stub-script> <pattern>
expect_fail_matching() {
  local name="$1" pattern="$5"
  if run_waiter "$1" "$2" "$3" "$4"; then
    echo "expected $name to fail:" >&2
    cat "$tmp_dir/out-$name" >&2
    exit 1
  fi
  if ! grep -q "$pattern" "$tmp_dir/out-$name"; then
    echo "expected $name output to match '$pattern':" >&2
    cat "$tmp_dir/out-$name" >&2
    exit 1
  fi
}

expect_pass passing-run 5 3 "$completed_success"

# A run still going is worth waiting for; only the whole window gives up.
expect_pass slow-run 3 99 \
  "$(printf '%s\n%s\n%s' "$in_progress" "$in_progress" "$completed_success")"

# A transient gh failure must not be read as "no run yet".
expect_pass transient-gh-error 5 1 \
  "$(printf '%s\n%s' error "$completed_success")"

# A run that only appears after a few polls must not trip the missing-run bail.
expect_pass late-run 6 4 \
  "$(printf '%s\n%s\n%s' "$no_runs" "$no_runs" "$completed_success")"

expect_fail_matching failed-run 5 3 "$completed_failure" "concluded 'failure'"

# An unrecognised terminal conclusion fails closed rather than publishing.
expect_fail_matching unknown-conclusion 5 3 "$completed_neutral" "concluded 'neutral'"

# A ref that never landed on main has no push run and never will.
expect_fail_matching no-run 99 3 "$no_runs" "No Rust workflow run on main"

expect_fail_matching never-completes 3 99 "$in_progress" "Timed out waiting"

echo "rust-gate waiter self-tests passed"
