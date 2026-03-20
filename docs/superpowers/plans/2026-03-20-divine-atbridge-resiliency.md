# divine-atbridge Resiliency Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `divine-atbridge` survive routine relay and ATProto/PDS failures without exiting, while exposing degraded readiness after sustained failure.

**Architecture:** Keep the existing worker and health server structure, but add shared runtime health state and change the relay loop from fail-fast to fail-soft. Relay connect/read errors trigger bounded reconnects, malformed frames are skipped, per-event processing failures are logged and skipped, and readiness reflects sustained degradation instead of unconditional success.

**Tech Stack:** Rust, Tokio, Axum, tokio-tungstenite, Diesel, existing `divine-atbridge` relay and pipeline abstractions.

---

## Chunk 1: Lock Failure Behavior With Tests

### Task 1: Add runtime tests for fail-soft relay and processing behavior

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/tests/runtime_health.rs`
- Create or modify: `crates/divine-atbridge/tests/runtime_resilience.rs`

- [ ] **Step 1: Write failing tests for runtime resiliency**

Add tests covering:

- relay connect failure updates runtime state instead of exiting the process immediately
- malformed relay frame is skipped and later frames can still be processed
- event processing failure does not terminate the runtime loop
- readiness remains healthy for transient failure and flips after sustained degradation

- [ ] **Step 2: Run the focused runtime tests and verify they fail**

Run:

```bash
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge --test runtime_health -- --nocapture
```

Expected: FAIL because runtime state and fail-soft loop behavior do not exist yet.

## Chunk 2: Add Shared Runtime State And Readiness Logic

### Task 2: Introduce runtime status tracking

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/src/health.rs`
- Modify: `crates/divine-atbridge/src/main.rs`
- Modify: `crates/divine-atbridge/src/lib.rs`

- [ ] **Step 1: Add a shared runtime status model**

Track:

- last successful relay connection
- last successful event processing
- last error summary
- consecutive relay failures
- consecutive processing failures
- degraded readiness flag

- [ ] **Step 2: Thread runtime status into the health app**

`/health` remains liveness-only.

`/health/ready` should evaluate runtime state rather than always returning `200 OK`.

- [ ] **Step 3: Run focused tests**

Run:

```bash
cargo test -p divine-atbridge --test runtime_health -- --nocapture
```

Expected: readiness tests move closer to green, runtime resilience tests still fail until the loop changes.

## Chunk 3: Make The Worker Loop Fail-Soft

### Task 3: Convert relay connect/read/parse/process failures into retry or skip behavior

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/src/nostr_consumer.rs`

- [ ] **Step 1: Implement bounded reconnect behavior for relay connect failures**

Use a simple bounded backoff and keep retrying.

- [ ] **Step 2: Convert relay read failures into reconnect behavior**

Drop the current session, mark degraded, and retry.

- [ ] **Step 3: Convert parse failures into per-frame skip behavior**

Log and continue instead of returning `Err`.

- [ ] **Step 4: Convert pipeline processing failures into per-event skip behavior**

Log the failed event and continue without exiting the process.

- [ ] **Step 5: Verify focused tests**

Run:

```bash
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge --test runtime_health -- --nocapture
```

Expected: PASS.

## Chunk 4: Final Verification

### Task 4: Verify the touched crate and summarize residual risk

**Files:**
- Modify if needed after verification: `crates/divine-atbridge/src/runtime.rs`
- Modify if needed after verification: `crates/divine-atbridge/src/health.rs`
- Modify if needed after verification: `crates/divine-atbridge/tests/runtime_health.rs`
- Modify if needed after verification: `crates/divine-atbridge/tests/runtime_resilience.rs`

- [ ] **Step 1: Run crate verification**

Run:

```bash
cargo test -p divine-atbridge --test runtime_health -- --nocapture
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge --test provision_api -- --nocapture
cargo check -p divine-atbridge --all-targets
```

- [ ] **Step 2: Review residual risk**

Confirm what still remains intentionally out of scope:

- no multi-replica consumer coordination
- no durable retry queue for failed events
- readiness reflects sustained degradation, not per-event perfection
