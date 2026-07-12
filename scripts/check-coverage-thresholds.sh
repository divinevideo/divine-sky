#!/usr/bin/env bash
set -euo pipefail

# LCOV canonicalizes macOS' /tmp symlink to /private/tmp. Canonicalize the
# repository root too so source-file paths compare consistently everywhere.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [[ "${1:-}" == "--self-test" ]]; then
  exec bash "$repo_root/scripts/tests/check-coverage-thresholds.sh"
fi
thresholds="$repo_root/.coverage-thresholds.json"
lcov_path=""
summary_json_path=""
base_ref=""
diff_path=""

while (($#)); do
  case "$1" in
    --thresholds) thresholds="$2"; shift 2 ;;
    --lcov) lcov_path="$2"; shift 2 ;;
    --summary-json) summary_json_path="$2"; shift 2 ;;
    --base) base_ref="$2"; shift 2 ;;
    --diff) diff_path="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

for command_name in jq awk git grep cut sort; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "missing required command: $command_name" >&2
    exit 2
  }
done

[[ -f "$thresholds" ]] || { echo "missing thresholds: $thresholds" >&2; exit 2; }
[[ -f "$lcov_path" ]] || { echo "missing LCOV: $lcov_path" >&2; exit 2; }
[[ -f "$summary_json_path" ]] || { echo "missing LLVM summary JSON: $summary_json_path" >&2; exit 2; }

jq -e '
  .version == 1 and
  (.global.lines | type == "number") and
  (.global.functions | type == "number") and
  (.global.regions | type == "number") and
  (.global.non_decreasing | type == "boolean") and
  .changed_rust.lines == 100 and
  (.changed_rust.allow_absent | type == "array") and
  all(.changed_rust.allow_absent[]; type == "string") and
  (.changed_rust.allow_uncovered | type == "array") and
  all(.changed_rust.allow_uncovered[];
    (.path | type == "string") and
    (.line | type == "number") and
    (.reason | type == "string" and length > 0)
  )
' "$thresholds" >/dev/null || {
  echo "invalid coverage threshold schema" >&2
  exit 2
}

tmp_diff=""
if [[ -z "$diff_path" ]]; then
  [[ -n "$base_ref" ]] || { echo "--base or --diff is required" >&2; exit 2; }
  tmp_diff="$(mktemp)"
  trap 'rm -f "$tmp_diff"' EXIT
  git -C "$repo_root" diff --unified=0 "$base_ref"...HEAD -- '*.rs' >"$tmp_diff"
  diff_path="$tmp_diff"
fi

changed_path="$(mktemp)"
uncovered_path="$(mktemp)"
covered_files_path="$(mktemp)"
trap 'rm -f "$tmp_diff" "$changed_path" "$uncovered_path" "$covered_files_path"' EXIT

awk '
  /^\+\+\+ b\// { file = substr($0, 7); next }
  /^@@ / {
    if (file !~ /\.rs$/) next
    # Integration-test sources drive instrumented code but cargo-llvm-cov does
    # not include the test harness itself in LCOV. Gate production Rust only.
    if (file ~ /(^|\/)tests\//) next
    split($0, parts, "+")
    split(parts[2], range, " ")
    split(range[1], nums, ",")
    start = nums[1] + 0
    count = (nums[2] == "" ? 1 : nums[2] + 0)
    for (i = 0; i < count; i++) print file ":" start + i
  }
' "$diff_path" >"$changed_path"

awk -v root="$repo_root/" '
  /^SF:/ {
    file = substr($0, 4)
    if (index(file, root) == 1) file = substr(file, length(root) + 1)
    print file
  }
' "$lcov_path" | sort -u >"$covered_files_path"

while IFS= read -r changed_file; do
  [[ -n "$changed_file" ]] || continue
  if ! grep -Fqx "$changed_file" "$covered_files_path"; then
    if jq -e --arg path "$changed_file" '.changed_rust.allow_absent | index($path) != null' "$thresholds" >/dev/null; then
      continue
    fi
    echo "changed Rust file absent from LCOV: $changed_file" >&2
    exit 1
  fi
done < <(cut -d: -f1 "$changed_path" | sort -u)

awk -v root="$repo_root/" '
  NR == FNR { changed[$0] = 1; next }
  /^SF:/ {
    file = substr($0, 4)
    if (index(file, root) == 1) file = substr(file, length(root) + 1)
    next
  }
  /^DA:/ {
    split(substr($0, 4), values, ",")
    key = file ":" values[1]
    if (changed[key] && values[2] + 0 == 0) print key
  }
' "$changed_path" "$lcov_path" >"$uncovered_path"

while IFS= read -r exception; do
  [[ -n "$exception" ]] || continue
  sed -i.bak "\\|^${exception}$|d" "$uncovered_path"
  rm -f "$uncovered_path.bak"
done < <(jq -r '.changed_rust.allow_uncovered[] | "\(.path):\(.line)"' "$thresholds")

if [[ -s "$uncovered_path" ]]; then
  while IFS= read -r key; do
    echo "uncovered changed Rust line: $key" >&2
  done <"$uncovered_path"
  exit 1
fi

check_floor() {
  local name="$1" actual="$2" floor="$3"
  awk -v actual="$actual" -v floor="$floor" 'BEGIN { exit !(actual + 0 >= floor + 0) }' || {
    echo "$name coverage $actual is below threshold $floor" >&2
    exit 1
  }
}

read -r line_pct function_pct region_pct < <(
  jq -er '
    .data[0].totals as $totals |
    select(($totals.lines.count | type) == "number") |
    select(($totals.functions.count | type) == "number") |
    select(($totals.regions.count | type) == "number" and $totals.regions.count > 0) |
    "\($totals.lines.percent) \($totals.functions.percent) \($totals.regions.percent)"
  ' "$summary_json_path"
) || {
  echo "invalid LLVM summary JSON or no instrumented regions" >&2
  exit 2
}

check_floor lines "$line_pct" "$(jq -r '.global.lines' "$thresholds")"
check_floor functions "$function_pct" "$(jq -r '.global.functions' "$thresholds")"
check_floor regions "$region_pct" "$(jq -r '.global.regions' "$thresholds")"

echo "coverage thresholds passed: lines=$line_pct functions=$function_pct regions=$region_pct changed_uncovered=0"
