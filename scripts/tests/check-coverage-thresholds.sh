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
  "changed_rust": {
    "lines": 100,
    "allow_absent": ["crates/allowed/src/lib.rs"],
    "allow_uncovered": [
      {"path": "crates/example/src/lib.rs", "line": 12, "reason": "test fixture"}
    ]
  },
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

cat >"$tmp_dir/summary.json" <<'JSON'
{"data":[{"totals":{"lines":{"count":2,"covered":2,"percent":100},"functions":{"count":1,"covered":1,"percent":100},"regions":{"count":2,"covered":2,"percent":100}}}]}
JSON

cat >"$tmp_dir/low-regions.json" <<'JSON'
{"data":[{"totals":{"lines":{"count":2,"covered":2,"percent":100},"functions":{"count":1,"covered":1,"percent":100},"regions":{"count":10,"covered":4,"percent":40}}}]}
JSON

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
  --summary-json "$tmp_dir/summary.json" \
  --diff "$tmp_dir/changed.diff"

if "$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/uncovered.info" \
  --summary-json "$tmp_dir/summary.json" \
  --diff "$tmp_dir/changed.diff"; then
  echo "expected uncovered changed line to fail" >&2
  exit 1
fi

if "$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/covered.info" \
  --summary-json "$tmp_dir/low-regions.json" \
  --diff "$tmp_dir/changed.diff"; then
  echo "expected LLVM region coverage below the floor to fail" >&2
  exit 1
fi

cat >"$tmp_dir/test-source.diff" <<'DIFF'
diff --git a/crates/example/tests/integration.rs b/crates/example/tests/integration.rs
--- a/crates/example/tests/integration.rs
+++ b/crates/example/tests/integration.rs
@@ -0,0 +1 @@
+#[test] fn drives_covered_code() {}
DIFF

"$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/covered.info" \
  --summary-json "$tmp_dir/summary.json" \
  --diff "$tmp_dir/test-source.diff"

cat >"$tmp_dir/missing-file.diff" <<'DIFF'
diff --git a/crates/missing/src/lib.rs b/crates/missing/src/lib.rs
--- a/crates/missing/src/lib.rs
+++ b/crates/missing/src/lib.rs
@@ -0,0 +1 @@
+pub fn missing() {}
DIFF

if "$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/covered.info" \
  --summary-json "$tmp_dir/summary.json" \
  --diff "$tmp_dir/missing-file.diff"; then
  echo "expected a changed Rust file absent from LCOV to fail" >&2
  exit 1
fi

sed 's#crates/missing/src/lib.rs#crates/allowed/src/lib.rs#g' \
  "$tmp_dir/missing-file.diff" >"$tmp_dir/allowed-file.diff"

"$checker" \
  --thresholds "$tmp_dir/thresholds.json" \
  --lcov "$tmp_dir/covered.info" \
  --summary-json "$tmp_dir/summary.json" \
  --diff "$tmp_dir/allowed-file.diff"

cat >"$tmp_dir/malformed.json" <<'JSON'
{"version": 1}
JSON

if "$checker" \
  --thresholds "$tmp_dir/malformed.json" \
  --lcov "$tmp_dir/covered.info" \
  --summary-json "$tmp_dir/summary.json" \
  --diff "$tmp_dir/changed.diff"; then
  echo "expected malformed thresholds to fail" >&2
  exit 1
fi

echo "coverage checker self-tests passed"
