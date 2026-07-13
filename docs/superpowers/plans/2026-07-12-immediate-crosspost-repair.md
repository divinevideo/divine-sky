# Immediate Crosspost Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deploy proactive PDS-session refresh and safely revive the currently stranded BadJwt publication jobs so the missing Divine videos publish and play on Bluesky.

**Architecture:** Keep this incident slice compatible with the current database and queue. Add a dry-run-first repair binary backed by a small append-only audit table; it selects only exact known events or the exact observed BadJwt signature, revalidates account/job state in one PostgreSQL transaction, and makes terminal jobs claimable without introducing the later reconciliation schema. Promote one immutable application image from staging to production, then verify source IDs, mappings, AppView records, and playback.

**Tech Stack:** Rust 2021, Diesel/PostgreSQL, cargo-llvm-cov, Kubernetes/ArgoCD, rsky PDS, Bluesky AppView.

---

## File map

- Create `.coverage-thresholds.json`: versioned global non-regression and 100% changed-code thresholds.
- Create `scripts/check-coverage-thresholds.sh`: validate LCOV changed Rust lines and the threshold schema.
- Modify `.github/workflows/rust.yml`: install/run `cargo-llvm-cov` and the checker.
- Create `migrations/007_operator_actions/up.sql` and `down.sql`: append-only repair audit and before-images.
- Modify `crates/divine-bridge-db/src/migrations.rs`: embed migration 007.
- Modify `crates/divine-bridge-db/src/schema.rs`: Diesel schema for `operator_actions`.
- Modify `crates/divine-bridge-db/src/models.rs`: repair request/result/audit data types.
- Modify `crates/divine-bridge-db/src/queries.rs`: preview, confirm, rollback, and audit queries.
- Create `crates/divine-atbridge/src/legacy_repair.rs`: validation and repair orchestration.
- Modify `crates/divine-atbridge/src/lib.rs`: export `legacy_repair`.
- Create `crates/divine-atbridge/src/bin/repair_legacy_badjwt.rs`: operator CLI.
- Modify `crates/divine-atbridge/Cargo.toml`: register the repair binary.
- Create `crates/divine-atbridge/tests/legacy_badjwt_repair.rs`: isolated PostgreSQL behavior coverage.
- Modify `docs/runbooks/bluesky-crosspost-launch-blockers.md`: exact staging/production repair and rollback commands.

## Task 1: Establish the blocking coverage gate

**Files:**
- Create: `.coverage-thresholds.json`
- Create: `scripts/check-coverage-thresholds.sh`
- Modify: `.github/workflows/rust.yml`

- [ ] **Step 1: Add the threshold contract**

Create this exact initial contract; `global` is populated from the first clean main-branch measurement, while changed Rust lines are immediately 100%:

```json
{
  "version": 1,
  "global": {
    "lines": 0,
    "functions": 0,
    "regions": 0,
    "non_decreasing": true
  },
  "changed_rust": {
    "lines": 100
  },
  "exclude": [
    "target/**"
  ]
}
```

- [ ] **Step 2: Write the checker test fixture first**

Add shell self-tests at the bottom of `scripts/check-coverage-thresholds.sh` behind `--self-test`. Fixtures must prove: one uncovered changed `.rs` line exits 1; all changed `.rs` lines covered exits 0; malformed/missing threshold fields exit 1. Use temporary LCOV/diff files and never read production data.

- [ ] **Step 3: Run the absent checker to establish RED**

Run:

```bash
bash scripts/check-coverage-thresholds.sh --self-test
```

Expected: FAIL because the script does not exist.

- [ ] **Step 4: Implement the checker**

The script must accept `--lcov <path> --base <git-ref>`, validate JSON with `jq`, obtain changed Rust lines from `git diff --unified=0 "$base"...HEAD -- '*.rs'`, parse LCOV `SF`/`DA` records, and fail unless every changed executable line has a hit count greater than zero. It must also compare reported global percentages to the JSON floors and reject decreases. No `eval`, secret output, or network access.

- [ ] **Step 5: Wire CI**

After the Rust toolchain step in `.github/workflows/rust.yml`, add:

```yaml
      - name: Install cargo-llvm-cov
        uses: taiki-e/install-action@cargo-llvm-cov

      - name: Verify coverage thresholds
        run: |
          cargo llvm-cov --workspace --all-features --lcov --output-path target/llvm-cov/lcov.info
          bash scripts/check-coverage-thresholds.sh \
            --lcov target/llvm-cov/lcov.info \
            --base "${{ github.event.pull_request.base.sha || github.event.before }}"
```

- [ ] **Step 6: Verify and commit**

Run:

```bash
bash scripts/check-coverage-thresholds.sh --self-test
cargo llvm-cov --workspace --all-features --lcov --output-path target/llvm-cov/lcov.info
bash scripts/check-coverage-thresholds.sh --lcov target/llvm-cov/lcov.info --base origin/main
```

Expected: self-tests PASS; coverage command succeeds; checker reports 100% of changed executable Rust lines covered. Replace the three global zero floors with measured main-branch percentages before commit.

```bash
git add .coverage-thresholds.json scripts/check-coverage-thresholds.sh .github/workflows/rust.yml
git commit -m "ci: enforce bridge coverage thresholds"
```

## Task 2: Add append-only operator repair audit state

**Files:**
- Create: `migrations/007_operator_actions/up.sql`
- Create: `migrations/007_operator_actions/down.sql`
- Modify: `crates/divine-bridge-db/src/migrations.rs`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`

- [ ] **Step 1: Write the failing migration integration assertion**

In `crates/divine-atbridge/tests/legacy_badjwt_repair.rs`, create an isolated schema using the existing `TEST_DATABASE_URL` pattern, run pending migrations, and assert `operator_actions` has `operation_id`, `action_type`, `actor`, `scope`, `dry_run`, `confirmation_digest`, `before_images`, `matched_count`, `changed_count`, immutable apply/rollback counts and timestamps, and `status`. Assert a duplicate `operation_id` fails.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair migration_creates_append_only_operator_actions -- --exact
```

Expected: FAIL because the test/table does not exist.

- [ ] **Step 3: Add migration 007**

`up.sql` must create:

```sql
CREATE TABLE IF NOT EXISTS operator_actions (
    operation_id TEXT PRIMARY KEY CHECK (operation_id ~ '^[0-9a-f-]{36}$'),
    action_type TEXT NOT NULL CHECK (action_type IN ('repair_legacy_badjwt')),
    actor TEXT NOT NULL,
    scope JSONB NOT NULL,
    dry_run BOOLEAN NOT NULL,
    confirmation_digest TEXT NOT NULL,
    before_images JSONB NOT NULL DEFAULT '[]'::jsonb,
    matched_count BIGINT NOT NULL DEFAULT 0,
    changed_count BIGINT NOT NULL DEFAULT 0,
    applied_count BIGINT NOT NULL DEFAULT 0,
    applied_at TIMESTAMPTZ,
    rollback_restored_count BIGINT NOT NULL DEFAULT 0,
    rollback_skipped_count BIGINT NOT NULL DEFAULT 0,
    rollback_at TIMESTAMPTZ,
    status TEXT NOT NULL CHECK (status IN ('previewed','applied','rolled_back','failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_operator_actions_type_created
    ON operator_actions (action_type, created_at DESC);
```

`down.sql` drops only `operator_actions`. Embed it after migration 006 and add matching Diesel declarations/models. `before_images` stores only job IDs/state/attempt/error/lease/completion timestamps—never event payloads or credentials.

- [ ] **Step 4: Run GREEN and commit**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair migration_creates_append_only_operator_actions -- --exact
```

Expected: PASS.

```bash
git add migrations/007_operator_actions crates/divine-bridge-db/src/{migrations.rs,schema.rs,models.rs} crates/divine-atbridge/tests/legacy_badjwt_repair.rs
git commit -m "feat(db): audit legacy publish repairs"
```

## Task 3: Implement dry-run preview and confirmation digest

**Files:**
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-atbridge/src/legacy_repair.rs`
- Modify: `crates/divine-atbridge/src/lib.rs`
- Modify: `crates/divine-atbridge/tests/legacy_badjwt_repair.rs`

- [ ] **Step 1: Write failing preview tests**

Cover these exact cases:

```rust
#[test]
fn preview_matches_only_terminal_failed_jobs_with_exact_error_and_account_scope() {}

#[test]
fn preview_accepts_explicit_full_event_ids_without_error_matching() {}

#[test]
fn preview_rejects_short_event_ids_empty_actor_zero_limit_and_unready_account() {}

#[test]
fn preview_digest_is_stable_and_excludes_payload_and_credentials() {}
```

Seed matching and near-miss rows: nonterminal failed, published, wrong account, substring-only error, exact error, and disabled account.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair preview_ -- --nocapture
```

Expected: FAIL because `legacy_repair` does not exist.

- [ ] **Step 3: Define the request/result types**

Use these public shapes:

```rust
pub const BADJWT_SIGNATURE_ERROR: &str = "BadJwt: Signature tag didn't verify";

pub struct LegacyRepairScope {
    pub nostr_pubkey: String,
    pub event_ids: Vec<String>,
    pub exact_error: Option<String>,
    pub max_rows: i64,
}

pub struct LegacyRepairPreview {
    pub operation_id: uuid::Uuid,
    pub actor: String,
    pub matched_event_ids: Vec<String>,
    pub confirmation_digest: String,
}
```

Add `uuid = { version = "1", features = ["v4", "serde"] }` under root
`[workspace.dependencies]` and `uuid = { workspace = true }` under
`crates/divine-atbridge/Cargo.toml`. Store its lowercase hyphenated text form.
Validate full lowercase 64-character event IDs, `1..=1000` max rows, nonempty
actor/account, and either explicit IDs or the one exact allowlisted error.
Compute SHA-256 over canonical JSON containing operation ID, actor, normalized
scope, sorted matched full IDs, and their current state/version timestamps.

- [ ] **Step 4: Implement a parameterized locked preview query**

Select only rows joined to `account_links` where `provisioning_state='ready'`, `crosspost_enabled=true`, `state='failed'`, `completed_at IS NOT NULL`, no active lease, exact account, and either exact full IDs or exact error equality. Order by full event ID and limit. Do not use string-formatted SQL for values.

- [ ] **Step 5: Run GREEN and commit**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair preview_ -- --nocapture
cargo test -p divine-atbridge legacy_repair
```

Expected: all preview tests PASS.

```bash
git add Cargo.toml Cargo.lock crates/divine-atbridge crates/divine-bridge-db
git commit -m "feat(atbridge): preview bounded BadJwt repairs"
```

## Task 4: Implement confirmed repair and safe rollback

**Files:**
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Modify: `crates/divine-atbridge/src/legacy_repair.rs`
- Modify: `crates/divine-atbridge/tests/legacy_badjwt_repair.rs`

- [ ] **Step 1: Write failing mutation tests**

Add:

```rust
#[test]
fn confirm_rechecks_digest_and_atomically_makes_jobs_claimable() {}

#[test]
fn confirm_rejects_changed_scope_state_or_digest_without_partial_updates() {}

#[test]
fn confirm_is_idempotent_by_operation_id() {}

#[test]
fn rollback_restores_only_jobs_unchanged_since_repair() {}

#[test]
fn rollback_refuses_claimed_or_completed_jobs() {}
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair confirm_ -- --nocapture
cargo test -p divine-atbridge --test legacy_badjwt_repair rollback_ -- --nocapture
```

Expected: FAIL because confirm/rollback are absent.

- [ ] **Step 3: Implement one-transaction confirmation**

Inside a serializable Diesel transaction, lock the previewed job IDs, recompute the digest, and require equality. Insert/update the audit row, save bounded before-images, then set each job to:

```text
state = 'failed'
completed_at = NULL
lease_owner = NULL
lease_expires_at = NOW()
error = exact previous error (retained for audit/diagnosis)
updated_at = NOW()
```

Do not reset `attempt`; current claim logic will pick the nonterminal failed row. Mark the audit `applied` with changed count. Repeating the same operation ID returns the stored applied result without another mutation.

- [ ] **Step 4: Implement guarded rollback**

Rollback loads the audit before-images and updates a job only when its current state is still `failed`, `completed_at IS NULL`, lease is absent/expired, and `updated_at` equals the action's recorded post-update timestamp. Any changed job produces a nonzero skipped count; rollback never overwrites a claimed/published job.

Apply and rollback remain distinct audit phases: confirmation writes immutable
`applied_count`/`applied_at`, while rollback writes
`rollback_restored_count`/`rollback_skipped_count`/`rollback_at`. Repeating a
full or partial rollback returns those persisted rollback facts without touching
publish jobs again.

- [ ] **Step 5: Run GREEN and commit**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair -- --test-threads=1
cargo test -p divine-bridge-db
```

Expected: PASS with no partial mutation in negative cases.

```bash
git add crates/divine-atbridge/src/legacy_repair.rs crates/divine-atbridge/tests/legacy_badjwt_repair.rs crates/divine-bridge-db/src/queries.rs
git commit -m "feat(atbridge): confirm and rollback BadJwt repairs"
```

## Task 5: Add the operator CLI

**Files:**
- Create: `crates/divine-atbridge/src/bin/repair_legacy_badjwt.rs`
- Modify: `crates/divine-atbridge/Cargo.toml`
- Modify: `crates/divine-atbridge/tests/legacy_badjwt_repair.rs`

- [ ] **Step 1: Write failing CLI parser/output tests**

Test dry-run default, explicit `--confirm-digest`, `--rollback-operation-id`, repeated full `--event-id`, exact-error mode, maximum rows, missing `DATABASE_URL`, and JSON output redaction. Assert output never contains `event_payload`, JWT-like text, URLs, or database credentials.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair cli_ -- --nocapture
```

Expected: FAIL because the binary is absent.

- [ ] **Step 3: Implement the binary**

Register:

```toml
[[bin]]
name = "repair-legacy-badjwt"
path = "src/bin/repair_legacy_badjwt.rs"
```

Use a small explicit argument parser (no shell evaluation). Required preview arguments are `--actor`, `--nostr-pubkey`, `--max-rows`, plus repeated `--event-id` or `--exact-badjwt`. Confirmation additionally requires `--operation-id` and `--confirm-digest`. Output one JSON object with operation ID, mode, full matched IDs, counts, digest, and status.

- [ ] **Step 4: Run GREEN and commit**

```bash
cargo test -p divine-atbridge --test legacy_badjwt_repair cli_ -- --nocapture
cargo run -p divine-atbridge --bin repair-legacy-badjwt -- --help
```

Expected: tests PASS and help exits 0 without requiring service configuration.

```bash
git add crates/divine-atbridge/Cargo.toml crates/divine-atbridge/src/bin/repair_legacy_badjwt.rs crates/divine-atbridge/tests/legacy_badjwt_repair.rs
git commit -m "feat(atbridge): add audited BadJwt repair CLI"
```

## Task 6: Verify the proactive session-refresh image

**Files:**
- Modify only if a failing test exposes a defect: `crates/divine-atbridge/src/publisher.rs`, `runtime.rs`, `video_service.rs`
- Test: existing publisher/video-service tests and `crates/divine-atbridge/tests/publish_path_integration.rs`

- [ ] **Step 1: Run the focused refresh tests at commit e532ea1+**

```bash
cargo test -p divine-atbridge proactive
cargo test -p divine-atbridge expired
cargo test -p divine-atbridge --test publish_path_integration -- --test-threads=1
```

Expected: proactive refresh occurs before `getServiceAuth`, rotated session persists, and publish uses the refreshed access token. If any fail, add the failing regression case first, make the smallest fix, rerun, and commit `fix(atbridge): preserve proactive session refresh`.

- [ ] **Step 2: Run complete verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/test-workspace.sh
cargo llvm-cov --workspace --all-features --lcov --output-path target/llvm-cov/lcov.info
bash scripts/check-coverage-thresholds.sh --lcov target/llvm-cov/lcov.info --base origin/main
```

Expected: all commands exit 0 and changed Rust coverage is 100%.

## Task 7: Document and execute staging-to-production repair

**Files:**
- Modify: `docs/runbooks/bluesky-crosspost-launch-blockers.md`
- External deployment repository: `/Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/{staging,production}/kustomization.yaml`

- [ ] **Step 1: Add exact runbook commands**

Document: build/push immutable divine-atbridge image from the reviewed commit; record registry digest; update staging tag/digest; Argo sync; wait rollout; run expired-session publish fixture; run repair CLI preview in a one-shot Kubernetes Job using existing secret references; record digest and matched full IDs; confirm; verify mapping/AppView/playback; promote the identical digest to production; repeat preview/confirm with bounded scope; rollback only if no repaired job has been claimed.

Never print/decode Kubernetes secrets. The Job imports `DATABASE_URL` by `secretKeyRef` and prints only the CLI's safe JSON.

- [ ] **Step 2: Verify staging before mutation**

```bash
kubectl config current-context
kubectl -n sky get deploy divine-atbridge -o jsonpath='{.spec.template.spec.containers[0].image}{"\n"}'
kubectl -n sky rollout status deploy/divine-atbridge --timeout=5m
kubectl -n sky port-forward service/divine-atbridge 18080:8080
# In a second shell while the port-forward runs:
curl -fsS http://127.0.0.1:18080/health/ready
```

Expected: the first command names the staging cluster context; deployed image
equals the recorded candidate digest; rollout succeeds; readiness is 200. Stop
if the context is not the documented staging cluster—never infer environment
from a namespace because both environments use namespace `sky`.

- [ ] **Step 3: Preview and confirm staging repair**

Run the repair Job first without confirmation. Expected: only the allowlisted full event IDs/exact-error terminal jobs appear. Re-run with the returned operation ID/digest and confirmation. Expected: changed count equals matched count. Watch logs for successful publish without credential output.

- [ ] **Step 4: Verify staging behavior**

For every repaired full event ID, query the bridge mapping/status interface and AppView record; fetch the video playlist/blob with HTTP range support and verify a successful playable response. Re-run preview; expected matched count is zero or only rows already changed since preview and therefore rejected.

- [ ] **Step 5: Promote identical digest and repair production**

Update only the production image reference to the exact staging-verified digest, review the diff, sync, wait rollout, and verify readiness. Preview the three known full IDs plus exact BadJwt-scoped affected jobs with `--max-rows 1000`; inspect counts; confirm once; monitor until mapped/published.

- [ ] **Step 6: Produce the redacted acceptance artifact**

Record commit, immutable image digest, operation IDs, full source event IDs, AT URIs, mapping/AppView status, playback result, and timestamps in the incident runbook. Include no payloads, media URLs, account JWTs, database URLs, or secret values.

- [ ] **Step 7: Commit runbook and deployment changes separately**

In divine-sky:

```bash
git add docs/runbooks/bluesky-crosspost-launch-blockers.md
git commit -m "docs(runbook): add audited crosspost recovery"
```

In divine-iac-coreconfig, after staging evidence and production promotion:

```bash
git add k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml \
        k8s/applications/divine-atbridge/overlays/production/kustomization.yaml
git commit -m "fix(atbridge): promote verified session recovery"
```

## Completion gate

This incident plan is complete only when:

- all workspace, clippy, and 100%-changed-code coverage gates pass;
- staging proves proactive refresh, mapping, AppView visibility, and playback;
- production runs the identical digest;
- the three known missing source IDs and every confirmed exact BadJwt match are mapped and playable or have a newly typed evidence-backed non-auth failure;
- rerunning preview is idempotent and no secret-bearing output was produced;
- rollback remains available only for repaired rows not subsequently claimed.
