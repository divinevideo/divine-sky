# Handle State Startup Reconciliation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `divine-handle-gateway` republish public ATProto handle state for all existing bridged accounts on startup, while keeping manual provision repair paths consistent.

**Architecture:** Extend the current startup replay logic so `pending` rows still re-run provisioning, and non-pending lifecycle rows are reconciled directly from `account_links` into keycast and `divine-name-server`. Keep downstream syncs idempotent and log row-level failures without aborting the process. Preserve the new manual provision route behavior so repairs work immediately and old rows self-heal on restart.

**Tech Stack:** Rust, Axum, Diesel, PostgreSQL, reqwest, mockito, tokio

---

## Chunk 1: Add the Failing Reconciliation Tests

### Task 1: Cover startup reconciliation for preexisting lifecycle rows

**Files:**
- Modify: `crates/divine-handle-gateway/tests/provision_flow.rs`

- [ ] **Step 1: Write the failing ready-row reconciliation test**

Add a test that inserts a preexisting `ready` row, runs the startup reconciliation API, and expects:
- no provisioning request
- one keycast `ready` sync
- one name-server `ready` sync

- [ ] **Step 2: Run the ready-row test to verify it fails**

Run:

```bash
export LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}
cargo test -p divine-handle-gateway startup_reconciliation_republishes_ready_rows -- --nocapture
```

Expected: FAIL because startup reconciliation for ready rows does not exist yet.

- [ ] **Step 3: Write the failing failed-row reconciliation test**

Add a test that inserts a preexisting `failed` row with an error message and expects:
- one keycast `failed` sync carrying the stored error
- one name-server `failed` sync

- [ ] **Step 4: Run the failed-row test to verify it fails**

Run:

```bash
export LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}
cargo test -p divine-handle-gateway startup_reconciliation_republishes_failed_rows -- --nocapture
```

Expected: FAIL because failed-row reconciliation does not exist yet.

- [ ] **Step 5: Write the failing disabled-row reconciliation test**

Add a test that inserts a preexisting `disabled` row and expects:
- one keycast `disabled` sync
- one name-server `disabled` sync

- [ ] **Step 6: Run the disabled-row test to verify it fails**

Run:

```bash
export LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}
cargo test -p divine-handle-gateway startup_reconciliation_republishes_disabled_rows -- --nocapture
```

Expected: FAIL because disabled-row reconciliation does not exist yet.

## Chunk 2: Implement Startup Reconciliation

### Task 2: Load non-pending rows from the database

**Files:**
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Modify: `crates/divine-bridge-db/src/lib.rs`

- [ ] **Step 1: Add a query helper returning lifecycle rows for startup reconciliation**

Create a Diesel query that loads rows where:
- `provisioning_state IN ('ready', 'failed', 'disabled')`

Return full lifecycle rows so the reconciler has `nostr_pubkey`, `handle`, `did`, `provisioning_state`, `provisioning_error`, and `disabled_at`.

- [ ] **Step 2: Run a compile check**

Run:

```bash
cargo check -p divine-bridge-db
```

Expected: PASS.

### Task 3: Reconcile lifecycle rows in the startup runner

**Files:**
- Modify: `crates/divine-handle-gateway/src/provision_runner.rs`
- Modify: `crates/divine-handle-gateway/src/main.rs`

- [ ] **Step 1: Add a `reconcile_existing_from_database()` startup pass**

Implement a method that:
- loads non-pending rows
- dispatches downstream syncs based on stored state
- logs row-level sync failures and continues
- returns the number of rows examined

- [ ] **Step 2: Reuse existing sync semantics**

For each row:
- `ready`: require `did`, then call keycast `sync_ready` and name-server `sync_state_for_handle(..., Some(did), "ready")`
- `failed`: call keycast `sync_failed` with stored error or a fallback message, then sync name-server `failed`
- `disabled`: call keycast `sync_disabled`, then sync name-server `disabled`

- [ ] **Step 3: Run the targeted tests to verify they now pass**

Run:

```bash
export LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}
cargo test -p divine-handle-gateway startup_reconciliation_republishes_ready_rows -- --nocapture
cargo test -p divine-handle-gateway startup_reconciliation_republishes_failed_rows -- --nocapture
cargo test -p divine-handle-gateway startup_reconciliation_republishes_disabled_rows -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Call the reconciler from `main.rs`**

Run the new startup reconciliation after pending replay and log how many rows were processed.

## Chunk 3: Verify the Repair Path Still Works

### Task 4: Keep manual provisioning as an immediate repair path

**Files:**
- Modify: `crates/divine-handle-gateway/tests/control_plane.rs`
- Modify: `crates/divine-handle-gateway/src/routes/provision.rs` if needed

- [ ] **Step 1: Keep the existing manual provision regression test green**

Run:

```bash
export LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}
cargo test -p divine-handle-gateway control_plane_manual_provision_syncs_ready_state_downstream -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Run a focused compile check for the crate**

Run:

```bash
cargo check -p divine-handle-gateway --tests
```

Expected: PASS.

## Chunk 4: Update the Runbook

### Task 5: Document the self-healing startup behavior

**Files:**
- Modify: `docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Document startup reconciliation**

Add a note that `divine-handle-gateway` republishes persisted non-pending lifecycle state on startup so old rows self-heal and public DID resolution is restored without manual per-account repair.

- [ ] **Step 2: Run the focused verification set**

Run:

```bash
export LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${LIBRARY_PATH:-}
export DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/opt/libpq/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}
cargo test -p divine-handle-gateway startup_reconciliation_republishes_ready_rows -- --nocapture
cargo test -p divine-handle-gateway startup_reconciliation_republishes_failed_rows -- --nocapture
cargo test -p divine-handle-gateway startup_reconciliation_republishes_disabled_rows -- --nocapture
cargo test -p divine-handle-gateway control_plane_manual_provision_syncs_ready_state_downstream -- --nocapture
cargo check -p divine-handle-gateway --tests
```

Expected: PASS.
