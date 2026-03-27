# Handle State Startup Reconciliation

**Date:** 2026-03-28
**Status:** Approved

## Purpose

Ensure every bridged ATProto account eventually republishes its public handle state, even if the original provisioning flow skipped downstream syncs or a prior deploy wrote the database row without updating the public name read model.

## Problem

Some accounts can reach `account_links.provisioning_state = "ready"` and publish successfully while still failing public handle resolution. In the observed staging case:

- the PLC document already advertised `at://heybob.divine.video`
- the PDS repo existed and accepted posts
- `https://heybob.divine.video/.well-known/atproto-did` still returned `404`

This means the public name read model was stale, not the DID or repo.

## Decision

`divine-handle-gateway` will reconcile persisted lifecycle rows on startup.

- `pending` rows keep the existing provisioning replay path
- `ready` rows republish `ready` to keycast and the name server
- `failed` rows republish `failed` to keycast and the name server
- `disabled` rows republish `disabled` to keycast and the name server

The manual `POST /api/account-links/provision` path must also sync `ready` state immediately so new repair actions do not depend on a future restart.

## Components

- **`crates/divine-handle-gateway/src/provision_runner.rs`**
  Add startup reconciliation for non-pending lifecycle rows and reuse the existing downstream sync clients.
- **`crates/divine-handle-gateway/src/main.rs`**
  Run the reconciliation pass during boot, alongside pending replay.
- **`crates/divine-handle-gateway/src/routes/provision.rs`**
  Keep manual/admin provisioning consistent with startup and async provisioning by syncing `ready` state downstream.
- **`crates/divine-bridge-db/src/queries.rs`**
  Add a query for loading lifecycle rows that need reconciliation.

## Data Flow

```
gateway startup
    -> load account_links where state in (ready, failed, disabled)
    -> for each row:
         ready    -> keycast ready + name-server ready
         failed   -> keycast failed + name-server failed
         disabled -> keycast disabled + name-server disabled
    -> log per-row errors, continue processing remaining rows

pending startup path
    -> existing replay_pending_from_database()
    -> call divine-atbridge /provision
    -> mark ready/failed
    -> sync downstream
```

## Constraints

- Startup reconciliation must be idempotent.
- One bad row must not abort the entire boot sequence.
- Reconciliation should operate from durable `account_links` state only; no PLC or PDS probing is required.
- Public resolution remains served by `divine-router` from `divine-name-server`; `divine-handle-gateway` only republishes state.

## Test Coverage

- startup reconciliation republishes a preexisting `ready` row
- startup reconciliation republishes a preexisting `failed` row
- startup reconciliation republishes a preexisting `disabled` row
- manual `/api/account-links/provision` continues to sync `ready` immediately

## Deferred

- periodic background reconciliation beyond startup
- a dedicated operator repair CLI
- automatic verification that the router now resolves every synced handle
