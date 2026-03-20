# Keycast Bring-Your-Own ATProto With Custodial Nostr Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Keycast architecture path where an existing ATProto user links their current account, receives a custodial Nostr identity plus vanity NIP-05 alias, and stays synchronized across ATProto and Nostr for creates, edits, and deletes.

**Architecture:** Reuse `crates/divine-handle-gateway` as the control-plane entrypoint, add a dedicated signer boundary for custodial Nostr keys, and introduce a sync worker that owns cross-network mapping and reconciliation. Keep the existing DiVine-provisioned AT path isolated so the Keycast path can evolve without mutating the old assumptions in place.

**Tech Stack:** Rust workspace crates, Axum services, PostgreSQL, encrypted secret storage or KMS-backed key references, ATProto OAuth/client APIs, host-based NIP-05 `nostr.json` resolution.

---

## Chunk 1: Identity Link And Alias Claims

### Task 1: Extend The Control Plane Data Model

**Files:**
- Create: `migrations/002_keycast_linked_accounts/up.sql`
- Create: `migrations/002_keycast_linked_accounts/down.sql`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Test: `crates/divine-bridge-db/tests/keycast_linked_accounts.rs`

- [ ] **Step 1: Write failing database tests for linked account and alias claim persistence**
- [ ] **Step 2: Run the new database tests and confirm they fail**
- [ ] **Step 3: Add `linked_accounts` and `alias_claims` schema plus Rust models**
- [ ] **Step 4: Add query functions keyed by `at_did` and `alias_host`**
- [ ] **Step 5: Re-run the database tests and confirm they pass**

### Task 2: Add BYO-AT Account Link Routes

**Files:**
- Modify: `crates/divine-handle-gateway/src/lib.rs`
- Create: `crates/divine-handle-gateway/src/routes/atproto_link.rs`
- Create: `crates/divine-handle-gateway/src/routes/alias_claim.rs`
- Modify: `crates/divine-handle-gateway/src/routes/mod.rs`
- Test: `crates/divine-handle-gateway/tests/keycast_account_link.rs`

- [ ] **Step 1: Write failing route tests for AT link creation and alias claim**
- [ ] **Step 2: Run the route tests and confirm they fail**
- [ ] **Step 3: Implement the control-plane routes and state transitions**
- [ ] **Step 4: Validate alias claims are bound to `at_did`, not handle text**
- [ ] **Step 5: Re-run the route tests and confirm they pass**

## Chunk 2: Custodial Nostr Signer

### Task 3: Introduce A Dedicated Signer Boundary

**Files:**
- Create: `crates/divine-keycast-signer/Cargo.toml`
- Create: `crates/divine-keycast-signer/src/lib.rs`
- Create: `crates/divine-keycast-signer/src/signer.rs`
- Create: `crates/divine-keycast-signer/src/store.rs`
- Test: `crates/divine-keycast-signer/tests/signing.rs`

- [ ] **Step 1: Write failing signer tests for key generation, signature issuance, and disabled-account rejection**
- [ ] **Step 2: Run the signer tests and confirm they fail**
- [ ] **Step 3: Implement encrypted key references and signing API surface**
- [ ] **Step 4: Ensure private key bytes never leave the signer boundary in public APIs**
- [ ] **Step 5: Re-run the signer tests and confirm they pass**

## Chunk 3: Alias Resolution

### Task 4: Serve Host-Based NIP-05 Resolution For `*.bluesky.name`

**Files:**
- Create: `crates/divine-handle-gateway/src/routes/nostr_well_known.rs`
- Modify: `crates/divine-handle-gateway/src/routes/mod.rs`
- Modify: `crates/divine-handle-gateway/src/lib.rs`
- Test: `crates/divine-handle-gateway/tests/nostr_well_known.rs`

- [ ] **Step 1: Write failing tests for `/.well-known/nostr.json?name=_` host-based resolution**
- [ ] **Step 2: Run the resolver tests and confirm they fail**
- [ ] **Step 3: Implement host lookup from `alias_claims` and return the active Nostr pubkey**
- [ ] **Step 4: Reject disabled or inactive alias claims**
- [ ] **Step 5: Re-run the resolver tests and confirm they pass**

## Chunk 4: Cross-Network Sync

### Task 5: Add Mapping And Operation Log Persistence

**Files:**
- Create: `migrations/003_keycast_sync_ops/up.sql`
- Create: `migrations/003_keycast_sync_ops/down.sql`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Test: `crates/divine-bridge-db/tests/keycast_sync_ops.rs`

- [ ] **Step 1: Write failing persistence tests for `content_mappings` and `sync_operations`**
- [ ] **Step 2: Run the persistence tests and confirm they fail**
- [ ] **Step 3: Implement schema and queries for idempotent operation tracking**
- [ ] **Step 4: Re-run the persistence tests and confirm they pass**

### Task 6: Build The Keycast Sync Worker

**Files:**
- Create: `crates/divine-atbridge/src/keycast_sync.rs`
- Modify: `crates/divine-atbridge/src/lib.rs`
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Test: `crates/divine-atbridge/tests/keycast_sync.rs`

- [ ] **Step 1: Write failing tests for create, edit, and delete pass-through using stored mappings**
- [ ] **Step 2: Run the sync tests and confirm they fail**
- [ ] **Step 3: Implement sync operations that call the signer and the linked user's PDS client**
- [ ] **Step 4: Implement last-successful-operation-wins conflict handling with idempotent retries**
- [ ] **Step 5: Re-run the sync tests and confirm they pass**

## Chunk 5: AT Auth And Operational Hardening

### Task 7: Add Delegated AT Auth Storage And Reconnect Behavior

**Files:**
- Create: `crates/divine-handle-gateway/src/atproto_oauth.rs`
- Modify: `crates/divine-handle-gateway/src/lib.rs`
- Test: `crates/divine-handle-gateway/tests/atproto_oauth.rs`

- [ ] **Step 1: Write failing tests for token persistence, refresh, and blocked-sync behavior**
- [ ] **Step 2: Run the OAuth tests and confirm they fail**
- [ ] **Step 3: Implement encrypted token reference handling and blocked-account transitions**
- [ ] **Step 4: Re-run the OAuth tests and confirm they pass**

### Task 8: Document The New Keycast Path

**Files:**
- Modify: `docs/runbooks/login-divine-video.md`
- Create: `docs/runbooks/keycast-byo-atproto.md`
- Modify: `docs/runbooks/launch-checklist.md`

- [ ] **Step 1: Document the signer boundary and custodial-key assumptions**
- [ ] **Step 2: Document alias claim behavior for `*.bluesky.name`**
- [ ] **Step 3: Document AT reconnect and disable flows**
- [ ] **Step 4: Review the runbooks for consistency with the design spec**

Plan complete and saved to `docs/superpowers/plans/2026-03-20-keycast-byo-atproto-custodial-nostr.md`. Ready to execute?
