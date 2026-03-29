# Oldest-First Crosspost Scheduler Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn ATProto crossposting into a durable scheduler that enqueues live relay events, seeds migrated-user backlog jobs oldest first, and lets new live posts publish immediately ahead of backlog work.

**Architecture:** Expand `publish_jobs` into the real scheduler, add durable backlog state to `account_links`, split relay ingest from publish execution, add a backlog planner that seeds historical jobs per eligible account, and run a worker loop that prioritizes `live` jobs before oldest-first `backfill` jobs.

**Tech Stack:** Rust, Tokio, Axum, Diesel, PostgreSQL, Nostr relay WebSocket

---

## Chunk 1: Durable Scheduler Substrate

### Task 1: Extend the database schema for queueing and backlog state

**Files:**
- Create: `migrations/004_publish_job_scheduler/up.sql`
- Create: `migrations/004_publish_job_scheduler/down.sql`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-types/src/lib.rs`
- Modify: `crates/divine-atbridge/tests/bridge_opt_in_gate.rs`
- Modify: `crates/divine-atbridge/tests/provision_api.rs`

- [ ] **Step 1: Write the failing schema/query test cases**

Add tests that expect:

- `publish_jobs` rows to store `nostr_pubkey`, `event_created_at`, `job_source`, lease fields, and completion fields
- `account_links` rows to store `publish_backfill_state`, `publish_backfill_started_at`, `publish_backfill_completed_at`, and `publish_backfill_error`

Suggested new test file:

```rust
// crates/divine-atbridge/tests/publish_queue_scheduler.rs
#[test]
fn placeholder_schema_contract_for_scheduler_columns() {
    panic!("write DB-backed assertions before adding implementation");
}
```

- [ ] **Step 2: Add the migration**

In `migrations/004_publish_job_scheduler/up.sql`, add:

```sql
ALTER TABLE publish_jobs
  ADD COLUMN nostr_pubkey TEXT,
  ADD COLUMN event_created_at TIMESTAMPTZ,
  ADD COLUMN job_source TEXT NOT NULL DEFAULT 'live',
  ADD COLUMN lease_owner TEXT,
  ADD COLUMN lease_expires_at TIMESTAMPTZ,
  ADD COLUMN completed_at TIMESTAMPTZ;

ALTER TABLE account_links
  ADD COLUMN publish_backfill_state TEXT NOT NULL DEFAULT 'not_started',
  ADD COLUMN publish_backfill_started_at TIMESTAMPTZ,
  ADD COLUMN publish_backfill_completed_at TIMESTAMPTZ,
  ADD COLUMN publish_backfill_error TEXT;
```

Add indexes for:

- pending/claimable jobs by `job_source` plus `event_created_at`
- expired leases
- eligible backlog scan on `account_links`

- [ ] **Step 3: Update Diesel models and shared enums**

Add model fields in:

- `crates/divine-bridge-db/src/models.rs`
- `crates/divine-bridge-types/src/lib.rs`

Expected additions:

```rust
pub enum PublishJobSource {
    Live,
    Backfill,
}
```

Also update any manual `AccountLinkLifecycleRow` construction in `crates/divine-atbridge/tests/bridge_opt_in_gate.rs` and any DB reset helpers in `crates/divine-atbridge/tests/provision_api.rs` so they include the new lifecycle shape and apply migration `004` during test setup.

- [ ] **Step 4: Run the focused compile check**

Run: `cargo check -p divine-bridge-db -p divine-bridge-types -p divine-atbridge`

Expected: pass for the targeted crates.

- [ ] **Step 5: Commit**

```bash
git add migrations/004_publish_job_scheduler/up.sql migrations/004_publish_job_scheduler/down.sql crates/divine-bridge-db/src/schema.rs crates/divine-bridge-db/src/models.rs crates/divine-bridge-types/src/lib.rs crates/divine-atbridge/tests/bridge_opt_in_gate.rs crates/divine-atbridge/tests/provision_api.rs
git commit -m "feat: add publish scheduler schema"
```

### Task 2: Add queue and backlog query APIs

**Files:**
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Modify: `crates/divine-bridge-db/src/lib.rs`
- Test: `crates/divine-atbridge/tests/publish_queue_scheduler.rs`

- [ ] **Step 1: Write failing queue query tests**

Cover:

- enqueue is idempotent on `nostr_event_id`
- live jobs claim ahead of backfill jobs
- backfill jobs claim oldest first by `event_created_at`
- expired leases become claimable again
- eligible backlog accounts load in account creation order

Run: `cargo test -p divine-atbridge --test publish_queue_scheduler -- --nocapture`

Expected: FAIL because query APIs do not exist yet.

- [ ] **Step 2: Add scheduler query functions**

Implement query functions such as:

```rust
pub fn enqueue_publish_job(...)
pub fn claim_next_publish_job(...)
pub fn mark_publish_job_completed(...)
pub fn mark_publish_job_failed(...)
pub fn cancel_publish_job(...)
pub fn get_publish_job(...)
pub fn list_accounts_requiring_backfill(...)
pub fn mark_account_backfill_started(...)
pub fn mark_account_backfill_completed(...)
pub fn mark_account_backfill_failed(...)
```

Ordering rule inside `claim_next_publish_job`:

1. claim `live` jobs first
2. otherwise claim `backfill` jobs by `event_created_at ASC`

Do not rely on `record_mappings` alone for dedupe. Queue-aware code must treat an already-enqueued `publish_jobs` row as sufficient evidence that a source event is already in flight.

- [ ] **Step 3: Export the new query surface**

Update `crates/divine-bridge-db/src/lib.rs` so runtime code can call the new queue/backfill APIs.

- [ ] **Step 4: Re-run the queue tests**

Run: `cargo test -p divine-atbridge --test publish_queue_scheduler -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/divine-bridge-db/src/queries.rs crates/divine-bridge-db/src/lib.rs crates/divine-atbridge/tests/publish_queue_scheduler.rs
git commit -m "feat: add publish scheduler queries"
```

## Chunk 2: Runtime Split And Backfill Planner

### Task 3: Split pipeline logic into enqueue inspection and job execution

**Files:**
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/lib.rs`
- Test: `crates/divine-atbridge/tests/publish_path_integration.rs`
- Test: `crates/divine-atbridge/tests/replay_and_delete.rs`

- [ ] **Step 1: Write failing pipeline tests**

Add tests that prove:

- live ingest can classify an event as enqueueable without publishing inline
- worker execution still performs the existing publish path
- delete events can cancel a queued publish before any `record_mapping` exists

Run: `cargo test -p divine-atbridge publish_path_integration -- --nocapture`

Expected: FAIL because the pipeline still publishes inline.

- [ ] **Step 2: Extract a queue-facing inspection phase**

Create or refactor methods so `pipeline.rs` exposes:

```rust
pub async fn prepare_publish_job(&self, event: &NostrEvent) -> Result<QueueDecision>;
pub async fn execute_publish_job(&self, job: &PublishJobEnvelope) -> Result<ProcessResult>;
```

`prepare_publish_job` must stop before blob upload or PDS writes.

- [ ] **Step 3: Update `run_bridge_session` helpers to use enqueue semantics**

Modify `crates/divine-atbridge/src/lib.rs` so helper flows match the runtime split and do not diverge from production behavior.

- [ ] **Step 4: Re-run the pipeline slices**

Run:

```bash
cargo test -p divine-atbridge --test publish_path_integration -- --nocapture
cargo test -p divine-atbridge --test replay_and_delete -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/src/pipeline.rs crates/divine-atbridge/src/lib.rs crates/divine-atbridge/tests/publish_path_integration.rs crates/divine-atbridge/tests/replay_and_delete.rs
git commit -m "refactor: split enqueue and publish execution"
```

### Task 4: Add the backlog planner and author-history relay scan

**Files:**
- Create: `crates/divine-atbridge/src/backfill_planner.rs`
- Modify: `crates/divine-atbridge/src/nostr_consumer.rs`
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/src/config.rs`
- Test: `crates/divine-atbridge/tests/backfill_planner.rs`

- [ ] **Step 1: Write failing planner tests**

Add tests that expect:

- an eligible ready+enabled account is discovered for backlog seeding
- historical events for that author are enqueued as `backfill`
- the planner marks backlog complete on `EOSE`
- repeated planner runs do not duplicate jobs

Run: `cargo test -p divine-atbridge --test backfill_planner -- --nocapture`

Expected: FAIL because the planner module does not exist yet.

- [ ] **Step 2: Add author-history relay support**

Extend `crates/divine-atbridge/src/nostr_consumer.rs` with an author-scoped filter or helper such as:

```rust
pub fn author_history_filter(author: String) -> NostrFilter
pub async fn collect_history_until_eose(...)
```

This flow should read stored historical events for a single author and stop once `EOSE` arrives.

- [ ] **Step 3: Implement the backlog planner**

In `crates/divine-atbridge/src/backfill_planner.rs`, implement:

- load eligible accounts from the DB
- mark backlog started
- scan author history
- enqueue eligible historical publish events as `backfill`
- mark backlog complete or failed

- [ ] **Step 4: Wire planner configuration and runtime invocation**

Add only the minimum configuration required. Prefer module constants unless an env var is clearly necessary. If added, keep it to planner poll interval or lease duration only.

Run: `cargo test -p divine-atbridge --test backfill_planner -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/src/backfill_planner.rs crates/divine-atbridge/src/nostr_consumer.rs crates/divine-atbridge/src/runtime.rs crates/divine-atbridge/src/config.rs crates/divine-atbridge/tests/backfill_planner.rs
git commit -m "feat: add oldest-first backlog planner"
```

## Chunk 3: Worker Arbitration, Runtime Wiring, And Verification

### Task 5: Replace inline publishing with the scheduler worker

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/src/health.rs`
- Modify: `crates/divine-atbridge/src/main.rs`
- Test: `crates/divine-atbridge/tests/runtime_resilience.rs`
- Test: `crates/divine-atbridge/tests/e2e_local_stack.rs`

- [ ] **Step 1: Write failing runtime tests**

Add tests that prove:

- the relay cursor advances after enqueue persistence, not after publish completion
- live jobs publish ahead of queued backfill jobs
- worker lease recovery retries abandoned jobs
- end-to-end runtime still publishes and deletes successfully

Run:

```bash
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge --test e2e_local_stack -- --nocapture
```

Expected: FAIL because runtime still publishes inline.

- [ ] **Step 2: Add a publish worker loop**

In `crates/divine-atbridge/src/runtime.rs`, run three responsibilities:

- live relay ingest loop that only enqueues
- backlog planner loop
- publish worker loop that claims jobs and calls `execute_publish_job`

Cursor rule:

```rust
persist_relay_cursor(...) only after enqueue_publish_job(...) commits
```

- [ ] **Step 3: Update health and main wiring**

Ensure health only reflects real runtime failures and the new worker/planner loops are launched from the normal service entrypoint.

- [ ] **Step 4: Re-run the runtime slices**

Run:

```bash
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge --test e2e_local_stack -- --nocapture
cargo check -p divine-atbridge -p divine-bridge-db -p divine-bridge-types
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/src/runtime.rs crates/divine-atbridge/src/health.rs crates/divine-atbridge/src/main.rs crates/divine-atbridge/tests/runtime_resilience.rs crates/divine-atbridge/tests/e2e_local_stack.rs
git commit -m "feat: run crosspost publishing through scheduler"
```

### Task 6: Final regression, docs, and rollout notes

**Files:**
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Modify: `docs/runbooks/atproto-opt-in-smoke-test.md`
- Modify: `docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Write the doc expectations**

Document:

- live posts are queue-backed
- backlog publishes oldest first
- live jobs may overtake backlog jobs
- delete events can cancel queued backlog work before publish
- workspace-wide `cargo check --workspace` is currently blocked by an unrelated `divine-feedgen` baseline issue if it is still present

- [ ] **Step 2: Update runbooks**

Add operational notes for:

- how to identify stuck leased jobs
- how to identify a user with `publish_backfill_state = failed`
- what ordering guarantee operators should expect

- [ ] **Step 3: Run the focused verification suite**

Run:

```bash
cargo test -p divine-atbridge --test publish_queue_scheduler -- --nocapture
cargo test -p divine-atbridge --test backfill_planner -- --nocapture
cargo test -p divine-atbridge --test publish_path_integration -- --nocapture
cargo test -p divine-atbridge --test replay_and_delete -- --nocapture
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge --test e2e_local_stack -- --nocapture
cargo check -p divine-atbridge -p divine-bridge-db -p divine-bridge-types
```

Expected: PASS

- [ ] **Step 4: If the workspace baseline is fixed, run the full workspace check**

Run: `cargo check --workspace`

Expected: PASS. If it still fails in unrelated crates, capture that separately instead of folding it into this feature.

- [ ] **Step 5: Commit**

```bash
git add docs/runbooks/dev-bootstrap.md docs/runbooks/launch-checklist.md docs/runbooks/atproto-opt-in-smoke-test.md docs/runbooks/login-divine-video.md
git commit -m "docs: record publish scheduler behavior"
```

Plan complete and saved to `docs/superpowers/plans/2026-03-29-oldest-first-crosspost-scheduler.md`. Ready to execute?
