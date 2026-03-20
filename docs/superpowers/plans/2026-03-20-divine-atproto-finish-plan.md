# DiVine ATProto Finish Plan Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the incomplete DiVine ATProto implementation by promoting the real Rust worktree into the canonical repo, making the core publish path durable and testable, then adding the missing control-plane, discovery, moderation, and rollout pieces.

**Architecture:** Reuse the existing Rust implementation from `.claude/worktrees/zealous-bartik` instead of rewriting it. Promote that code into the root repository as a top-level Cargo workspace rooted in `crates/`, then harden the core pipeline around durable replay, correct delete ownership, hash-verified media handling, and a real `login.divine.video` control plane before starting feedgen or moderation features.

**Tech Stack:** Rust, Cargo workspace, Diesel/PostgreSQL, `rsky-pds`, S3/R2-compatible storage, Nostr relay/WebSocket consumption, Blossom, Docker Compose, optional Ozone.

---

## Context

This plan is based on:

- the canonical plan in `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
- the implementation currently living in `.claude/worktrees/zealous-bartik`
- review findings from parallel agents over core bridge flow, identity/provisioning, video/infra, and milestone coverage

## Planning Decisions

- Preserve the existing `crates/` implementation shape for now. Do not block shipping on a `services/` and `libs/` migration.
- Promote the current worktree code into the root repository before adding more features. Hidden worktree code is not a maintainable source of truth.
- Finish the bridge and control plane before building feedgen or moderation services.
- Make `cargo test --workspace` reproducible before claiming milestone completion.

## Chunk 1: Promote The Existing Implementation

### Task 1: Move The Claude Worktree Code Into The Root Workspace

**Files:**
- Create: `Cargo.toml`
- Create: `config/docker-compose.yml`
- Create: `crates/divine-atbridge/Cargo.toml`
- Create: `crates/divine-atbridge/src/config.rs`
- Create: `crates/divine-atbridge/src/deletion.rs`
- Create: `crates/divine-atbridge/src/main.rs`
- Create: `crates/divine-atbridge/src/nostr_consumer.rs`
- Create: `crates/divine-atbridge/src/pipeline.rs`
- Create: `crates/divine-atbridge/src/provisioner.rs`
- Create: `crates/divine-atbridge/src/publisher.rs`
- Create: `crates/divine-atbridge/src/signature.rs`
- Create: `crates/divine-atbridge/src/text_builder.rs`
- Create: `crates/divine-atbridge/src/translator.rs`
- Create: `crates/divine-bridge-db/Cargo.toml`
- Create: `crates/divine-bridge-db/src/lib.rs`
- Create: `crates/divine-bridge-db/src/models.rs`
- Create: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-bridge-db/src/schema.rs`
- Create: `crates/divine-bridge-types/Cargo.toml`
- Create: `crates/divine-bridge-types/src/lib.rs`
- Create: `crates/divine-video-worker/Cargo.toml`
- Create: `crates/divine-video-worker/src/blob_upload.rs`
- Create: `crates/divine-video-worker/src/blossom.rs`
- Create: `crates/divine-video-worker/src/cid.rs`
- Create: `crates/divine-video-worker/src/main.rs`
- Create: `migrations/001_bridge_tables/up.sql`
- Create: `migrations/001_bridge_tables/down.sql`
- Create: `deploy/pds/docker-compose.yml`
- Create: `deploy/pds/env.example`
- Create: `deploy/pds/README.md`
- Modify: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`

- [x] **Step 1: Copy the current worktree implementation into the root repo**

Source: `.claude/worktrees/zealous-bartik`
Target: repository root paths listed above

- [x] **Step 2: Update the canonical plan to bless `crates/` as the current implementation layout**

Change the “Proposed Repository Layout” section to reflect the promoted workspace rather than the unimplemented `services/`/`libs/` split.

- [x] **Step 3: Run a root-level compile check**

Run: `cargo check --workspace`
Expected: exit code `0`

- [x] **Step 4: Commit the promoted baseline**

```bash
git add Cargo.toml crates config migrations deploy/pds docs/plans/2026-03-20-divine-atproto-unified-plan.md
git commit -m "chore: promote atproto implementation into root workspace"
```

### Task 2: Make Workspace Verification Reproducible

**Files:**
- Modify: `Cargo.toml`
- Modify: `config/docker-compose.yml`
- Create: `scripts/test-workspace.sh`
- Create: `docs/runbooks/dev-bootstrap.md`
- Create: `.github/workflows/rust.yml`

- [x] **Step 1: Document the native dependency requirements**

Include `libpq` requirements for macOS and Linux and the expected environment variables if Homebrew installs `libpq` keg-only.

- [x] **Step 2: Add a single test bootstrap script**

Run: `bash scripts/test-workspace.sh`
Expected: the script checks prerequisites, then runs the workspace verification suite

- [x] **Step 3: Add CI for compile and test execution**

Make CI provision PostgreSQL client dependencies so `cargo test --workspace` does not silently depend on a particular laptop state.

- [x] **Step 4: Run the current tests from the root workspace**

Run: `cargo test -p divine-atbridge && cargo test -p divine-video-worker`
Expected: both commands pass

- [x] **Step 5: Run the full workspace tests**

Run: `cargo test --workspace`
Expected: exit code `0`

## Chunk 2: Finish Identity And Control Plane

### Task 3: Harden Account Link State And Provisioning Lifecycle

**Files:**
- Modify: `migrations/001_bridge_tables/up.sql`
- Modify: `migrations/001_bridge_tables/down.sql`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Modify: `crates/divine-atbridge/src/provisioner.rs`
- Create: `crates/divine-atbridge/tests/provisioning_lifecycle.rs`

- [x] **Step 1: Write failing lifecycle tests for account-link state**

Cover `pending`, `ready`, `failed`, and `disabled` transitions, plus idempotent retry by `nostr_pubkey`.

Run: `cargo test -p divine-atbridge provisioning_lifecycle -- --nocapture`
Expected: FAIL because lifecycle fields and idempotent behavior are incomplete

- [x] **Step 2: Extend `account_links` to represent real control-plane state**

Add at least:
- `disabled_at`
- `updated_at`
- `provisioning_state`
- `provisioning_error`
- `plc_rotation_key_ref`

- [x] **Step 3: Separate AT repo signing key and PLC rotation key**

Provisioning must no longer use one key for both purposes.

- [x] **Step 4: Make provisioning idempotent and repairable**

Persist in-progress state before PLC/PDS side effects and make retries reconcile rather than minting a second DID.

- [x] **Step 5: Re-run lifecycle tests**

Run: `cargo test -p divine-atbridge provisioning_lifecycle -- --nocapture`
Expected: PASS

### Task 4: Build The `login.divine.video` Control Plane Service

**Files:**
- Create: `crates/divine-handle-gateway/Cargo.toml`
- Create: `crates/divine-handle-gateway/src/main.rs`
- Create: `crates/divine-handle-gateway/src/routes/mod.rs`
- Create: `crates/divine-handle-gateway/src/routes/opt_in.rs`
- Create: `crates/divine-handle-gateway/src/routes/provision.rs`
- Create: `crates/divine-handle-gateway/src/routes/status.rs`
- Create: `crates/divine-handle-gateway/src/routes/disable.rs`
- Create: `crates/divine-handle-gateway/src/routes/export.rs`
- Create: `crates/divine-handle-gateway/src/routes/well_known.rs`
- Create: `crates/divine-handle-gateway/tests/control_plane.rs`
- Create: `docs/runbooks/login-divine-video.md`
- Modify: `Cargo.toml`

- [x] **Step 1: Write failing tests for opt-in, status, disable, export, and `.well-known` resolution**

Run: `cargo test -p divine-handle-gateway control_plane -- --nocapture`
Expected: FAIL because the service and routes do not exist yet

- [x] **Step 2: Implement host-based handle resolution**

Support `https://username.divine.video/.well-known/atproto-did` from stored account-link state.

- [x] **Step 3: Implement control-plane routes**

Routes must own:
- opt-in
- provisioning trigger
- status
- disable
- export

- [x] **Step 4: Document operational responsibilities for `login.divine.video`**

Capture boundary rules, auth assumptions, and handoff to the bridge/PDS.

- [x] **Step 5: Re-run the control-plane tests**

Run: `cargo test -p divine-handle-gateway control_plane -- --nocapture`
Expected: PASS

## Chunk 3: Make The Publish Path Durable

### Task 5: Fix Replay State And Delete Ownership

**Files:**
- Modify: `crates/divine-atbridge/src/nostr_consumer.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/deletion.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-atbridge/tests/replay_and_delete.rs`

- [x] **Step 1: Write failing tests for replay persistence and delete ownership**

Cover:
- no offset advance on failed publish
- restart/reconnect behavior
- delete-author mismatch rejection
- delete against `mapping.did`

Run: `cargo test -p divine-atbridge replay_and_delete -- --nocapture`
Expected: FAIL because replay/delete behavior is currently unsafe

- [x] **Step 2: Move replay progression behind successful processing**

The bridge must only persist durable offsets after publish/delete success.

- [x] **Step 3: Collapse delete handling into one path**

Use `mapping.did`, validate ownership, and keep behavior identical across direct and pipeline paths.

- [x] **Step 4: Re-run replay/delete tests**

Run: `cargo test -p divine-atbridge replay_and_delete -- --nocapture`
Expected: PASS

### Task 6: Unify Media Handling And Persist Publish Lineage

**Files:**
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/publisher.rs`
- Modify: `crates/divine-video-worker/src/main.rs`
- Modify: `crates/divine-video-worker/src/blossom.rs`
- Modify: `crates/divine-video-worker/src/blob_upload.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-atbridge/tests/media_lineage.rs`
- Create: `docs/runbooks/media-path.md`

- [x] **Step 1: Choose one media-path architecture and delete the split**

Decision:
- either `divine-video-worker` becomes the real fetch/verify/upload service
- or media handling is folded fully into `divine-atbridge`

Do not keep two partial paths.

- [x] **Step 2: Write failing tests for hash-based fetch and manifest persistence**

Cover:
- fetch by expected Blossom hash
- SHA-256 verification
- CID capture
- `asset_manifest` write
- `record_mappings` persistence with `cid` and status

Run: `cargo test -p divine-atbridge media_lineage -- --nocapture`
Expected: FAIL because the current flow is URL-based and does not persist full lineage

- [x] **Step 3: Implement the real media contract**

The pipeline must carry expected source hash, destination CID, MIME type, size, and publish status.

- [x] **Step 4: Add safe network behavior for video-sized fetches**

Set explicit timeouts and avoid unbounded buffering assumptions where possible.

- [x] **Step 5: Re-run media lineage tests**

Run: `cargo test -p divine-atbridge media_lineage -- --nocapture`
Expected: PASS

### Task 7: Finish The Runnable Bridge Service

**Files:**
- Modify: `crates/divine-atbridge/src/main.rs`
- Modify: `crates/divine-atbridge/src/config.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/publisher.rs`
- Create: `crates/divine-atbridge/tests/publish_path_integration.rs`

- [x] **Step 1: Replace the stub executable with a real service loop**

The binary must wire real DB-backed stores, relay consumer, and PDS/blob collaborators.

- [x] **Step 2: Write a failing integration test for the real publish path**

Run: `cargo test -p divine-atbridge publish_path_integration -- --nocapture`
Expected: FAIL because the service loop is not fully wired yet

- [x] **Step 3: Implement the concrete adapters**

Required collaborators:
- DB-backed account store
- DB-backed mapping store
- hash-verifying Blossom fetcher
- real PDS publisher/blob uploader

- [x] **Step 4: Re-run the publish integration test**

Run: `cargo test -p divine-atbridge publish_path_integration -- --nocapture`
Expected: PASS

## Chunk 4: Complete M3 And Test The Stack End To End

### Task 8: Add Profile Sync And End-To-End Local Stack

**Files:**
- Modify: `config/docker-compose.yml`
- Modify: `deploy/pds/docker-compose.yml`
- Create: `config/minio-init.sh`
- Create: `config/mock-blossom/`
- Create: `crates/divine-atbridge/src/profile_sync.rs`
- Create: `crates/divine-atbridge/tests/e2e_local_stack.rs`
- Create: `docs/runbooks/pds-operations.md`

- [x] **Step 1: Expand the local stack to cover the real publish path**

Bring up:
- PostgreSQL
- MinIO with bucket bootstrap
- PDS
- bridge
- mock Blossom service

Add healthchecks.

- [x] **Step 2: Write failing end-to-end tests**

Cover:
- NIP-71 publish to renderable AT post
- delete propagation
- replay restart
- kind `0` profile sync

Run: `cargo test -p divine-atbridge e2e_local_stack -- --nocapture`
Expected: FAIL before wiring is complete

- [x] **Step 3: Implement one-way profile sync from Nostr kind `0`**

Map display name, bio, avatar, banner, and website.

- [x] **Step 4: Re-run end-to-end tests**

Run: `cargo test -p divine-atbridge e2e_local_stack -- --nocapture`
Expected: PASS

## Chunk 5: Discovery, Moderation, And Rollout

### Task 9: Add Feed Generator

**Files:**
- Create: `crates/divine-feedgen/Cargo.toml`
- Create: `crates/divine-feedgen/src/main.rs`
- Create: `crates/divine-feedgen/src/skeleton.rs`
- Create: `crates/divine-feedgen/tests/feed_skeleton.rs`
- Modify: `Cargo.toml`

- [x] **Step 1: Write failing feed skeleton tests**

Run: `cargo test -p divine-feedgen feed_skeleton -- --nocapture`
Expected: FAIL because the crate does not exist yet

- [x] **Step 2: Implement latest and trending feed skeleton endpoints**

Back them with DiVine-owned data only; do not require full firehose ingestion.

- [x] **Step 3: Re-run feed tests**

Run: `cargo test -p divine-feedgen feed_skeleton -- --nocapture`
Expected: PASS

### Task 10: Add Moderation Adapter

**Files:**
- Create: `crates/divine-moderation-adapter/Cargo.toml`
- Create: `crates/divine-moderation-adapter/src/main.rs`
- Create: `crates/divine-moderation-adapter/src/labels.rs`
- Create: `crates/divine-moderation-adapter/tests/label_mapping.rs`
- Modify: `Cargo.toml`

- [x] **Step 1: Write failing label mapping tests**

Run: `cargo test -p divine-moderation-adapter label_mapping -- --nocapture`
Expected: FAIL because the crate does not exist yet

- [x] **Step 2: Implement DiVine label translation and moderation queue hooks**

Keep inbound moderation human-reviewed.

- [x] **Step 3: Re-run label tests**

Run: `cargo test -p divine-moderation-adapter label_mapping -- --nocapture`
Expected: PASS

### Task 11: Add Runbooks, Launch Checks, And Final Verification

**Files:**
- Create: `docs/runbooks/launch-checklist.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/login-divine-video.md`
- Modify: `docs/runbooks/pds-operations.md`

- [x] **Step 1: Add the launch checklist**

Cover:
- BGS crawl enablement
- rate-limit review
- alerting
- rollback
- DMCA/takedown handling
- staged cohort rollout

- [x] **Step 2: Add a final operator bootstrap section**

A new developer should be able to stand up the full stack from one runbook.

- [x] **Step 3: Run the final verification suite**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all commands exit `0`

For portable local verification on machines where `libpq` is not already exported into the linker environment, use `bash scripts/test-workspace.sh`; it configures `libpq` paths first and then runs `cargo test --workspace`.

- [x] **Step 4: Commit rollout readiness**

```bash
git add .
git commit -m "feat: finish divine atproto implementation path"
```

## Parallelization Guidance

After promotion into the root repo, the safest parallel split is:

- Stream A: workspace/test normalization and runtime wiring
- Stream B: `handle-gateway` control plane

Do not parallelize feedgen or moderation ahead of a runnable bridge plus durable replay state.

## Stop Conditions

Stop and re-evaluate if any of these happens:

- `rsky-pds` proves incompatible with the required publish path
- `pds.divine.video` cannot meet origin-isolation requirements
- integration tests require a different local stack shape than this plan assumes
- the promoted `crates/` layout becomes clearly inferior to a `services/` and `libs/` split before more code lands
