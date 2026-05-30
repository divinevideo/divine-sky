# Rollout Chunk G — Scheduler Production Polish (divine-sky)

> **Parent plan:** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk G, lines ~280–303). Read it first for context.
>
> **For agentic workers:** Use `superpowers:test-driven-development` for each task (failing test first, then code). Steps use checkbox (`- [ ]`) syntax for tracking. Every command below is real and copy-pasteable; expected output is stated. NO placeholders.

**Editability:** `in-repo-divine-sky`. Every file changed in this chunk lives in `divine-sky`. No sibling repo is touched. (The IAC alerting wiring referenced in G2 Step 6 is a follow-up note, not an edit in this chunk — see "Out of scope".)

**Primary repo:** `divine-sky` (crates `divine-atbridge`, `divine-bridge-db`).

**Goal:** Close two correctness/ops gaps before widening the crosspost cohort:
- **G1:** Make relay-cursor advancement and job enqueue **atomic** in one DB transaction so a crash between the two cannot silently skip an event (idempotency masks this today; make it correct).
- **G2:** Add a watchdog that emits a metric/alert when publish jobs have **expired leases** (`state='in_progress' AND lease_expires_at < NOW()`) or accounts are stuck in `publish_backfill_state='failed'`, surfaced on the existing AT-bridge health server.

---

## Environment & constraints (read before running anything)

- **Diesel 2.2** (`Cargo.toml:18`: `diesel = { version = "2.2", features = ["postgres", "chrono", "serde_json"] }`). Transaction API: `conn.transaction::<T, E, _>(|conn| { ... })` where the closure returns `Result<T, E>` and `E: From<diesel::result::Error>`. Use `diesel::result::Error` as the closure error type and map it at the boundary, OR use `anyhow` via a manual `transaction` that returns `diesel::result::Error` — see the exact pattern in Task G1 Step 3 (we keep the closure error as `diesel::result::Error` and surface `anyhow` outside).
- **The connection is `Arc<Mutex<PgConnection>>`** (`runtime.rs:35`, `pub type SharedConnection`). A transaction must run while holding the lock; do not re-lock inside the closure (would deadlock). Current `enqueue_live_event` locks, does enqueue in a `{}` block, **drops the lock**, then `persist_relay_cursor` **re-locks** — that drop/re-lock is exactly the non-atomic gap.
- **Integration tests require a live Postgres** at `TEST_DATABASE_URL` (default `postgres://divine:divine_dev@[::1]:5432/divine_bridge`, see `tests/runtime_resilience.rs:77-80`). **This planning/CI-less environment has no database.** Therefore the Build phase verifies with:
  - `cargo check -p divine-atbridge -p divine-bridge-db` (type-checks everything)
  - `cargo test -p divine-atbridge --no-run` (compiles tests without executing)
  - The DB-backed assertions (`cargo test -p divine-atbridge runtime_resilience`) run **only where `TEST_DATABASE_URL` points at a reachable Postgres** (dev box / staging job) — call that out in the commit and PR, do not claim green from compile alone.
- **RTK note:** the user's shell rewrites commands through `rtk`. Output of `grep`/`cat` may be reformatted. Use the dedicated Read tool for file inspection; the commands below are still correct to run.

---

## Task G1 — Transactionalize relay-cursor advancement with enqueue

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs` (`enqueue_live_event`, lines ~246–286; and the free fn `persist_relay_cursor`, lines ~188–206, which will be inlined/reused inside the transaction)
- Test: `crates/divine-atbridge/tests/runtime_resilience.rs` (add one DB-backed test; pattern off `runtime_scheduler_persists_cursor_after_enqueue_before_publish_completion`, line ~382)

### Step 1 — Write the failing test (crash between enqueue and cursor must not skip the event)

- [ ] Add a test that proves atomicity: if cursor persistence fails, the enqueued job must NOT be left committed (and vice-versa). Because we cannot literally kill the process mid-call in a unit test, assert the **transactional contract**: a forced failure inside the cursor step rolls back the enqueue, so on replay the event is re-processed (not silently skipped).

Add to `crates/divine-atbridge/tests/runtime_resilience.rs`:

```rust
#[tokio::test]
async fn enqueue_live_event_is_atomic_cursor_and_job_commit_together() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let connection = shared_connection(&database_url);

    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    let event = make_profile_event(&keypair, 1_700_100_040, "Atomic Enqueue");
    {
        let mut conn = connection.lock().unwrap();
        insert_ready_account(
            &mut conn,
            &event.pubkey,
            "did:plc:runtime-atomic",
            "runtime-atomic.divine.video",
        );
    }

    let pipeline = build_runtime_pipeline(
        connection.clone(),
        FlakyPublisher {
            fail_first_write: Mutex::new(false),
            published: Mutex::new(vec![]),
        },
    );

    // Happy path: after a successful enqueue the job AND the cursor are both present.
    enqueue_live_event(&connection, "runtime-atomic-test", &pipeline, &event)
        .await
        .expect("ingest should commit job and cursor together");

    let mut conn = connection.lock().unwrap();
    let job = get_publish_job(&mut conn, &event.id)
        .expect("job lookup should succeed")
        .expect("job should be committed");
    assert_eq!(job.state, "pending");
    let offset = get_ingest_offset(&mut conn, "runtime-atomic-test")
        .expect("cursor lookup should succeed")
        .expect("cursor should be committed in the same transaction as the job");
    assert_eq!(offset.last_event_id, event.id);
    assert_eq!(offset.last_created_at.timestamp(), event.created_at);
}
```

- [ ] **Strengthen the rollback assertion** (the part that actually fails before the fix). Add a second test using a sentinel that makes the cursor step error *after* the enqueue insert, and assert the job is absent (rolled back). Implement the fault injection by passing an **out-of-range timestamp** — `persist_relay_cursor` already errors on `from_timestamp` overflow (`runtime.rs:194`), and the production code must do enqueue + cursor in one transaction so the bad-cursor error rolls the enqueue back:

```rust
#[tokio::test]
async fn enqueue_live_event_rolls_back_job_when_cursor_persist_fails() {
    let _guard = test_db_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let database_url = test_database_url();
    reset_database(&database_url);
    let connection = shared_connection(&database_url);

    let secp = Secp256k1::new();
    let keypair = Keypair::new(&secp, &mut OsRng);
    // i64::MAX seconds is out of chrono range => persist_relay_cursor errors,
    // so the whole transaction (including the enqueue insert) must roll back.
    let event = make_profile_event(&keypair, i64::MAX, "Cursor Overflow");
    {
        let mut conn = connection.lock().unwrap();
        insert_ready_account(
            &mut conn,
            &event.pubkey,
            "did:plc:runtime-rollback",
            "runtime-rollback.divine.video",
        );
    }

    let pipeline = build_runtime_pipeline(
        connection.clone(),
        FlakyPublisher {
            fail_first_write: Mutex::new(false),
            published: Mutex::new(vec![]),
        },
    );

    let result = enqueue_live_event(&connection, "runtime-rollback-test", &pipeline, &event).await;
    assert!(result.is_err(), "out-of-range cursor must fail the call");

    let mut conn = connection.lock().unwrap();
    assert!(
        get_publish_job(&mut conn, &event.id)
            .expect("job lookup should succeed")
            .is_none(),
        "enqueue must roll back when cursor persistence fails in the same transaction"
    );
    assert!(
        get_ingest_offset(&mut conn, "runtime-rollback-test")
            .expect("cursor lookup should succeed")
            .is_none(),
        "no cursor should be committed when the transaction rolls back"
    );
}
```

> Note: the timestamp-validation must move to BEFORE the DB writes inside the transaction so the error path is the rollback path. See Step 3 — `event_timestamp(event.created_at)?` is computed first, then enqueue, then cursor upsert with the already-validated timestamp. The `i64::MAX` test exercises that ordering.

- [ ] **Confirm this test FAILS against current code** (where it compiles — i.e. a box with `TEST_DATABASE_URL`). On the current non-transactional code the rollback test fails because the enqueue is committed before the cursor step errors. If you only have `--no-run` here, state that the failing-test gate is deferred to the DB-enabled box and proceed; do not skip writing it.

### Step 2 — Add a transactional enqueue query helper in `divine-bridge-db` (optional but cleaner)

Two valid shapes. **Pick A** (keeps the transaction boundary in `runtime.rs` where the cursor logic already lives; smallest diff, no new pub API):

- **Shape A (recommended):** Wrap the existing calls in `conn.transaction(...)` directly inside `enqueue_live_event`. No new `divine-bridge-db` function. The existing `enqueue_publish_job`, `cancel_publish_job`, `get_record_mapping`, `upsert_ingest_offset` already take `&mut PgConnection` and compose inside a transaction closure unchanged.
- **Shape B:** Add `divine_bridge_db::enqueue_live_event_tx(conn, ...)` that owns the transaction. More encapsulated but pushes pipeline-shaped args (envelopes, source-name) into the DB crate, which currently has no `runtime`/pipeline types. **Reject B** — it leaks runtime concepts into the data layer.

- [ ] Proceed with Shape A. No `divine-bridge-db` source change required for G1.

### Step 3 — Rewrite `enqueue_live_event` to do enqueue + cursor in ONE transaction

In `crates/divine-atbridge/src/runtime.rs`, replace the body of `enqueue_live_event` (lines ~259–285, everything after `let decision = ...`). Current structure: a `{ let mut conn = lock; match decision {...} }` block that drops the lock, then a separate `persist_relay_cursor(connection, ...)` re-lock. New structure: single lock, single `conn.transaction(...)` containing both the decision match and the cursor upsert.

Exact replacement (the `decision` line above it stays):

```rust
    let decision = pipeline.prepare_publish_job(event).await?;

    // Validate the cursor timestamp BEFORE opening the transaction-affecting
    // writes so a bad timestamp rolls back the enqueue instead of leaving a
    // committed job with no cursor advance.
    let cursor_timestamp = event_timestamp(event.created_at)?;

    let mut conn = connection.lock().unwrap();
    conn.transaction::<(), diesel::result::Error, _>(|conn| {
        match decision {
            QueueDecision::Enqueue(job) => {
                let queued = new_publish_job(&job, PublishJobSource::Live)
                    .map_err(|_| diesel::result::Error::RollbackTransaction)?;
                enqueue_publish_job(conn, &queued)
                    .map_err(to_rollback)?;
            }
            QueueDecision::Cancel {
                target_nostr_event_id,
                tombstone_job,
            } => {
                let tombstone = new_publish_job(&tombstone_job, PublishJobSource::Live)
                    .map_err(|_| diesel::result::Error::RollbackTransaction)?;
                cancel_publish_job(conn, &tombstone, Some("live delete replay"))
                    .map_err(to_rollback)?;

                if get_record_mapping(conn, &target_nostr_event_id)
                    .map_err(to_rollback)?
                    .is_some()
                {
                    let delete_job_envelope = delete_execution_envelope(&tombstone_job)
                        .map_err(|_| diesel::result::Error::RollbackTransaction)?;
                    let delete_job = new_publish_job(&delete_job_envelope, PublishJobSource::Live)
                        .map_err(|_| diesel::result::Error::RollbackTransaction)?;
                    enqueue_publish_job(conn, &delete_job).map_err(to_rollback)?;
                }
            }
            QueueDecision::Skip { .. } => {}
        }

        upsert_ingest_offset(
            conn,
            &UpsertIngestOffset {
                source_name: relay_source_name,
                last_event_id: &event.id,
                last_created_at: cursor_timestamp,
            },
        )
        .map_err(to_rollback)?;

        Ok(())
    })
    .map_err(|error| anyhow::anyhow!("enqueue_live_event transaction failed: {error}"))?;

    Ok(())
```

- [ ] Add a small private helper near the top of `runtime.rs` (below the `const` block) to collapse `anyhow::Error -> diesel::result::Error` (the existing query fns return `anyhow::Result`, so inside the diesel closure we must downcast to the rollback variant — Diesel re-runs the closure error as the transaction error and rolls back):

```rust
/// Collapse an `anyhow` query error into a Diesel transaction error so the
/// transaction rolls back; the original cause is logged by the caller.
fn to_rollback(error: anyhow::Error) -> diesel::result::Error {
    tracing::warn!(error = %error, "rolling back enqueue_live_event transaction");
    diesel::result::Error::RollbackTransaction
}
```

- [ ] **Remove the now-unused `persist_relay_cursor` free function** (lines ~188–206) IF no other caller remains. Verify:

```bash
grep -rn "persist_relay_cursor" crates/divine-atbridge/src crates/divine-atbridge/tests
```
Expected: only the definition (which you then delete) — `run_service` already advances `consumer.last_seen_timestamp` from the relay event, not from this fn. If any other caller exists, keep the fn but have it delegate to the transactional path. Confirm `event_timestamp` (lines ~208–210) and `UpsertIngestOffset` import (already imported at `runtime.rs:11`) remain used.

- [ ] **Imports:** `diesel::Connection` is already imported (`runtime.rs:6`) — that trait provides `.transaction(...)`. No new import needed. `upsert_ingest_offset` and `UpsertIngestOffset` are already imported (lines 11, 16). `anyhow` macro is reachable via `anyhow::anyhow!`.

### Step 4 — Type-check and compile the tests (this environment)

- [ ] Run:

```bash
cargo check -p divine-atbridge -p divine-bridge-db
```
Expected: `Finished` with no errors. Watch specifically for: closure error-type mismatch (must be `diesel::result::Error`), and any "cannot borrow `*conn`" — the closure receives a fresh `&mut PgConnection`, use that, never the outer `connection`/lock.

- [ ] Run:

```bash
cargo test -p divine-atbridge --no-run
```
Expected: `Finished` and `Executable ... (target/debug/deps/runtime_resilience-*)` lines printed — both new tests compiled.

### Step 5 — DB-backed run (only on a box with reachable Postgres; NOT this env)

- [ ] On a dev/staging box where `TEST_DATABASE_URL` resolves:

```bash
export TEST_DATABASE_URL=postgres://divine:divine_dev@127.0.0.1:5432/divine_bridge
cargo test -p divine-atbridge --test runtime_resilience
```
Expected: all `runtime_resilience` tests pass, including `enqueue_live_event_is_atomic_cursor_and_job_commit_together` and `enqueue_live_event_rolls_back_job_when_cursor_persist_fails`. If the rollback test fails, the transaction is not wrapping both writes — re-check Step 3.

### Step 6 — Commit

- [ ] 
```bash
git add crates/divine-atbridge/src/runtime.rs crates/divine-atbridge/tests/runtime_resilience.rs
git commit -m "fix(atbridge): commit relay cursor and publish enqueue in one transaction"
```

---

## Task G2 — Lease-expiry + failed-backfill watchdog metric/alert

The AT-bridge health surface is `crates/divine-atbridge/src/health.rs` — an Axum router exposing `/health` and `/health/ready` (lines ~159–169, 225–230). There is **no `/metrics` endpoint and no Prometheus dependency** today (`grep` confirmed). The smallest correct addition is a JSON watchdog endpoint backed by two count queries, returning non-200 (or an `alert: true` flag) when either count is > 0, so the existing alerting stack can poll it. Do NOT add a Prometheus crate in this chunk unless the IAC alert stack scrapes Prometheus (confirm in Step 6 / follow-up); JSON-on-health matches the current surface.

**Files:**
- Add query: `crates/divine-bridge-db/src/queries.rs` (two count fns + one `QueryableByName` count row)
- Modify: `crates/divine-atbridge/src/health.rs` (add `/health/watchdog` route + handler; thread a `SharedConnection` or a `BridgeConfig`-derived connection into `InternalApiState`)
- Test: `crates/divine-bridge-db` unit test (or `crates/divine-atbridge/tests/`) — DB-backed, behind `TEST_DATABASE_URL`

### Step 1 — Add the two count queries in `divine-bridge-db`

In `crates/divine-bridge-db/src/queries.rs` add (the `Int8` sql type is already imported at line 9; `sql_query` at line 8):

```rust
#[derive(Debug, QueryableByName)]
struct CountRow {
    #[diesel(sql_type = Int8)]
    count: i64,
}

/// Count publish jobs whose lease has expired while still marked in_progress.
/// A non-zero value means a worker died mid-job and the lease watchdog should fire.
pub fn count_expired_publish_leases(conn: &mut PgConnection) -> Result<i64> {
    let row = sql_query(
        "SELECT COUNT(*) AS count
         FROM publish_jobs
         WHERE state = 'in_progress'
           AND lease_expires_at IS NOT NULL
           AND lease_expires_at < NOW()",
    )
    .get_result::<CountRow>(conn)?;
    Ok(row.count)
}

/// Count account links whose backfill is stuck in the 'failed' terminal state.
pub fn count_failed_backfill_accounts(conn: &mut PgConnection) -> Result<i64> {
    let row = sql_query(
        "SELECT COUNT(*) AS count
         FROM account_links
         WHERE publish_backfill_state = 'failed'",
    )
    .get_result::<CountRow>(conn)?;
    Ok(row.count)
}
```

- [ ] These are exported automatically via `pub use queries::*;` (`lib.rs:10`). The column names (`state`, `lease_expires_at`, `publish_backfill_state`) match migration `004_publish_job_scheduler/up.sql` (lines 1, 16–17) and `schema.rs`.

### Step 2 — Failing DB-backed test for the counts

Add `crates/divine-bridge-db/tests/watchdog_counts.rs` (mirror the reset/connect pattern from `runtime_resilience.rs:82-118`; reuse migrations 001 + 004):

```rust
// Behind TEST_DATABASE_URL; skipped where no Postgres is reachable.
// Asserts:
//  - fresh DB => count_expired_publish_leases == 0 && count_failed_backfill_accounts == 0
//  - after forcing one job to state='in_progress', lease_expires_at = NOW() - INTERVAL '1 second'
//      => count_expired_publish_leases == 1
//  - after setting one account_links row publish_backfill_state='failed'
//      => count_failed_backfill_accounts == 1
```

- [ ] Implement the three assertions with `diesel::sql_query(...).execute(conn)` UPDATEs exactly like `runtime_resilience.rs:604-613` forces an expired lease. Use a unique `nostr_pubkey`/`nostr_event_id` per row.

### Step 3 — Add the watchdog endpoint to the health server

In `crates/divine-atbridge/src/health.rs`:

- [ ] Add a connection handle to `InternalApiState`. The health server today does not hold a `SharedConnection`; add an optional one:

```rust
#[derive(Clone, Default)]
struct InternalApiState {
    runtime: RuntimeHealthState,
    expected_bearer: Option<String>,
    provisioner: Option<Arc<dyn ProvisioningService>>,
    watchdog_conn: Option<crate::runtime::SharedConnection>, // new
}
```

- [ ] Add the response type + handler:

```rust
#[derive(Debug, serde::Serialize)]
struct WatchdogResponse {
    expired_leases: i64,
    failed_backfills: i64,
    alert: bool,
}

async fn health_watchdog(
    State(state): State<InternalApiState>,
) -> Result<(StatusCode, Json<WatchdogResponse>), StatusCode> {
    let Some(conn) = state.watchdog_conn.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let (expired_leases, failed_backfills) = {
        let mut conn = conn.lock().unwrap();
        let expired = divine_bridge_db::count_expired_publish_leases(&mut conn)
            .map_err(|error| {
                tracing::error!(error = %error, "watchdog lease count failed");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let failed = divine_bridge_db::count_failed_backfill_accounts(&mut conn)
            .map_err(|error| {
                tracing::error!(error = %error, "watchdog backfill count failed");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        (expired, failed)
    };
    let alert = expired_leases > 0 || failed_backfills > 0;
    // 200 when clean, 503 when an alert condition is live so a uptime/HTTP probe
    // in the alerting stack fires without parsing the body.
    let status = if alert { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
    Ok((
        status,
        Json(WatchdogResponse { expired_leases, failed_backfills, alert }),
    ))
}
```

- [ ] Register the route in `app_with_state` (line ~225, alongside `/health/ready`):

```rust
    Router::new()
        .route("/health", get(health))
        .route("/health/ready", get(health_ready))
        .route("/health/watchdog", get(health_watchdog))
        .merge(protected)
        .with_state(state)
```

- [ ] Thread the connection in. `spawn(...)` (line ~270) and `run_service_with_state` (`runtime.rs:469`) both already build a `SharedConnection`. Add a `watchdog_conn` arg to `spawn` (or build a fresh `establish_connection(&config.database_url)` inside `spawn`) and set `watchdog_conn: Some(conn)` in the `InternalApiState` there. For `app()` / `app_with_runtime_state` / `app_with_config` constructors that have no live DB, leave `watchdog_conn: None` (endpoint returns 503 — acceptable; only the running service serves it). Update those three constructors and the two `InternalApiState { ... }` literals (lines ~237, ~263, ~279) to include `watchdog_conn: None` / `Some(...)` so the struct-literal still compiles.

- [ ] **Dependency check:** `divine-atbridge` already depends on `divine-bridge-db` (it uses `divine_bridge_db::...` throughout `runtime.rs`). `serde` is available (used in `health.rs` for `Deserialize`/`Serialize`). No new crate.

### Step 4 — Type-check and compile

- [ ] Run:

```bash
cargo check -p divine-atbridge -p divine-bridge-db
```
Expected: `Finished`. Common failures to fix: every `InternalApiState { ... }` struct literal must now set `watchdog_conn` (the `#[derive(Default)]` covers `..Default::default()` sites but explicit literals do not).

- [ ] Run:

```bash
cargo test -p divine-atbridge -p divine-bridge-db --no-run
```
Expected: `Finished` with `Executable` lines for `watchdog_counts` and existing suites.

### Step 5 — DB-backed run (box with Postgres only)

- [ ] 
```bash
export TEST_DATABASE_URL=postgres://divine:divine_dev@127.0.0.1:5432/divine_bridge
cargo test -p divine-bridge-db --test watchdog_counts
```
Expected: pass — clean DB reports 0/0, a forced expired lease reports `expired_leases == 1`, a `failed` backfill reports `failed_backfills == 1`.

### Step 6 — Wire the alert + commit

- [ ] **Wire into the existing alerting stack.** This is a follow-up edit in the IAC repo (`../divine-iac-coreconfig`), which is OUT OF SCOPE for this in-repo chunk — do not edit it here. Instead, record the contract in this repo's launch checklist so the IAC owner can add the probe: add a line to `docs/runbooks/launch-checklist.md` under the Safety section:
  - `GET https://<atbridge-health-host>/health/watchdog` returns 200 when clean, 503 + `{expired_leases, failed_backfills, alert:true}` when a lease is stuck or a backfill failed; alerting stack must page on the 503.
- [ ] Commit:

```bash
git add crates/divine-bridge-db/src/queries.rs \
        crates/divine-bridge-db/tests/watchdog_counts.rs \
        crates/divine-atbridge/src/health.rs \
        docs/runbooks/launch-checklist.md
git commit -m "feat(atbridge): add lease-expiry and failed-backfill watchdog endpoint"
```

---

## Build phase — exact files

**Edited / created in this chunk (all in divine-sky):**
- `crates/divine-atbridge/src/runtime.rs` — G1: wrap enqueue + cursor in `conn.transaction(...)`; add `to_rollback`; remove/retire `persist_relay_cursor`.
- `crates/divine-atbridge/tests/runtime_resilience.rs` — G1: two new DB-backed tests (`enqueue_live_event_is_atomic_...`, `enqueue_live_event_rolls_back_job_when_cursor_persist_fails`).
- `crates/divine-bridge-db/src/queries.rs` — G2: `CountRow`, `count_expired_publish_leases`, `count_failed_backfill_accounts`.
- `crates/divine-bridge-db/tests/watchdog_counts.rs` — G2: count assertions (new file).
- `crates/divine-atbridge/src/health.rs` — G2: `WatchdogResponse`, `health_watchdog`, `/health/watchdog` route, `watchdog_conn` on `InternalApiState`, thread connection in `spawn`.
- `docs/runbooks/launch-checklist.md` — G2: watchdog probe contract under Safety.

**Read-only references (do not edit):**
- `crates/divine-bridge-db/src/schema.rs`, `crates/divine-bridge-db/src/models.rs` (column names)
- `migrations/004_publish_job_scheduler/up.sql` (lease/backfill columns)
- `crates/divine-atbridge/src/lib.rs` (export surface)

**Verification commands (this env — no DB):**
```bash
cargo check -p divine-atbridge -p divine-bridge-db
cargo test -p divine-atbridge -p divine-bridge-db --no-run
```
**Verification commands (DB-enabled box only):**
```bash
export TEST_DATABASE_URL=postgres://divine:divine_dev@127.0.0.1:5432/divine_bridge
cargo test -p divine-atbridge --test runtime_resilience
cargo test -p divine-bridge-db --test watchdog_counts
```

---

## Out of scope (do not do in this chunk)
- IAC alert-rule wiring in `../divine-iac-coreconfig` (cross-repo; only document the probe contract here).
- Prometheus/OpenMetrics exporter (JSON-on-health matches the current surface; add only if IAC confirms a scrape model).
- Rate limits / DMCA intake — that is Chunk H.
- Any change to the claim/lease SQL in `claim_next_job` — the watchdog only *observes* expired leases; reclaim already works (`runtime_scheduler_reclaims_expired_worker_leases` passes).

## Risks
- **Deadlock from double-locking the `Mutex<PgConnection>`.** The transaction closure must use the `&mut PgConnection` Diesel hands it, never re-lock `connection`. The G1 rewrite holds exactly one lock for the whole transaction.
- **Closure error type.** `conn.transaction::<_, diesel::result::Error, _>` requires the closure's `Err` to be `diesel::result::Error`; the `to_rollback` helper bridges `anyhow` query errors. Getting this wrong is the most likely `cargo check` failure.
- **Compile-only confidence.** This env cannot run the DB tests; the atomicity/rollback guarantee is only *proven* on a box with `TEST_DATABASE_URL`. The PR/commit must say "DB tests deferred to dev box," not claim green from `--no-run`.
- **Struct-literal breakage.** Adding `watchdog_conn` to `InternalApiState` breaks every explicit `InternalApiState { ... }` literal until updated — there are ~3 (lines ~237, ~263, ~279 of `health.rs`).
