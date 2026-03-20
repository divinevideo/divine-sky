# ATProto Reverse Projection Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reverse DiVine bridge that ingests ATProto activity and selectively projects it back into funnelcake as valid Nostr events, while keeping standalone AT posts and videos behind explicit consent.

**Architecture:** Keep the existing `divine-atbridge` publish path for Nostr -> ATProto unchanged. Add a separate inbound worker, `divine-atfirehose`, that consumes Jetstream or `subscribeRepos` for public AT activity, filters for DiVine-root threads plus consented standalone content, expands reply subtrees, provisions or looks up custodial Keycast shadow users, signs projected Nostr events, and publishes them to funnelcake. Views stay separate from the firehose path because they are aggregate AppView data; represent them as DiVine service-authored rollup events, not per-user shadow activity.

**Tech Stack:** Rust workspace crates, Diesel/PostgreSQL, Jetstream or `com.atproto.sync.subscribeRepos`, Bluesky AppView APIs, Keycast headless + Nostr RPC APIs, funnelcake Nostr relay publishing, object storage, ProofMode/C2PA-friendly original blob retention.

---

## Planned File Structure

- Modify: `Cargo.toml`
  Add a new workspace member for the inbound worker.
- Create: `crates/divine-atfirehose/Cargo.toml`
  New reverse-ingest service crate.
- Create: `crates/divine-atfirehose/src/main.rs`
  Binary entrypoint and runtime wiring.
- Create: `crates/divine-atfirehose/src/lib.rs`
  Public app wiring and shared exports for tests.
- Create: `crates/divine-atfirehose/src/config.rs`
  Environment contract for Jetstream, AppView, Keycast, funnelcake, and object storage.
- Create: `crates/divine-atfirehose/src/jetstream.rs`
  Stream client and reconnect loop.
- Create: `crates/divine-atfirehose/src/appview.rs`
  Post-thread, label, and view-count lookups.
- Create: `crates/divine-atfirehose/src/filter.rs`
  Root-thread, consent, collection, and media-shape filters.
- Create: `crates/divine-atfirehose/src/projector.rs`
  ATProto -> Nostr projection logic.
- Create: `crates/divine-atfirehose/src/keycast.rs`
  Shadow-user lookup/provisioning and signing client.
- Create: `crates/divine-atfirehose/src/funnelcake.rs`
  Nostr relay publishing client.
- Create: `crates/divine-atfirehose/src/threads.rs`
  Reply-tree expansion and ancestry tracking.
- Create: `crates/divine-atfirehose/src/views.rs`
  AppView polling and service-authored view rollups.
- Create: `crates/divine-atfirehose/src/labels.rs`
  Label fetch and trust policy helpers.
- Create: `crates/divine-atfirehose/tests/root_filters.rs`
- Create: `crates/divine-atfirehose/tests/projection_events.rs`
- Create: `crates/divine-atfirehose/tests/thread_expansion.rs`
- Create: `crates/divine-atfirehose/tests/media_rehost.rs`
- Create: `crates/divine-atfirehose/tests/view_rollups.rs`
- Create: `migrations/003_atproto_projection_tables/up.sql`
- Create: `migrations/003_atproto_projection_tables/down.sql`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-bridge-db/tests/atproto_projection.rs`
  Projection persistence, idempotency, and rollup tests.
- Create: `crates/divine-bridge-types/src/atproto_projection.rs`
- Modify: `crates/divine-bridge-types/src/lib.rs`
  Shared projection kinds, tags, and DTOs.
- Create: `crates/divine-video-worker/src/atproto_blob_fetch.rs`
- Create: `crates/divine-video-worker/src/proof_manifest.rs`
- Modify: `crates/divine-video-worker/src/main.rs`
  Inbound blob fetch, original retention, and proof metadata helpers.
- Modify: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
  Canonical decision updates.
- Modify: `docs/research/2026-03-20-divine-atproto-technical-spec.md`
  Supporting technical notes.
- Create: `docs/runbooks/atproto-reverse-projection.md`
- Create: `docs/runbooks/atproto-reverse-projection-smoke-test.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Modify: `../keycast/api/src/api/http/headless.rs`
- Modify: `../keycast/api/src/api/http/nostr_rpc.rs`
- Modify: `../keycast/api/src/api/http/routes.rs`
- Modify: `../keycast/api/src/api/http/atproto.rs`
- Create: `../keycast/api/tests/atproto_shadow_projection_test.rs`
  Shadow-user provisioning and signing surface for projected AT actors.

## Scope Guard

This plan intentionally splits the reverse path into two phases:

- Phase 1: profiles, likes, replies, reply subtrees, media in replies, and view rollups for DiVine-origin AT threads.
- Phase 2: standalone AT posts and standalone AT videos for explicitly consented AT actors.

Do not collapse those phases. The thread-interaction path is already large enough and is the part the product wants first.

## Chunk 1: Decision Lock And Canonical Docs

### Task 1: Update The Canonical Rules Before Code Changes

**Files:**
- Modify: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
- Modify: `docs/research/2026-03-20-divine-atproto-technical-spec.md`
- Create: `docs/runbooks/atproto-reverse-projection.md`

- [ ] **Step 1: Update the canonical plan to replace the current "analytics-only" reverse path with the approved projection model**
- [ ] **Step 2: Record the consent boundary explicitly: standalone AT posts and videos require consent, but likes, replies, and descendant replies on DiVine-root threads flow without extra consent**
- [ ] **Step 3: Record the protocol boundary explicitly: funnelcake keeps serving valid Nostr events only**
- [ ] **Step 4: Record the identity boundary explicitly: replies and likes use headless Keycast shadow users; views are DiVine service-authored rollups**
- [ ] **Step 5: Review the updated docs and remove any remaining text that says bidirectional likes or replies are out of scope**

## Chunk 2: Projection Data Model

### Task 2: Add Projection Persistence And Shared Types

**Files:**
- Create: `migrations/003_atproto_projection_tables/up.sql`
- Create: `migrations/003_atproto_projection_tables/down.sql`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-bridge-db/tests/atproto_projection.rs`
- Create: `crates/divine-bridge-types/src/atproto_projection.rs`
- Modify: `crates/divine-bridge-types/src/lib.rs`

- [ ] **Step 1: Write failing database tests for shadow-account mappings, projected-event idempotency, root-thread tracking, media retention, and view rollups**
- [ ] **Step 2: Run the new database tests and confirm they fail**
- [ ] **Step 3: Add projection tables for `atproto_shadow_accounts`, `atproto_projection_events`, `atproto_thread_roots`, `atproto_media_mirrors`, and `atproto_view_rollups`**
- [ ] **Step 4: Add Rust models and query helpers keyed by AT DID, AT URI, root AT URI, and projected Nostr event id**
- [ ] **Step 5: Add shared DTOs and tag helpers for projected likes, replies, profiles, and view rollups**
- [ ] **Step 6: Re-run the database tests and confirm they pass**

## Chunk 3: Shadow Accounts And Worker Skeleton

### Task 3: Extend Keycast For Shadow Projection Accounts

**Files:**
- Modify: `../keycast/api/src/api/http/headless.rs`
- Modify: `../keycast/api/src/api/http/nostr_rpc.rs`
- Modify: `../keycast/api/src/api/http/routes.rs`
- Modify: `../keycast/api/src/api/http/atproto.rs`
- Create: `../keycast/api/tests/atproto_shadow_projection_test.rs`

- [ ] **Step 1: Write failing Keycast tests for deterministic shadow-user creation keyed by AT DID**
- [ ] **Step 2: Run the Keycast tests and confirm they fail**
- [ ] **Step 3: Add an internal route or RPC method that creates or looks up a headless custodial user for a public AT actor**
- [ ] **Step 4: Add signing access for projected `kind:0`, `kind:1`, and `kind:7` events without exposing raw private keys**
- [ ] **Step 5: Mark shadow accounts so they cannot publish standalone mirrored AT posts or videos until consent is recorded**
- [ ] **Step 6: Re-run the Keycast tests and confirm they pass**

### Task 4: Create The Inbound Worker And Stream Filters

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/divine-atfirehose/Cargo.toml`
- Create: `crates/divine-atfirehose/src/main.rs`
- Create: `crates/divine-atfirehose/src/lib.rs`
- Create: `crates/divine-atfirehose/src/config.rs`
- Create: `crates/divine-atfirehose/src/jetstream.rs`
- Create: `crates/divine-atfirehose/src/filter.rs`
- Create: `crates/divine-atfirehose/src/keycast.rs`
- Create: `crates/divine-atfirehose/src/funnelcake.rs`
- Create: `crates/divine-atfirehose/tests/root_filters.rs`

- [ ] **Step 1: Write failing worker tests for profile updates, likes, replies, and consent-gated standalone post filtering**
- [ ] **Step 2: Run the worker tests and confirm they fail**
- [ ] **Step 3: Add workspace wiring and create the new `divine-atfirehose` crate**
- [ ] **Step 4: Implement stream connection, reconnect behavior, and cursor persistence using `ingest_offsets`**
- [ ] **Step 5: Implement filters that accept:**
  - `app.bsky.actor.profile` for actors already in tracked DiVine-root threads
  - `app.bsky.feed.like` targeting DiVine-root thread posts
  - `app.bsky.feed.post` replies whose root is a DiVine-origin AT post
  - standalone `app.bsky.feed.post` only when the actor has explicit Nostr-side consent
- [ ] **Step 6: Re-run the worker tests and confirm they pass**

## Chunk 4: Projection Semantics

### Task 5: Project Valid Nostr Events With Provenance Tags

**Files:**
- Create: `crates/divine-atfirehose/src/projector.rs`
- Create: `crates/divine-atfirehose/tests/projection_events.rs`
- Modify: `crates/divine-bridge-types/src/atproto_projection.rs`

- [ ] **Step 1: Write failing tests for projected profiles, replies, and likes**
- [ ] **Step 2: Run the projection tests and confirm they fail**
- [ ] **Step 3: Implement profile projection as `kind:0` signed by the actor's shadow Keycast user**
- [ ] **Step 4: Implement reply projection as `kind:1` with normal Nostr reply tags plus `proxy` provenance tags back to the source `at://` URI**
- [ ] **Step 5: Implement like projection as `kind:7` with `proxy` provenance tags back to the source `at://` URI**
- [ ] **Step 6: Ensure every projected event is valid Nostr JSON and can be published to funnelcake without special websocket envelopes**
- [ ] **Step 7: Re-run the projection tests and confirm they pass**

### Task 6: Expand Full Reply Subtrees Under DiVine Roots

**Files:**
- Create: `crates/divine-atfirehose/src/threads.rs`
- Create: `crates/divine-atfirehose/src/appview.rs`
- Create: `crates/divine-atfirehose/tests/thread_expansion.rs`
- Modify: `crates/divine-atfirehose/src/filter.rs`
- Modify: `crates/divine-atfirehose/src/projector.rs`

- [ ] **Step 1: Write failing tests for direct replies, replies-to-replies, and replay idempotency under a DiVine-root thread**
- [ ] **Step 2: Run the thread-expansion tests and confirm they fail**
- [ ] **Step 3: Track DiVine-root AT URIs from existing outbound `record_mappings`**
- [ ] **Step 4: Implement ancestry resolution so the worker accepts descendant replies whose root points at a tracked DiVine-origin post**
- [ ] **Step 5: Persist projected-thread state so reconnects do not duplicate Nostr events**
- [ ] **Step 6: Re-run the thread-expansion tests and confirm they pass**

## Chunk 5: Media, Proof, And Views

### Task 7: Rehost Inbound Media As Originals First

**Files:**
- Create: `crates/divine-video-worker/src/atproto_blob_fetch.rs`
- Create: `crates/divine-video-worker/src/proof_manifest.rs`
- Modify: `crates/divine-video-worker/src/main.rs`
- Create: `crates/divine-atfirehose/tests/media_rehost.rs`
- Modify: `crates/divine-atfirehose/src/projector.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`

- [ ] **Step 1: Write failing tests for rehosting image and video replies with original-byte retention**
- [ ] **Step 2: Run the media tests and confirm they fail**
- [ ] **Step 3: Add an ATProto blob fetch client that downloads the original blob bytes from the source PDS**
- [ ] **Step 4: Store original bytes and metadata in `atproto_media_mirrors` before generating any derived playback assets**
- [ ] **Step 5: Record source hash, source `at://` URI, original mime type, and optional ProofMode/C2PA bundle references**
- [ ] **Step 6: Keep the projected Nostr event pointed at the DiVine-hosted canonical original or its approved derivative, but retain a durable link back to the original for verification**
- [ ] **Step 7: Re-run the media tests and confirm they pass**

### Task 8: Add View Rollups As DiVine Service Events

**Files:**
- Create: `crates/divine-atfirehose/src/views.rs`
- Modify: `crates/divine-atfirehose/src/appview.rs`
- Create: `crates/divine-atfirehose/tests/view_rollups.rs`
- Modify: `crates/divine-bridge-types/src/atproto_projection.rs`

- [ ] **Step 1: Write failing tests for AppView-backed view-count polling and replaceable rollup publication**
- [ ] **Step 2: Run the view-rollup tests and confirm they fail**
- [ ] **Step 3: Implement AppView reads for tracked DiVine-root posts because views are not firehose-authored repo records**
- [ ] **Step 4: Publish view counts as DiVine service-authored replaceable Nostr events, not shadow-user events**
- [ ] **Step 5: Persist last-seen counts so repeated polls only emit when the count changes**
- [ ] **Step 6: Re-run the view-rollup tests and confirm they pass**

## Chunk 6: Labels, Moderation, And Ops

### Task 9: Fold In Bluesky Label Context Without Blocking The Same Moderation Path Used For Native Nostr

**Files:**
- Create: `crates/divine-atfirehose/src/labels.rs`
- Modify: `crates/divine-bridge-types/src/atproto_labels.rs`
- Create: `crates/divine-atfirehose/tests/labels.rs`

- [ ] **Step 1: Write failing tests for label lookups on inbound actors and subjects**
- [ ] **Step 2: Run the label tests and confirm they fail**
- [ ] **Step 3: Fetch public label state for inbound content and actors during projection**
- [ ] **Step 4: Drop or quarantine obviously hidden or takedown-labeled content before projection**
- [ ] **Step 5: Persist label snapshots for projected events, but still let the normal DiVine moderation path handle post-publication review**
- [ ] **Step 6: Re-run the label tests and confirm they pass**

### Task 10: Document Reverse Projection Operations

**Files:**
- Create: `docs/runbooks/atproto-reverse-projection.md`
- Create: `docs/runbooks/atproto-reverse-projection-smoke-test.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/launch-checklist.md`

- [ ] **Step 1: Document the consent model and the difference between thread interactions and standalone mirrored content**
- [ ] **Step 2: Document the shadow-user model and the service-authored view-rollup exception**
- [ ] **Step 3: Document media retention, proof verification, and how external validators should find the canonical original**
- [ ] **Step 4: Add a smoke test covering a DiVine-root AT post, an inbound reply with media, an inbound like, and a view-rollup update**
- [ ] **Step 5: Review the runbooks for consistency with the canonical plan**

## Chunk 7: Phase 2 Standalone Content

### Task 11: Add Consent-Gated Standalone AT Post And Video Mirroring

**Files:**
- Modify: `crates/divine-atfirehose/src/filter.rs`
- Create: `crates/divine-atfirehose/src/consent.rs`
- Create: `crates/divine-atfirehose/tests/consented_content.rs`
- Modify: `../keycast/api/src/api/http/atproto.rs`

- [ ] **Step 1: Write failing tests for standalone AT post and video mirroring behind explicit consent**
- [ ] **Step 2: Run the consented-content tests and confirm they fail**
- [ ] **Step 3: Add a consent lookup keyed by AT DID that distinguishes thread-interaction projection from standalone content mirroring**
- [ ] **Step 4: Gate standalone posts and videos behind that consent state**
- [ ] **Step 5: Leave the `<= 6 second` standalone-video filter as an explicit follow-up decision instead of silently baking it into v1**
- [ ] **Step 6: Re-run the consented-content tests and confirm they pass**

Plan complete and saved to `docs/superpowers/plans/2026-03-21-atproto-reverse-projection.md`. Ready to execute?
