# divine-atbridge Resiliency Design

**Date:** 2026-03-20
**Status:** Approved

## Purpose

Harden `divine-atbridge` so routine upstream failures and bad input do not terminate the worker process. The bridge must keep trying to consume Nostr events, keep serving liveness/readiness endpoints, and make operator-visible state available without becoming easy to crash through bad relay behavior or per-event publish failures.

## Problem

The current worker loop is fail-fast:

- initial Nostr relay connect failure exits the process
- relay read and parse failures exit the process
- per-event processing failures, including PDS write failures, exit the process
- Kubernetes readiness does not reflect relay health and always returns success

That behavior is brittle. A bad relay URL, a flaky relay, malformed relay frames, or a single bad upstream ATProto write should not take the service down.

## Goals

- Keep the process alive through routine relay and upstream failures
- Keep retrying Nostr relay connectivity with backoff
- Skip bad relay frames instead of terminating the worker
- Log per-event processing failures and continue with later events
- Expose degraded readiness after sustained upstream failure
- Preserve the existing singleton deployment model

## Non-Goals

- Multi-replica relay-consumer coordination
- New external APIs
- Reworking the bridge pipeline semantics beyond failure handling
- Turning the worker into a queue-driven system

## Failure Policy

### Relay connect failures

If the websocket connection to the Nostr relay fails:

- log the error with relay URL and attempt count
- mark runtime state as degraded
- sleep with bounded backoff
- retry indefinitely

This must not terminate the process.

### Relay read failures

If the websocket read fails after a connection is established:

- log the error
- mark runtime state as degraded
- close or drop the current relay session
- reconnect with bounded backoff

This must not terminate the process.

### Relay parse failures

If a single relay frame cannot be parsed:

- log the raw-frame failure at warning or error level
- increment an error counter
- skip that frame
- continue reading subsequent frames on the same connection

This must not terminate the process or force reconnect by itself.

### Per-event processing failures

If one event fails pipeline processing, including ATProto/PDS writes:

- log the event id, author pubkey when available, and error summary
- increment the runtime error state
- do not advance the replay cursor for that failed event
- continue processing later frames

This must not terminate the process.

### Core dependency failures

If runtime initialization fails before the loop starts, startup may still fail. Once the worker has started, runtime dependency failures should transition readiness to degraded before process exit is considered. The worker should strongly prefer retry over exiting.

## Runtime State Model

Add shared runtime state owned by the worker loop and read by the health server.

State to track:

- `started_at`
- `connected_at`
- `last_event_at`
- `last_successful_event_at`
- `last_error`
- `consecutive_relay_failures`
- `consecutive_processing_failures`
- `degraded`

This state is internal-only and exists to drive logging and readiness.

## Health Contract

### `GET /health`

Liveness only.

- returns `200 OK` if the process and HTTP server are running
- does not depend on relay connectivity

### `GET /health/ready`

Readiness for operational routing.

- returns `200 OK` when the runtime has not crossed the sustained-failure threshold
- returns non-`200` when the worker has sustained relay or core dependency failures long enough to be considered degraded
- a single bad frame or one failed event must not flip readiness to failed

Initial recommended threshold:

- relay unavailable for several consecutive attempts, or
- repeated core processing failures without recent success

Exact thresholds can be conservative and simple in v1.

## Logging

Log enough information to debug without crashing:

- relay connect failures with URL and retry delay
- relay read failures
- parse failures with concise frame context
- processing failures with event id and stage summary
- readiness degradation transitions
- recovery transitions when relay connectivity or successful processing resumes

## Testing

Add focused runtime tests for:

- relay connect failure does not terminate the loop immediately
- malformed relay frame is skipped and the loop continues
- per-event processing failure is logged and the loop continues
- readiness stays live for transient failures and flips only after sustained degradation

Keep tests focused on behavior rather than internal implementation details.

## Implementation Shape

Prefer small additions over a full runtime rewrite:

- introduce shared runtime state in `runtime.rs` or a small adjacent module
- pass that state to the health app
- refactor the loop so fatal `?` paths become controlled retry or skip paths
- keep the current pipeline and relay abstractions where possible

## Verification

- focused `divine-atbridge` runtime tests for the new failure modes
- `cargo check -p divine-atbridge --all-targets`
- confirm health endpoints still pass existing tests

## Deferred

- metrics export for the runtime state
- operator-visible admin status endpoint beyond `/health/ready`
- dead-letter handling for repeatedly failing events
- durable retry queues for failed event application
