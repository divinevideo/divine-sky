#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
thresholds="$repo_root/.coverage-thresholds.json"
lcov_path=""
base_ref=""
diff_path=""

while (($#)); do
  case "$1" in
    --thresholds) thresholds="$2"; shift 2 ;;
    --lcov) lcov_path="$2"; shift 2 ;;
    --base) base_ref="$2"; shift 2 ;;
    --diff) diff_path="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

for command_name in jq awk git; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "missing required command: $command_name" >&2
    exit 2
  }
done

[[ -f "$thresholds" ]] || { echo "missing thresholds: $thresholds" >&2; exit 2; }
[[ -f "$lcov_path" ]] || { echo "missing LCOV: $lcov_path" >&2; exit 2; }

jq -e '
  .version == 1 and
  (.global.lines | type == "number") and
  (.global.functions | type == "number") and
  (.global.regions | type == "number") and
  (.global.non_decreasing | type == "boolean") and
  .changed_rust.lines == 100
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
trap 'rm -f "$tmp_diff" "$changed_path" "$uncovered_path"' EXIT

awk '
  /^\+\+\+ b\// { file = substr($0, 7); next }
  /^@@ / {
    if (file !~ /\.rs$/) next
    split($0, parts, "+")
    split(parts[2], range, " ")
    split(range[1], nums, ",")
    start = nums[1] + 0
    count = (nums[2] == "" ? 1 : nums[2] + 0)
    for (i = 0; i < count; i++) print file ":" start + i
  }
' "$diff_path" >"$changed_path"

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

if [[ -s "$uncovered_path" ]]; then
  while IFS= read -r key; do
    echo "uncovered changed Rust line: $key" >&2
  done <"$uncovered_path"
  exit 1
fi

read -r lines_found lines_hit functions_found functions_hit regions_found regions_hit < <(
  awk -F: '
    $1 == "LF" { lf += $2 }
    $1 == "LH" { lh += $2 }
    $1 == "FNF" { fnf += $2 }
    $1 == "FNH" { fnh += $2 }
    $1 == "BRF" { brf += $2 }
    $1 == "BRH" { brh += $2 }
    END { print lf+0, lh+0, fnf+0, fnh+0, brf+0, brh+0 }
  ' "$lcov_path"
)

percentage() {
  awk -v hit="$1" -v count="$2" 'BEGIN { if (count == 0) print 100; else print hit * 100 / count }'
}

check_floor() {
  local name="$1" actual="$2" floor="$3"
  awk -v actual="$actual" -v floor="$floor" 'BEGIN { exit !(actual + 0 >= floor + 0) }' || {
    echo "$name coverage $actual is below threshold $floor" >&2
    exit 1
  }
}

line_pct="$(percentage "$lines_hit" "$lines_found")"
function_pct="$(percentage "$functions_hit" "$functions_found")"
region_pct="$(percentage "$regions_hit" "$regions_found")"

check_floor lines "$line_pct" "$(jq -r '.global.lines' "$thresholds")"
check_floor functions "$function_pct" "$(jq -r '.global.functions' "$thresholds")"
check_floor regions "$region_pct" "$(jq -r '.global.regions' "$thresholds")"

echo "coverage thresholds passed: lines=$line_pct functions=$function_pct regions=$region_pct changed_uncovered=0"
