# DiVine Cross-Network View Analytics Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add creator-visible `Verified Plays` and `Estimated Reach` for DiVine short videos by enforcing a hard `<= 6.3s` media probe gate in the bridge, storing raw observation evidence, importing CDN deliveries, accepting trusted external reports, and recomputing optimistic rollups.

**Architecture:** Keep `divine-atbridge` as the source-of-truth Nostr -> ATProto publish path, but extend it to probe actual media duration and persist an explicit record-to-asset join. Add a new `divine-view-analytics` service that ingests first-party qualified play events, CDN delivery batches from DiVine Blossom or the CDN, and trusted external reports, then materializes `Verified Plays` and `Estimated Reach` from raw observations. Imported or bridge-created users keep explicit `username-domain.bluesky.name` NIP-05 aliases and are never keyed by alias text in analytics.

**Tech Stack:** Rust workspace crates, Axum, Diesel/PostgreSQL, reqwest, ffprobe or equivalent media-duration probe wrapper, scheduled rollup jobs, Blossom or CDN delivery batch ingestion, existing bridge lineage tables.

---

## Planned File Structure

- Modify: `Cargo.toml`
  Add a new analytics service crate and any shared dependency updates.
- Create: `migrations/003_view_analytics_tables/up.sql`
- Create: `migrations/003_view_analytics_tables/down.sql`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-bridge-db/tests/view_analytics.rs`
- Create: `crates/divine-bridge-types/src/view_analytics.rs`
- Modify: `crates/divine-bridge-types/src/lib.rs`
- Modify: `crates/divine-atbridge/Cargo.toml`
- Modify: `crates/divine-atbridge/src/config.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Create: `crates/divine-atbridge/tests/video_duration_gate.rs`
- Modify: `crates/divine-atbridge/tests/media_lineage.rs`
- Modify: `crates/divine-video-worker/Cargo.toml`
- Create: `crates/divine-video-worker/src/lib.rs`
- Create: `crates/divine-video-worker/src/probe.rs`
- Modify: `crates/divine-video-worker/src/main.rs`
- Create: `crates/divine-view-analytics/Cargo.toml`
- Create: `crates/divine-view-analytics/src/lib.rs`
- Create: `crates/divine-view-analytics/src/main.rs`
- Create: `crates/divine-view-analytics/src/config.rs`
- Create: `crates/divine-view-analytics/src/store.rs`
- Create: `crates/divine-view-analytics/src/fingerprint.rs`
- Create: `crates/divine-view-analytics/src/rollup.rs`
- Create: `crates/divine-view-analytics/src/routes/mod.rs`
- Create: `crates/divine-view-analytics/src/routes/first_party.rs`
- Create: `crates/divine-view-analytics/src/routes/cdn_batches.rs`
- Create: `crates/divine-view-analytics/src/routes/external_reports.rs`
- Create: `crates/divine-view-analytics/src/routes/stats.rs`
- Create: `crates/divine-view-analytics/tests/first_party_ingest.rs`
- Create: `crates/divine-view-analytics/tests/cdn_batches.rs`
- Create: `crates/divine-view-analytics/tests/external_reports.rs`
- Create: `crates/divine-view-analytics/tests/rollups.rs`
- Modify: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
- Modify: `docs/research/2026-03-20-divine-atproto-technical-spec.md`
- Modify: `docs/runbooks/media-path.md`
- Modify: `docs/runbooks/login-divine-video.md`
- Create: `docs/runbooks/view-analytics.md`
- Create: `docs/runbooks/view-analytics-smoke-test.md`

## Scope Guard

This plan only covers short-form DiVine-origin video that is eligible for outbound mirroring.

- Do not add long-form support.
- Do not add synthetic Nostr social actions from AT analytics.
- Do not try to infer exact unique users across all AT clients.
- Do not make imported users look like native `username.divine.video` accounts.

## Chunk 1: Canonical Rules And Persistence

### Task 1: Lock The Product Rules In Canonical Docs

**Files:**
- Modify: `docs/plans/2026-03-20-divine-atproto-unified-plan.md`
- Modify: `docs/research/2026-03-20-divine-atproto-technical-spec.md`
- Modify: `docs/runbooks/login-divine-video.md`
- Create: `docs/runbooks/view-analytics.md`

- [ ] **Step 1: Add the approved counter model**
  Document `verified_plays`, `cdn_deliveries`, and `reported_external_views` as internal counters and `Verified Plays` plus `Estimated Reach` as public numbers.
- [ ] **Step 2: Add the hard short-form invariant**
  Record that only media with `canonical_duration_ms <= 6300` is eligible for mirror and analytics.
- [ ] **Step 3: Add the attribution policy**
  Document `trusted_first_party`, `trusted_partner`, `inferred`, and `unknown`.
- [ ] **Step 4: Add the namespace policy**
  Document that imported or bridge-created users get explicit NIP-05 aliases like `username-domain.bluesky.name` and do not claim the plain native `username.divine.video` shape.
- [ ] **Step 5: Commit**

```bash
git add docs/plans/2026-03-20-divine-atproto-unified-plan.md docs/research/2026-03-20-divine-atproto-technical-spec.md docs/runbooks/login-divine-video.md docs/runbooks/view-analytics.md
git commit -m "docs: define cross-network video analytics policy"
```

### Task 2: Add Database Tables For Media Facts, Asset Links, And View Evidence

**Files:**
- Create: `migrations/003_view_analytics_tables/up.sql`
- Create: `migrations/003_view_analytics_tables/down.sql`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-bridge-db/tests/view_analytics.rs`
- Create: `crates/divine-bridge-types/src/view_analytics.rs`
- Modify: `crates/divine-bridge-types/src/lib.rs`

- [ ] **Step 1: Write failing database tests**
  Cover `record_asset_links`, eligible short-video facts on `asset_manifest`, immutable `view_observations`, and bucketed `video_view_rollups`.
- [ ] **Step 2: Run the new tests and verify they fail**

Run: `cargo test -p divine-bridge-db view_analytics -- --nocapture`
Expected: FAIL because the tables and query helpers do not exist yet

- [ ] **Step 3: Add migration tables and columns**
  Add duration and probe fields to `asset_manifest`, create `record_asset_links`, `view_observations`, `video_view_rollups`, and `app_registry`.
- [ ] **Step 4: Add Rust models and query helpers**
  Expose insert and query helpers keyed by `source_sha256`, `nostr_event_id`, `at_uri`, and rollup bucket.
- [ ] **Step 5: Add shared DTOs**
  Create normalized request and storage types for first-party plays, CDN deliveries, external reports, and rollup responses.
- [ ] **Step 6: Re-run the tests and verify they pass**

Run: `cargo test -p divine-bridge-db view_analytics -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add migrations/003_view_analytics_tables crates/divine-bridge-db crates/divine-bridge-types
git commit -m "feat: add view analytics persistence"
```

## Chunk 2: Short-Video Gate In The Publish Path

### Task 3: Add Actual Media Duration Probing

**Files:**
- Modify: `crates/divine-video-worker/Cargo.toml`
- Create: `crates/divine-video-worker/src/lib.rs`
- Create: `crates/divine-video-worker/src/probe.rs`
- Modify: `crates/divine-video-worker/src/main.rs`

- [ ] **Step 1: Write failing probe tests**
  Cover successful duration extraction, malformed media failure, and unsupported mime handling.
- [ ] **Step 2: Run the probe tests and verify they fail**

Run: `cargo test -p divine-video-worker probe -- --nocapture`
Expected: FAIL because no probe module exists yet

- [ ] **Step 3: Implement a probe wrapper**
  Add a small library API that measures duration from actual media bytes and returns `measured_duration_ms`.
- [ ] **Step 4: Keep the probe boundary replaceable**
  Use a trait or isolated wrapper so the bridge can unit test without shelling out to the real media probe in every test.
- [ ] **Step 5: Re-run the probe tests and verify they pass**

Run: `cargo test -p divine-video-worker probe -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-video-worker
git commit -m "feat: add actual media duration probing"
```

### Task 4: Enforce The `<= 6.3s` Gate Before Upload

**Files:**
- Modify: `crates/divine-atbridge/Cargo.toml`
- Modify: `crates/divine-atbridge/src/config.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Create: `crates/divine-atbridge/tests/video_duration_gate.rs`
- Modify: `crates/divine-atbridge/tests/media_lineage.rs`

- [ ] **Step 1: Write failing bridge tests**
  Cover sub-6.3-second success, exact-boundary success, over-limit skip, and probe failure skip.
- [ ] **Step 2: Run the failing bridge tests**

Run: `cargo test -p divine-atbridge video_duration_gate -- --nocapture`
Expected: FAIL because the pipeline does not probe or gate duration yet

- [ ] **Step 3: Add bridge config for max duration**
  Add a default `MAX_VIDEO_DURATION_MS=6300` contract rather than hard-coding magic numbers in the pipeline.
- [ ] **Step 4: Probe before upload**
  After fetch and hash verification, measure actual media duration, reject ineligible media before `uploadBlob`, and persist probe facts on the asset record.
- [ ] **Step 5: Persist the explicit record-to-asset link**
  Save `nostr_event_id`, `at_uri`, `source_sha256`, `blossom_url`, and `at_blob_cid` in `record_asset_links` for each published short video.
- [ ] **Step 6: Re-run the bridge tests and verify they pass**

Run: `cargo test -p divine-atbridge video_duration_gate -- --nocapture`
Expected: PASS

- [ ] **Step 7: Run the existing lineage tests**

Run: `cargo test -p divine-atbridge media_lineage -- --nocapture`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/divine-atbridge
git commit -m "feat: gate mirrored video by measured duration"
```

## Chunk 3: Analytics Service Ingestion

### Task 5: Create The Analytics Service And First-Party Play Ingest

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/divine-view-analytics/Cargo.toml`
- Create: `crates/divine-view-analytics/src/lib.rs`
- Create: `crates/divine-view-analytics/src/main.rs`
- Create: `crates/divine-view-analytics/src/config.rs`
- Create: `crates/divine-view-analytics/src/store.rs`
- Create: `crates/divine-view-analytics/src/fingerprint.rs`
- Create: `crates/divine-view-analytics/src/routes/mod.rs`
- Create: `crates/divine-view-analytics/src/routes/first_party.rs`
- Create: `crates/divine-view-analytics/tests/first_party_ingest.rs`

- [ ] **Step 1: Write failing ingest tests**
  Cover `play_started`, `play_qualified`, eligible-video lookup, fingerprint generation, and idempotent insert behavior.
- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p divine-view-analytics first_party_ingest -- --nocapture`
Expected: FAIL because the service crate does not exist yet

- [ ] **Step 3: Scaffold the service**
  Add Axum app wiring, configuration loading, Diesel store access, and health endpoints.
- [ ] **Step 4: Implement first-party ingest**
  Add a route that accepts normalized play events, resolves the canonical video through `record_asset_links`, rejects ineligible media, and only increments `verified_plays` from `play_qualified`.
- [ ] **Step 5: Re-run the tests and verify they pass**

Run: `cargo test -p divine-view-analytics first_party_ingest -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/divine-view-analytics
git commit -m "feat: add first-party view analytics ingest service"
```

### Task 6: Import CDN Delivery Batches From DiVine Blossom Or The CDN

**Files:**
- Create: `crates/divine-view-analytics/src/routes/cdn_batches.rs`
- Create: `crates/divine-view-analytics/tests/cdn_batches.rs`
- Modify: `crates/divine-view-analytics/src/fingerprint.rs`
- Modify: `crates/divine-view-analytics/src/store.rs`
- Modify: `crates/divine-view-analytics/src/routes/mod.rs`
- Create: `docs/runbooks/view-analytics-smoke-test.md`

- [ ] **Step 1: Write failing CDN ingest tests**
  Cover batch auth, source-hash resolution, duplicate edge retries, and unknown-asset rejection.
- [ ] **Step 2: Run the CDN ingest tests and verify they fail**

Run: `cargo test -p divine-view-analytics cdn_batches -- --nocapture`
Expected: FAIL because the batch route does not exist yet

- [ ] **Step 3: Implement a batch import route**
  Accept normalized delivery rows from DiVine Blossom or the CDN rather than scraping logs directly inside this repo.
- [ ] **Step 4: Apply light dedupe**
  Count one delivery per `video_id + viewer_fingerprint + app classification + time window`.
- [ ] **Step 5: Preserve rejected evidence**
  Store unresolved or malformed rows as raw observations with `accepted = false` and a rejection reason.
- [ ] **Step 6: Re-run the CDN ingest tests and verify they pass**

Run: `cargo test -p divine-view-analytics cdn_batches -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/divine-view-analytics docs/runbooks/view-analytics-smoke-test.md
git commit -m "feat: add cdn delivery ingest for view analytics"
```

### Task 7: Accept Trusted External View Reports

**Files:**
- Create: `crates/divine-view-analytics/src/routes/external_reports.rs`
- Create: `crates/divine-view-analytics/tests/external_reports.rs`
- Modify: `crates/divine-view-analytics/src/routes/mod.rs`
- Modify: `crates/divine-view-analytics/src/store.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`

- [ ] **Step 1: Write failing external-report tests**
  Cover trusted partner auth, replay protection, dedupe, and low-confidence acceptance with `unknown` attribution.
- [ ] **Step 2: Run the external-report tests and verify they fail**

Run: `cargo test -p divine-view-analytics external_reports -- --nocapture`
Expected: FAIL because the external report route does not exist yet

- [ ] **Step 3: Implement reporter verification**
  Resolve `app_registry` entries, verify partner auth, and reject unknown reporters when the route requires trust.
- [ ] **Step 4: Store accepted reports as `reported_external_views`**
  Count accepted reports for eligible videos after replay and dedupe checks.
- [ ] **Step 5: Re-run the external-report tests and verify they pass**

Run: `cargo test -p divine-view-analytics external_reports -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-view-analytics crates/divine-bridge-db/src/queries.rs
git commit -m "feat: add external partner view reporting"
```

## Chunk 4: Rollups And Read APIs

### Task 8: Materialize `Verified Plays` And `Estimated Reach`

**Files:**
- Create: `crates/divine-view-analytics/src/rollup.rs`
- Create: `crates/divine-view-analytics/src/routes/stats.rs`
- Create: `crates/divine-view-analytics/tests/rollups.rs`
- Modify: `crates/divine-view-analytics/src/lib.rs`
- Modify: `crates/divine-view-analytics/src/main.rs`
- Modify: `crates/divine-view-analytics/src/store.rs`

- [ ] **Step 1: Write failing rollup tests**
  Cover overlapping first-party plays and CDN deliveries, overlapping external reports, and public stat responses for one video.
- [ ] **Step 2: Run the rollup tests and verify they fail**

Run: `cargo test -p divine-view-analytics rollups -- --nocapture`
Expected: FAIL because no rollup engine exists yet

- [ ] **Step 3: Implement rollup recomputation**
  Aggregate raw observations into `video_view_rollups` by canonical video and bucket.
- [ ] **Step 4: Implement public stat derivation**
  Return `Verified Plays` directly from `verified_plays` and compute `Estimated Reach` from the union of the three internal counters with overlap suppression.
- [ ] **Step 5: Add stats routes**
  Expose per-video and per-creator read endpoints for internal dashboards and future product surfaces.
- [ ] **Step 6: Re-run the rollup tests and verify they pass**

Run: `cargo test -p divine-view-analytics rollups -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/divine-view-analytics
git commit -m "feat: add derived video analytics rollups"
```

## Chunk 5: Docs, Verification, And Rollout

### Task 9: Finalize Runbooks And Smoke Tests

**Files:**
- Modify: `docs/runbooks/media-path.md`
- Modify: `docs/runbooks/view-analytics.md`
- Modify: `docs/runbooks/view-analytics-smoke-test.md`

- [ ] **Step 1: Document the bridge gate**
  Explain the `<= 6.3s` probe rule, probe failure handling, and skipped-duration behavior.
- [ ] **Step 2: Document the CDN contract**
  Define the normalized delivery batch format DiVine Blossom or the CDN must send into the analytics service.
- [ ] **Step 3: Document the public formulas**
  Explain exactly how `Verified Plays` and `Estimated Reach` are derived.
- [ ] **Step 4: Add an operator smoke test**
  Cover one eligible short video, one too-long skipped video, one qualified first-party play, one deduped CDN delivery, and one accepted external report.
- [ ] **Step 5: Commit**

```bash
git add docs/runbooks/media-path.md docs/runbooks/view-analytics.md docs/runbooks/view-analytics-smoke-test.md
git commit -m "docs: add view analytics runbooks"
```

### Task 10: Verify The Full Workspace Slice

**Files:**
- Modify: `scripts/test-workspace.sh`
  Only if the new crate or tests need to be added to the workspace verification script.

- [ ] **Step 1: Run focused crate tests**

Run: `cargo test -p divine-video-worker probe -- --nocapture`
Expected: PASS

Run: `cargo test -p divine-atbridge video_duration_gate media_lineage -- --nocapture`
Expected: PASS

Run: `cargo test -p divine-view-analytics -- --nocapture`
Expected: PASS

Run: `cargo test -p divine-bridge-db view_analytics -- --nocapture`
Expected: PASS

- [ ] **Step 2: Run compile verification**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 3: Run the workspace script if it still applies cleanly**

Run: `bash scripts/test-workspace.sh`
Expected: PASS or a known unrelated failure documented in the PR notes

- [ ] **Step 4: Commit final verification updates if needed**

```bash
git add scripts/test-workspace.sh
git commit -m "chore: wire view analytics into workspace verification"
```

Plan complete and saved to `docs/superpowers/plans/2026-03-21-divine-cross-network-view-analytics.md`. Ready to execute?
