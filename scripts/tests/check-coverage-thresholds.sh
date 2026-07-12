#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
checker="$repo_root/scripts/check-coverage-thresholds.sh"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

cat >"$tmp_dir/thresholds.json" <<'JSON'
{
  "version": 1,
  "global": {"lines": 50, "functions": 50, "regions": 50, "non_decreasing": true},
  "changed_rust": {"lines": 100},
  "exclude": ["target/**"]
}
JSON

cat >"$tmp_dir/covered.info" <<'LCOV'
TN:
SF:crates/example/src/lib.rs
DA:10,1
DA:11,2
LF:2
LH:2
FNF:1
FNH:1
BRF:2
BRH:2
end_of_record
LCOV

cat >"$tmp_dir/uncovered.info" <<'LCOV'
TN:
SF:crates/example/src/lib.rs
DA:10,1
DA:11,0
LF:2
LH:1
FNF:1
FNH:1
BRF:2
BRH:1
end_of_record
LCOV

cat >"$tmp_dir/changed.diff" <<'DIFF'
diff --git a/crates/example/src/lib.rs b/crates/example/src/lib.rs
--- a/crates/example/src/lib.rs
+++ b/crates/example/src/lib.rs
@@ -0,0 +10,2 @@
+covered();
+also_covered();
DIFF

"$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/covered.info" \
  --diff "$tmp_dir/changed.diff"

if "$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/uncovered.info" \
  --diff "$tmp_dir/changed.diff"; then
  echo "expected uncovered changed line to fail" >&2
  exit 1
fi

cat >"$tmp_dir/malformed.json" <<'JSON'
{"version": 1}
JSON

if "$checker" \
  --thresholds "$tmp_dir/malformed.json" \
  --lcov "$tmp_dir/covered.info" \
  --diff "$tmp_dir/changed.diff"; then
  echo "expected malformed thresholds to fail" >&2
  exit 1
fi

echo "coverage checker self-tests passed"
