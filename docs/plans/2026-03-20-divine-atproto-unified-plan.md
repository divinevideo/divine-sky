# DiVine ATProto Unified Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish one canonical, decision-ready and execution-ready plan for shipping DiVine's ATProto distribution path without losing the Nostr-native authoring model.

**Architecture:** Nostr remains the write path and source of truth. A DiVine bridge consumes funnelcake, verifies Nostr-signed NIP-71 video events, resolves a linked ATProto account, uploads media to a DiVine-operated PDS, and writes standard `app.bsky.feed.post` plus `app.bsky.embed.video` records. `login.divine.video` is the consent and account-linking control plane; `pds.divine.video` is the branded PDS host and must stay origin-isolated from end-user app auth.

**Tech Stack:** Rust, `rsky-pds`, PostgreSQL, S3/R2-compatible object storage, funnelcake/NATS, Blossom, Nostr NIPs 05/09/39/71/92, ATProto `app.bsky.*`, optional Ozone for moderation operations.

---

## Document Role

This file is the canonical plan for this repository.

- `pompt_plan.md` is the original research brief and problem statement.
- `divine-sky-technical-spec.md` is a prior synthesis with useful detail but some contradictions.
- `docs/research/*.md` are supporting analyses and diagram inputs.
- This document is the only normative source for both architecture decisions and implementation sequencing.

## Source Hierarchy

1. This unified plan
2. Current external protocol specifications and official implementation docs
3. Supporting repo research documents
4. Original research prompt

When this file conflicts with older repo docs, this file wins.

## Resolved Decisions

| Topic | Decision | Why |
| --- | --- | --- |
| Source of truth | Nostr and Blossom remain primary | Keeps DiVine philosophically and operationally Nostr-first |
| Publish model | One-way Nostr to ATProto in MVP | Lower abuse, identity, and moderation complexity |
| AT identity | `did:plc` from day one | Portability and ecosystem compatibility beat `did:web` simplicity |
| Handles | `username.divine.video` | Clear user-facing brand and standard ATProto handle model |
| Login integration | `login.divine.video` owns consent, linking, export, and recovery UI | Separates human account management from repo hosting |
| PDS host | `pds.divine.video` for MVP, with strict origin isolation | Preserves brand while keeping the control plane separate |
| Blob strategy | Fetch from Blossom, verify SHA-256, upload to PDS storage | Spec-aligned and easiest to reason about |
| Record shape | Standard `app.bsky.feed.post` with `app.bsky.embed.video` | Works in Bluesky, Skylight, Flashes, and generic AT clients |
| Repo key strategy | Use NIP-71 `d` tag as `rkey` when valid, else deterministic hash fallback | Supports idempotent writes and updates |
| Identity link exposure | Internal link table plus NIP-39 `i` tags and profile metadata | Gives cross-protocol discoverability without custom DID fields |
| Feed strategy | Start with standard posts, add DiVine feed generators in phase 2 | Maximizes interoperability first |
| Moderation | DiVine labeler plus human review queue; no automatic Nostr-side synthetic enforcement from AT labels | Preserves auditability and avoids cross-network overreach |

## Decisions Closed From Prior Drafts

- The `did:web for MVP` heading in `divine-sky-technical-spec.md` is rejected. The canonical decision is `did:plc` from the start.
- `login.divine.video` is adopted as a control plane, not the PDS.
- `pds.divine.video` remains the canonical branded PDS host for MVP, but only if cookies, sessions, and blob-serving are isolated from the main app and login surfaces.
- `rsky-pds` is the base implementation target. `rsky-video` is a spike item, not a locked dependency, because its operational assumptions need validation against DiVine's stack.
- MVP stays on standard `app.bsky.*` records only. Any `video.divine.*` lexicon is deferred until there is a proven client or product need.

## Open Decisions To Close Before Public Launch

| Topic | Current position | Exit condition |
| --- | --- | --- |
| PDS hostname hardening | Use `pds.divine.video` unless origin isolation proves brittle | Production security review complete |
| Video processing path | Spike `rsky-video` vs custom upload worker | Throughput, cost, and client compatibility benchmark complete |
| Object store vendor | Prefer R2 if pricing and latency hold | Vendor decision memo approved |
| Labeler stack | Ozone likely for moderation operations | Small proof-of-concept validates fit |
| Public provenance lexicon | Deferred | Need from clients or creator tooling is demonstrated |

## Canonical Architecture

### System boundaries

- `divine.video`: public product surfaces
- `login.divine.video`: login, consent, account-linking, recovery, export, and status UI
- `pds.divine.video`: ATProto PDS, sync endpoints, blob serving
- funnelcake + NATS: Nostr event source and internal event bus
- Blossom: source media storage for Nostr-side publishing
- bridge workers: verify, translate, upload, and publish
- feed generator: phase 2 discovery service
- labeler and moderation adapter: phase 3 trust and safety service

### Core data flow

1. User records a loop and publishes a NIP-71 event through DiVine.
2. Media lands in Blossom and the event lands in funnelcake.
3. The bridge consumes the event stream, verifies the Nostr signature, confirms opt-in and identity link state, and derives the target AT write.
4. The media worker fetches the Blossom asset, verifies the source hash, uploads the AT blob, and prepares captions and metadata.
5. The bridge writes a standard ATProto post into the user's repository on `rsky-pds`.
6. The PDS exposes the new commit through sync endpoints and downstream AT clients render the post.

### Domain and security rules

- `login.divine.video` must not share browser session state with `pds.divine.video`.
- `pds.divine.video` must not serve the main DiVine app or login UX.
- PDS cookies, if any, must be host-only and never set for `.divine.video`.
- AT blob serving must be treated as untrusted media hosting, not as an authenticated application surface.

## Identity and Account Model

### Canonical decisions

- Create AT accounts only after explicit user opt-in.
- Generate a dedicated AT signing keypair per linked account.
- Store signing keys in KMS/HSM-backed custody.
- Store a separate PLC rotation key under stricter recovery controls.
- Publish the AT DID in Nostr profile metadata and NIP-39 `i` tags.
- Do not derive AT signing keys from Nostr keys in MVP.

### Handle resolution rules

- Primary handle format: `username.divine.video`
- Preferred resolution flow: `https://username.divine.video/.well-known/atproto-did`
- Support `_atproto.username.divine.video` TXT if operations later require it

### Required link-state model

`account_links` must minimally store:

- `nostr_pubkey`
- `atproto_did`
- `atproto_handle`
- `crosspost_enabled`
- `signing_key_ref`
- `created_at`
- `disabled_at`

## Content and Media Rules

- One DiVine video maps to one AT post.
- Standard shape is `app.bsky.feed.post` plus `app.bsky.embed.video`.
- Preserve original creation time in `createdAt`.
- Use richtext facets for hashtags, mentions, and URLs.
- Use the NIP-71 `d` tag as `rkey` when valid; otherwise derive a deterministic stable key.
- Keep a durable `record_mappings` table keyed by `nostr_event_id`.
- On NIP-09 deletion, delete the mapped AT record and preserve the mapping row as tombstoned provenance.

### Blob handling

- Fetch bytes from Blossom by referenced hash.
- Verify source SHA-256 before upload.
- Upload to PDS-controlled storage.
- Track source hash, destination CID, MIME type, and size in `asset_manifest`.
- Treat shared object storage as an optimization phase, not an MVP dependency.

## Feed, Discovery, and Moderation Rules

- MVP publishes only standard posts and relies on default AT discovery.
- Phase 2 adds DiVine-owned feed generators backed by Gorse signals and DiVine’s own mappings.
- Phase 3 adds a DiVine labeler plus an inbound moderation adapter and human review queue.
- AT engagement is analytics-only in MVP and phase 2.
- No synthetic Nostr likes, reposts, or replies are emitted from AT user actions in MVP.

## Proposed Repository Layout

The implementation has been promoted into the root repo. The current canonical layout is:

```text
Cargo.toml
Cargo.lock
config/
  docker-compose.yml
crates/
  divine-atbridge/
  divine-bridge-db/
  divine-bridge-types/
  divine-video-worker/
  divine-handle-gateway/        # planned next
  divine-feedgen/               # planned later
  divine-moderation-adapter/    # planned later
migrations/
deploy/
  pds/
docs/
  plans/
  research/
  runbooks/
```

Future refactors may still split shared logic into more focused crates, but `crates/` is the canonical implementation layout until there is a concrete need to reorganize it.

## Milestones

| Milestone | Outcome | Owner | Exit criteria |
| --- | --- | --- | --- |
| M0 | Canonical design locked | TBD | This plan approved and contradictory docs marked non-canonical |
| M1 | Identity and control plane working | TBD | Users can opt in and receive linked DID + handle state |
| M2 | PDS and bridge publish path working | TBD | A NIP-71 post becomes a renderable AT post reliably |
| M3 | Replay, delete, and profile sync working | TBD | Rebuild and delete workflows verified end to end |
| M4 | Discovery surfaces live | TBD | Feed generator returns valid feed skeletons |
| M5 | Moderation path live | TBD | Labeler and inbound moderation queue verified |
| M6 | Limited public rollout | TBD | Launch checklist complete and success metrics monitored |

## Success Metrics

- Mirrored publish median latency under 60 seconds
- Mirrored publish success rate at or above 95 percent
- Replay does not duplicate AT records for already-processed Nostr events
- Delete propagation succeeds for all mapped records in test and staging
- At least one DiVine-owned feed is subscribable in AT clients in phase 2
- Cross-network moderation queue SLA under 24 hours in phase 3

## Chunk 1: Canonical Planning And Repository Skeleton

### Task 1: Lock The Canonical Plan

**Files:**
- Create: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
- Modify: `docs/research/2026-03-20-divine-atproto-technical-spec.md`
- Modify: `docs/research/2026-03-20-divine-atproto-product-plan.md`
- Modify: `docs/research/2026-03-20-divine-atproto-architecture-diagram.md`

- [ ] **Step 1: Add a canonical-source note to the older research docs**

Update each supporting document to point readers back to this plan.

- [ ] **Step 2: Verify the canonical file path exists**

Run: `test -f docs/plans/2026-03-20-divine-atproto-unified-plan.md`
Expected: exit code `0`

- [ ] **Step 3: Record the source hierarchy in a short runbook**

Create: `docs/runbooks/source-of-truth.md`

- [ ] **Step 4: Commit the documentation lock**

```bash
git add docs/plans/2026-03-20-divine-atproto-unified-plan.md docs/research docs/runbooks/source-of-truth.md
git commit -m "docs: establish canonical divine atproto plan"
```

### Task 2: Promote And Extend The Rust Workspace

**Files:**
- Create: `Cargo.toml`
- Create: `Cargo.lock`
- Create: `config/docker-compose.yml`
- Create: `crates/divine-atbridge/Cargo.toml`
- Create: `crates/divine-atbridge/src/main.rs`
- Create: `crates/divine-bridge-db/Cargo.toml`
- Create: `crates/divine-bridge-db/src/lib.rs`
- Create: `crates/divine-bridge-types/Cargo.toml`
- Create: `crates/divine-bridge-types/src/lib.rs`
- Create: `crates/divine-video-worker/Cargo.toml`
- Create: `crates/divine-video-worker/src/main.rs`
- Create: `migrations/001_bridge_tables/up.sql`
- Create: `migrations/001_bridge_tables/down.sql`
- Create: `deploy/pds/docker-compose.yml`
- Create: `deploy/pds/env.example`
- Create: `deploy/pds/README.md`

- [ ] **Step 1: Promote the existing Cargo workspace into the repository root**

Copy the current implementation from the worktree into the root repo, excluding scratch files and build artifacts.

- [ ] **Step 2: Keep future implementation in the promoted `crates/` workspace**

Add new services as sibling crates such as `crates/divine-handle-gateway`, `crates/divine-feedgen`, and `crates/divine-moderation-adapter` instead of reintroducing a parallel layout.

- [ ] **Step 3: Make the workspace compile before feature work**

Run: `cargo check --workspace`
Expected: exit code `0`

- [ ] **Step 4: Commit the baseline skeleton**

```bash
git add Cargo.toml Cargo.lock crates config migrations deploy/pds
git commit -m "chore: scaffold atproto integration workspace"
```

## Chunk 2: Identity, Consent, And Handle Resolution

### Task 3: Implement Account Linking And Consent State

**Files:**
- Create: `libs/identity-linking/src/model.rs`
- Create: `libs/identity-linking/src/store.rs`
- Create: `libs/identity-linking/tests/account_links.rs`
- Create: `services/handle-gateway/src/routes/account_link.rs`
- Create: `infra/postgres/migrations/0001_account_links.sql`

- [ ] **Step 1: Write the failing identity-link store tests**

Run: `cargo test -p identity-linking account_links -- --nocapture`
Expected: FAIL because store and schema are not implemented

- [ ] **Step 2: Add the `account_links` schema and store**

Implement the canonical fields from this plan.

- [ ] **Step 3: Expose API routes for opt-in, disable, export, and status**

Routes should be owned by the `handle-gateway` service, which can sit behind `login.divine.video`.

- [ ] **Step 4: Re-run the identity tests**

Run: `cargo test -p identity-linking account_links -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit the identity-link layer**

```bash
git add libs/identity-linking services/handle-gateway infra/postgres/migrations/0001_account_links.sql
git commit -m "feat: add account linking and consent state"
```

### Task 4: Implement Handle Resolution And Login Control Plane Contracts

**Files:**
- Create: `services/handle-gateway/src/routes/well_known.rs`
- Create: `services/handle-gateway/src/routes/txt_lookup.rs`
- Create: `services/handle-gateway/tests/handle_resolution.rs`
- Create: `docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Write failing tests for `.well-known/atproto-did` resolution**

Run: `cargo test -p handle-gateway handle_resolution -- --nocapture`
Expected: FAIL because the route is missing

- [ ] **Step 2: Implement host-based lookup for `username.divine.video`**

Return the linked DID from the stored account-link state.

- [ ] **Step 3: Document the `login.divine.video` responsibilities**

Cover consent, provisioning trigger, recovery, export, and disable behavior.

- [ ] **Step 4: Re-run the handle tests**

Run: `cargo test -p handle-gateway handle_resolution -- --nocapture`
Expected: PASS

## Chunk 3: PDS Provisioning And Publish Path

### Task 5: Stand Up PDS Configuration And Provisioning Flow

**Files:**
- Create: `infra/pds/docker-compose.yml`
- Create: `infra/pds/env/pds.example.env`
- Create: `services/handle-gateway/src/routes/provision.rs`
- Create: `docs/runbooks/pds-operations.md`

- [ ] **Step 1: Add containerized local dependencies for PostgreSQL and object storage**

Run: `docker compose -f infra/pds/docker-compose.yml up -d`
Expected: PostgreSQL and object storage become healthy

- [ ] **Step 2: Document required PDS environment variables**

Include hostname, PLC URL, object-store settings, and service DID settings.

- [ ] **Step 3: Implement the provisioning trigger from the control plane**

This step creates linked account state and hands off repo provisioning to the PDS path.

- [ ] **Step 4: Verify local infrastructure boots**

Run: `docker compose -f infra/pds/docker-compose.yml ps`
Expected: required services show `running` or `healthy`

### Task 6: Build The NIP-71 To ATProto Mapping Library

**Files:**
- Create: `libs/protocol-mapping/src/nip71.rs`
- Create: `libs/protocol-mapping/src/atproto.rs`
- Create: `libs/protocol-mapping/src/rkey.rs`
- Create: `libs/protocol-mapping/tests/nip71_to_bsky.rs`

- [ ] **Step 1: Write failing tests for text, facets, and `rkey` generation**

Run: `cargo test -p protocol-mapping nip71_to_bsky -- --nocapture`
Expected: FAIL because mapping functions are not implemented

- [ ] **Step 2: Implement NIP-71 parsing and AT post construction**

Include hashtag facets, original `createdAt`, alt text, aspect ratio, and `d` tag `rkey` logic.

- [ ] **Step 3: Add invalid-`d` fallback tests**

Ensure deterministic hash fallback is stable.

- [ ] **Step 4: Re-run mapping tests**

Run: `cargo test -p protocol-mapping nip71_to_bsky -- --nocapture`
Expected: PASS

### Task 7: Implement Relay Consumption, Blob Upload, And Record Publishing

**Files:**
- Create: `services/atbridge/src/consumer.rs`
- Create: `services/atbridge/src/publisher.rs`
- Create: `services/atbridge/src/blob_worker.rs`
- Create: `services/atbridge/src/replay.rs`
- Create: `services/atbridge/tests/publish_pipeline.rs`
- Create: `infra/postgres/migrations/0002_record_mappings.sql`
- Create: `infra/postgres/migrations/0003_asset_manifest.sql`

- [ ] **Step 1: Write the failing end-to-end publish pipeline test**

Run: `cargo test -p atbridge publish_pipeline -- --nocapture`
Expected: FAIL because consumer, uploader, and publisher are not implemented

- [ ] **Step 2: Implement signature verification and link-state gating**

Only linked and enabled users should be mirrored.

- [ ] **Step 3: Implement Blossom fetch plus hash verification**

Track source SHA-256 and destination CID lineage in `asset_manifest`.

- [ ] **Step 4: Implement record publishing and mapping persistence**

Persist `nostr_event_id`, `at_uri`, `collection`, `rkey`, `cid`, and publish status.

- [ ] **Step 5: Re-run the pipeline test**

Run: `cargo test -p atbridge publish_pipeline -- --nocapture`
Expected: PASS

## Chunk 4: Replay, Deletes, Profiles, And Rollout

### Task 8: Add Replay Safety, Delete Propagation, And Profile Sync

**Files:**
- Create: `libs/replay-state/src/store.rs`
- Create: `libs/replay-state/tests/replay_offsets.rs`
- Create: `services/atbridge/src/delete_handler.rs`
- Create: `services/atbridge/src/profile_sync.rs`
- Create: `infra/postgres/migrations/0004_ingest_offsets.sql`

- [ ] **Step 1: Write failing replay-offset tests**

Run: `cargo test -p replay-state replay_offsets -- --nocapture`
Expected: FAIL because offset storage is not implemented

- [ ] **Step 2: Implement durable replay offsets**

Offsets must only advance after AT writes complete successfully.

- [ ] **Step 3: Implement NIP-09 delete handling**

Delete the mapped AT record and preserve tombstoned provenance.

- [ ] **Step 4: Implement one-way profile sync from Nostr kind `0`**

Map display name, bio, avatar, banner, and website.

- [ ] **Step 5: Re-run replay and bridge tests**

Run: `cargo test --workspace`
Expected: PASS

### Task 9: Add Feed Generation, Moderation Hooks, And Launch Checks

**Files:**
- Create: `services/feedgen/src/skeleton.rs`
- Create: `services/feedgen/tests/trending_feed.rs`
- Create: `services/moderation-adapter/src/labels.rs`
- Create: `services/moderation-adapter/tests/label_mapping.rs`
- Create: `docs/runbooks/launch-checklist.md`

- [ ] **Step 1: Write failing tests for feed skeleton generation**

Run: `cargo test -p feedgen trending_feed -- --nocapture`
Expected: FAIL because feed skeleton logic is not implemented

- [ ] **Step 2: Implement latest and trending feed skeleton endpoints**

Start with DiVine-owned data only; do not require full firehose ingestion.

- [ ] **Step 3: Write failing tests for moderation label mapping**

Run: `cargo test -p moderation-adapter label_mapping -- --nocapture`
Expected: FAIL because mappings are not implemented

- [ ] **Step 4: Implement DiVine label translation and moderation queue hooks**

Keep inbound moderation human-reviewed.

- [ ] **Step 5: Add a launch checklist**

Cover BGS crawl enablement, rate-limit review, alerting, rollback, and DMCA handling.

- [ ] **Step 6: Run the full verification suite**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all commands exit `0`

- [ ] **Step 7: Commit rollout readiness**

```bash
git add services/feedgen services/moderation-adapter docs/runbooks/launch-checklist.md
git commit -m "feat: add discovery, moderation, and launch readiness"
```

## Rollout Order

1. Internal developer-only publish tests
2. Private creator cohort with opt-in only
3. Public mirroring for opted-in users
4. Feed-generator launch
5. Labeler and inbound moderation launch

## Deferred Work

- Public `video.divine.*` provenance lexicon
- Bidirectional likes, reposts, or replies
- Translation of non-video Nostr event kinds
- Shared-object-store dedup between Blossom and AT blob storage

## Notes For Future Editors

- If a future decision invalidates `pds.divine.video`, update this file first, then migrate downstream docs.
- If the implementation stack changes away from Rust or `rsky-pds`, preserve the architectural contract before changing task details.
- Do not reintroduce competing canonical specs. Update this file instead.
