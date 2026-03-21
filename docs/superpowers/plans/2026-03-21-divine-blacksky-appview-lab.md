# Divine Blacksky AppView Lab Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Divine-only local ATProto read stack that indexes every repo hosted on the Divine PDS, serves appview-style endpoints, powers a tiny JS viewer, and hardens prospective Divine publishing so new posts, profiles, and media behavior line up more closely with official ATProto and Bluesky expectations.

**Architecture:** Keep the existing PDS and bridge path as the write/source-of-truth plane, but harden that write path first: new posts use validation-enabled `createRecord` with TID-backed keys, video blobs are normalized to spec-friendly MP4 before publish, and profiles use official fields. Then add an additive `deploy/appview-lab/` stack with PostgreSQL, the external `divinevideo/rsky-relay` image, a new `divine-appview-indexer` crate, a read-facing media-derivation worker built from `divine-video-worker`, a new `divine-appview` Axum service, the existing `divine-feedgen` crate backed by indexed data, and a small JS viewer under `apps/divine-blacksky-viewer/`.

**Tech Stack:** Docker Compose, PostgreSQL, Rust, Axum, Diesel, Bash, WebSocket firehose consumption, Vite, TypeScript, HTML5 video, HLS-friendly media derivation.

---

## Scope And Guardrails

- This is a second local dev path, not a replacement for `config/docker-compose.yml`.
- The Divine PDS remains the source of truth. The new stack is read-only from the viewer's point of view.
- Do not vendor `rsky` source code into this repo.
- Use the external `divinevideo/rsky-relay` fork via image or checked-out dependency configuration only.
- Backfill all repos on the configured Divine PDS on first boot.
- Use real Divine data already on the PDS as the must-pass acceptance anchor.
- The first milestone is read-only in the viewer: no login, no repo writes, no network-wide crawl.
- Historical Divine posts already on the PDS are indexed as-is. The publish hardening in this plan applies to new writes moving forward.
- The viewer is intentionally small. Do not drift into `divine-web` scope.

## Planned Repository Layout

```text
deploy/
  appview-lab/
    README.md
    docker-compose.yml
    env.example
crates/
  divine-atbridge/
    src/
      pipeline.rs
      profile_sync.rs
      publisher.rs
      translator.rs
    tests/
      post_record_contract.rs
      profile_record_contract.rs
      publish_path_integration.rs
  divine-video-worker/
    src/
      blob_upload.rs
      derivatives.rs
      main.rs
      normalize.rs
      profile_image.rs
    tests/
      derivatives.rs
      normalize.rs
  divine-appview-indexer/
    Cargo.toml
    src/
      config.rs
      lib.rs
      main.rs
      pds_client.rs
      relay.rs
      store.rs
      sync.rs
    tests/
      backfill.rs
      relay_refresh.rs
  divine-appview/
    Cargo.toml
    src/
      config.rs
      lib.rs
      main.rs
      routes/
        actor.rs
        feed.rs
        health.rs
        search.rs
      store.rs
      views.rs
    tests/
      actor.rs
      feed.rs
      search.rs
apps/
  divine-blacksky-viewer/
    package.json
    vite.config.ts
    tsconfig.json
    index.html
    src/
      api.ts
      App.tsx
      main.tsx
      styles.css
      components/
        AuthorPage.tsx
        FeedSwitcher.tsx
        PostDetail.tsx
        SearchBar.tsx
        VideoCard.tsx
migrations/
  003_appview_read_model/
    up.sql
    down.sql
scripts/
  appview-lab-up.sh
  appview-lab-down.sh
  appview-lab-smoke.sh
```

## Acceptance Anchors

These real fixtures must show up through the local read path before the work is complete:

- DID: `did:plc:ebt5msdpfavoklkap6gl54bm`
- Post URI: `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB`
- Post URI: `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU`

Prospective write-path verification must also pass before this work is considered complete:

- new bridged posts use validation-enabled `createRecord`
- new bridged posts do not derive their post rkeys from Nostr `d` tags or raw event IDs
- non-MP4 input is normalized or rejected before publish
- profile records use official fields such as `website`

## Chunk 1: Writer Contract Hardening

### Task 1: Use Validation-Enabled Post Creation And Persist Returned TIDs

**Files:**
- Create: `crates/divine-atbridge/tests/post_record_contract.rs`
- Modify: `crates/divine-atbridge/src/publisher.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/translator.rs`
- Modify: `crates/divine-atbridge/tests/publish_path_integration.rs`

- [ ] **Step 1: Write the failing contract test**

```rust
#[tokio::test]
async fn new_video_posts_use_create_record_with_validate_true_and_no_custom_rkey() {
    let request = capture_post_write_request().await;

    assert_eq!(request.path, "/xrpc/com.atproto.repo.createRecord");
    assert_eq!(request.body["validate"], true);
    assert!(request.body.get("rkey").is_none());
}
```

- [ ] **Step 2: Run the new test and verify it fails**

Run: `cargo test -p divine-atbridge --test post_record_contract new_video_posts_use_create_record_with_validate_true_and_no_custom_rkey -- --nocapture`
Expected: FAIL because the bridge currently uses `putRecord` with a derived rkey.

- [ ] **Step 3: Implement the post write contract**

Update the publish path so:

- new `app.bsky.feed.post` writes use `createRecord`
- the request body includes `"validate": true`
- the bridge does not derive the post rkey from Nostr metadata
- the bridge persists the returned AT-URI and rkey for later delete handling
- historical read-only indexing keeps accepting legacy non-TID posts already on the Divine PDS

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p divine-atbridge --test post_record_contract -- --nocapture`
Expected: PASS

Run: `cargo test -p divine-atbridge --test publish_path_integration publish_path_integration_processes_video_event_through_http_collaborators -- --nocapture`
Expected: PASS with the mocked write path now asserting `createRecord`.

- [ ] **Step 5: Run focused compile verification**

Run: `cargo check -p divine-atbridge`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-atbridge/tests/post_record_contract.rs crates/divine-atbridge/src/publisher.rs crates/divine-atbridge/src/pipeline.rs crates/divine-atbridge/src/translator.rs crates/divine-atbridge/tests/publish_path_integration.rs
git commit -m "fix: harden divine post creation contract"
```

### Task 2: Enforce Video And Profile Lexicon Constraints

**Files:**
- Create: `crates/divine-atbridge/tests/profile_record_contract.rs`
- Create: `crates/divine-video-worker/src/normalize.rs`
- Create: `crates/divine-video-worker/src/profile_image.rs`
- Create: `crates/divine-video-worker/tests/normalize.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/profile_sync.rs`
- Modify: `crates/divine-atbridge/src/translator.rs`
- Modify: `crates/divine-video-worker/src/main.rs`

- [ ] **Step 1: Write the failing media and profile tests**

```rust
#[tokio::test]
async fn non_mp4_source_is_normalized_before_video_publish() {
    let result = prepare_publishable_video(fake_webm_bytes(), "video/webm").await.unwrap();
    assert_eq!(result.mime_type, "video/mp4");
}
```

```rust
#[test]
fn profile_record_uses_website_field_instead_of_appending_to_description() {
    let record = build_profile_record(&parsed_profile_with_website(), None, None);
    assert_eq!(record["website"], "https://divine.video");
    assert!(!record["description"].as_str().unwrap().contains("Website:"));
}
```

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p divine-atbridge --test profile_record_contract -- --nocapture`
Expected: FAIL because profile records still stuff the website into `description`.

Run: `cargo test -p divine-video-worker -- --nocapture`
Expected: FAIL because video normalization helpers do not exist yet.

- [ ] **Step 3: Implement media and profile hardening**

Implement:

- video normalization helpers that either produce MP4 output or fail early
- size and MIME checks before `app.bsky.embed.video` publish
- profile image normalization for avatar/banner constraints
- profile record output that uses `website` and `createdAt`
- post translation that omits `langs` when the source language is unknown

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p divine-atbridge --test profile_record_contract -- --nocapture`
Expected: PASS

Run: `cargo test -p divine-video-worker -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run focused compile verification**

Run: `cargo check -p divine-atbridge -p divine-video-worker`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-atbridge/tests/profile_record_contract.rs crates/divine-atbridge/src/pipeline.rs crates/divine-atbridge/src/profile_sync.rs crates/divine-atbridge/src/translator.rs crates/divine-video-worker/src/normalize.rs crates/divine-video-worker/src/profile_image.rs crates/divine-video-worker/src/main.rs crates/divine-video-worker/tests/normalize.rs
git commit -m "fix: enforce divine media and profile constraints"
```

## Chunk 2: Read Model And Media View State

### Task 3: Add The AppView Read-Model Migration And Database Helpers

**Files:**
- Create: `migrations/003_appview_read_model/up.sql`
- Create: `migrations/003_appview_read_model/down.sql`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Create: `crates/divine-atbridge/tests/appview_lab_contract.rs`

- [ ] **Step 1: Add the failing schema contract test**

```rust
#[test]
fn appview_schema_includes_media_view_tables_and_queries() {
    let up = std::fs::read_to_string(repo_root().join("migrations/003_appview_read_model/up.sql")).unwrap();
    let queries = std::fs::read_to_string(repo_root().join("crates/divine-bridge-db/src/queries.rs")).unwrap();

    for table in ["appview_repos", "appview_profiles", "appview_posts", "appview_media_views", "appview_service_state"] {
        assert!(up.contains(table), "missing {table}");
    }

    assert!(queries.contains("upsert_appview_media_view"));
    assert!(queries.contains("load_post_with_media_view"));
}
```

- [ ] **Step 2: Run the schema contract test and verify it fails**

Run: `cargo test -p divine-atbridge appview_schema_includes_media_view_tables_and_queries -- --nocapture`
Expected: FAIL because the migration and media-view helpers do not exist yet.

- [ ] **Step 3: Add the migration**

Implement `migrations/003_appview_read_model/up.sql` with:

- `appview_repos`
- `appview_profiles`
- `appview_posts`
- `appview_media_views`
- `appview_service_state`
- indexes for author feed ordering, blob lookup, and text search

Implement `down.sql` to drop those tables in reverse dependency order.

- [ ] **Step 4: Extend `divine-bridge-db`**

Update `schema.rs`, `models.rs`, and `queries.rs` so the read stack can:

- upsert repos
- upsert profiles with `website` and media CIDs
- upsert posts with blob-CID metadata
- upsert media-view rows with playlist and thumbnail URLs
- fetch profiles by handle or DID
- fetch author feeds with cursor pagination
- fetch posts by URI with joined media-view state
- search post text
- store and read service-state cursors

- [ ] **Step 5: Run workspace compile verification**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Re-run the schema contract test**

Run: `cargo test -p divine-atbridge appview_schema_includes_media_view_tables_and_queries -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add migrations/003_appview_read_model crates/divine-bridge-db/src/schema.rs crates/divine-bridge-db/src/models.rs crates/divine-bridge-db/src/queries.rs crates/divine-atbridge/tests/appview_lab_contract.rs
git commit -m "feat: add appview media view schema"
```

## Chunk 3: Divine-Scoped Indexing And Media Derivation

### Task 4: Add The Divine-Scoped AppView Indexer

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/divine-appview-indexer/Cargo.toml`
- Create: `crates/divine-appview-indexer/src/config.rs`
- Create: `crates/divine-appview-indexer/src/lib.rs`
- Create: `crates/divine-appview-indexer/src/main.rs`
- Create: `crates/divine-appview-indexer/src/pds_client.rs`
- Create: `crates/divine-appview-indexer/src/relay.rs`
- Create: `crates/divine-appview-indexer/src/store.rs`
- Create: `crates/divine-appview-indexer/src/sync.rs`
- Create: `crates/divine-appview-indexer/tests/backfill.rs`
- Create: `crates/divine-appview-indexer/tests/relay_refresh.rs`

- [ ] **Step 1: Write the failing backfill test**

```rust
#[tokio::test]
async fn backfill_syncs_profiles_posts_and_blob_metadata_from_pds() {
    let pds = FakePdsClient::with_repo(
        "did:plc:ebt5msdpfavoklkap6gl54bm",
        fake_profile_record(),
        vec![fake_post_record("MA6mjTWZKEB"), fake_post_record("hFxlUuKIIqU")],
    );
    let store = MemoryStore::default();

    sync_repo_from_pds(&pds, &store, "did:plc:ebt5msdpfavoklkap6gl54bm")
        .await
        .unwrap();

    assert_eq!(store.posts().len(), 2);
    assert_eq!(store.posts()[0].embed_blob_cid.as_deref(), Some("bafkrei..."));
}
```

- [ ] **Step 2: Write the failing relay-refresh test**

```rust
#[tokio::test]
async fn relay_event_triggers_repo_resync_and_media_queueing() {
    let relay = FakeRelayStream::with_commit_for("did:plc:ebt5msdpfavoklkap6gl54bm");
    let pds = FakePdsClient::with_single_post("did:plc:ebt5msdpfavoklkap6gl54bm", "MA6mjTWZKEB");
    let store = MemoryStore::default();

    run_single_event_loop(&relay, &pds, &store).await.unwrap();

    assert_eq!(store.media_jobs().len(), 1);
}
```

- [ ] **Step 3: Run the new tests and verify they fail**

Run: `cargo test -p divine-appview-indexer -- --nocapture`
Expected: FAIL because the crate and sync code do not exist yet.

- [ ] **Step 4: Implement the indexer crate**

Implement:

- `config.rs` for env-driven config
- `pds_client.rs` for PDS `listRepos` and collection reads
- `store.rs` backed by `divine-bridge-db`
- `sync.rs` for full repo refresh and deletion reconciliation
- `relay.rs` for reading the local firehose and identifying changed DIDs
- `main.rs` for startup:
  - full backfill on boot
  - relay subscription after backfill
  - cursor persistence in `appview_service_state`
  - media-derivation queueing when indexed posts contain video blob CIDs

Important implementation rule:

- use relay events as change notifications
- rehydrate repo state from the Divine PDS
- do not build full CAR parsing into v1

- [ ] **Step 5: Run the indexer tests**

Run: `cargo test -p divine-appview-indexer -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run workspace compile verification**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/divine-appview-indexer
git commit -m "feat: add divine appview indexer"
```

### Task 5: Build Media View Derivation For AppView Playback

**Files:**
- Create: `crates/divine-video-worker/src/derivatives.rs`
- Create: `crates/divine-video-worker/tests/derivatives.rs`
- Modify: `crates/divine-video-worker/src/main.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`

- [ ] **Step 1: Write the failing derivative test**

```rust
#[tokio::test]
async fn derive_media_view_creates_playlist_and_thumbnail_urls() {
    let view = derive_media_view(fake_mp4_asset()).await.unwrap();
    assert!(view.playlist_url.ends_with(".m3u8"));
    assert!(view.thumbnail_url.as_deref().unwrap().ends_with(".jpg"));
}
```

- [ ] **Step 2: Run the derivative tests and verify they fail**

Run: `cargo test -p divine-video-worker derivatives -- --nocapture`
Expected: FAIL because the derivative generator does not exist yet.

- [ ] **Step 3: Implement derivative generation**

Extend `divine-video-worker` so it can:

- take a Divine DID plus blob CID as the media identity
- produce a playlist URL and optional thumbnail URL
- persist the finished media-view row into `appview_media_views`
- be rerun safely for backfilled legacy blobs already on the Divine PDS

The worker can stay lab-simple. The important contract is that appview reads stable view URLs from the database instead of guessing or exposing raw blob fetch endpoints.

- [ ] **Step 4: Run the targeted tests**

Run: `cargo test -p divine-video-worker derivatives -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run focused compile verification**

Run: `cargo check -p divine-video-worker`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-video-worker/src/derivatives.rs crates/divine-video-worker/src/main.rs crates/divine-video-worker/tests/derivatives.rs crates/divine-bridge-db/src/queries.rs
git commit -m "feat: derive appview media playback views"
```

## Chunk 4: AppView Read API And Discovery Feeds

### Task 6: Add The `divine-appview` Read Service

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/divine-appview/Cargo.toml`
- Create: `crates/divine-appview/src/config.rs`
- Create: `crates/divine-appview/src/lib.rs`
- Create: `crates/divine-appview/src/main.rs`
- Create: `crates/divine-appview/src/store.rs`
- Create: `crates/divine-appview/src/views.rs`
- Create: `crates/divine-appview/src/routes/actor.rs`
- Create: `crates/divine-appview/src/routes/feed.rs`
- Create: `crates/divine-appview/src/routes/health.rs`
- Create: `crates/divine-appview/src/routes/search.rs`
- Create: `crates/divine-appview/tests/actor.rs`
- Create: `crates/divine-appview/tests/feed.rs`
- Create: `crates/divine-appview/tests/search.rs`

- [ ] **Step 1: Write the failing route tests**

```rust
#[tokio::test]
async fn get_posts_hydrates_video_embed_view() {
    let app = app_with_store(FakeStore::with_video_post(
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB",
        "https://media.divine.test/playlists/bafkrei-demo.m3u8",
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getPosts?uris=at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

```rust
#[tokio::test]
async fn search_posts_returns_video_views_without_raw_blob_urls() {
    let body = read_search_response(search_app()).await;
    assert!(body.contains("\"playlist\""));
    assert!(!body.contains("com.atproto.sync.getBlob"));
}
```

- [ ] **Step 2: Run the route tests and verify they fail**

Run: `cargo test -p divine-appview -- --nocapture`
Expected: FAIL because the crate and routes do not exist yet.

- [ ] **Step 3: Implement the read API**

Expose:

- `/health`
- `/health/ready`
- `/xrpc/app.bsky.actor.getProfile`
- `/xrpc/app.bsky.feed.getAuthorFeed`
- `/xrpc/app.bsky.feed.getPosts`
- `/xrpc/app.bsky.feed.getPostThread`
- `/xrpc/app.bsky.feed.searchPosts`

Implementation notes:

- use a store abstraction so route tests can stay fast
- resolve `actor` by DID or handle
- paginate author feed and search results with stable cursors
- return readiness failure when the indexer freshness state is missing or stale
- hydrate post views and profile views from the local read model
- return `app.bsky.embed.video#view` for video posts using `appview_media_views`
- add CORS config for the local JS viewer origin

- [ ] **Step 4: Run the route tests**

Run: `cargo test -p divine-appview -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run workspace compile verification**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/divine-appview
git commit -m "feat: add divine appview read service"
```

### Task 7: Replace Static Feedgen Data With Read-Model Queries

**Files:**
- Modify: `crates/divine-feedgen/src/lib.rs`
- Modify: `crates/divine-feedgen/src/skeleton.rs`
- Modify: `crates/divine-feedgen/tests/feed_skeleton.rs`

- [ ] **Step 1: Write the failing feedgen tests**

Extend `crates/divine-feedgen/tests/feed_skeleton.rs` so `latest` and `trending` are asserted against injected indexed rows instead of hard-coded placeholder URIs.

Example shape:

```rust
#[tokio::test]
async fn feed_skeleton_latest_reads_indexed_posts() {
    let app = app_with_store(FakeFeedStore::with_latest(vec![
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB",
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU",
    ]));

    // GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=.../latest
}
```

- [ ] **Step 2: Run the feedgen tests and verify they fail**

Run: `cargo test -p divine-feedgen -- --nocapture`
Expected: FAIL because the service still returns static demo values.

- [ ] **Step 3: Implement read-model-backed discovery**

Update `divine-feedgen` so:

- `latest` returns newest indexed Divine posts
- `trending` uses a deterministic lab heuristic:
  - rank by any indexed engagement fields that actually exist
  - otherwise fall back to recency
- the response stays skeleton-only and returns post AT-URIs, not hydrated media or profile data

- [ ] **Step 4: Run the feedgen tests**

Run: `cargo test -p divine-feedgen -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run workspace compile verification**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-feedgen/src/lib.rs crates/divine-feedgen/src/skeleton.rs crates/divine-feedgen/tests/feed_skeleton.rs
git commit -m "feat: back feedgen with appview read model"
```

## Chunk 5: Viewer, Local Orchestration, And End-To-End Verification

### Task 8: Add The AppView Lab Compose Stack, Viewer, And Operator Smoke

**Files:**
- Create: `deploy/appview-lab/README.md`
- Create: `deploy/appview-lab/docker-compose.yml`
- Create: `deploy/appview-lab/env.example`
- Create: `apps/divine-blacksky-viewer/package.json`
- Create: `apps/divine-blacksky-viewer/vite.config.ts`
- Create: `apps/divine-blacksky-viewer/tsconfig.json`
- Create: `apps/divine-blacksky-viewer/index.html`
- Create: `apps/divine-blacksky-viewer/src/api.ts`
- Create: `apps/divine-blacksky-viewer/src/App.tsx`
- Create: `apps/divine-blacksky-viewer/src/main.tsx`
- Create: `apps/divine-blacksky-viewer/src/styles.css`
- Create: `apps/divine-blacksky-viewer/src/components/AuthorPage.tsx`
- Create: `apps/divine-blacksky-viewer/src/components/FeedSwitcher.tsx`
- Create: `apps/divine-blacksky-viewer/src/components/PostDetail.tsx`
- Create: `apps/divine-blacksky-viewer/src/components/SearchBar.tsx`
- Create: `apps/divine-blacksky-viewer/src/components/VideoCard.tsx`
- Create: `scripts/appview-lab-up.sh`
- Create: `scripts/appview-lab-down.sh`
- Create: `scripts/appview-lab-smoke.sh`
- Create: `docs/runbooks/appview-lab.md`
- Modify: `README.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/pds-operations.md`

- [ ] **Step 1: Write the failing contract and smoke tests**

Create:

- a frontend smoke that asserts feed cards render from mocked appview and feedgen responses
- a shell smoke that asserts:
  - the demo profile DID resolves
  - author feed includes `MA6mjTWZKEB` and `hFxlUuKIIqU`
  - search returns both demo posts
  - the hydrated post view includes a `playlist` field

- [ ] **Step 2: Run the build and smoke checks and verify they fail**

Run: `cd apps/divine-blacksky-viewer && npm install && npm run build`
Expected: FAIL because the app does not exist yet.

Run: `bash scripts/appview-lab-smoke.sh`
Expected: FAIL because the stack and smoke script do not exist yet.

- [ ] **Step 3: Implement the appview lab runtime**

Implement `deploy/appview-lab/docker-compose.yml` with:

- `postgres`
- `relay` using `${RSKY_RELAY_IMAGE:-ghcr.io/divinevideo/rsky-relay:latest}`
- `indexer` as a source-mounted Rust container running `cargo run -p divine-appview-indexer`
- `media-worker` as a source-mounted Rust container running the media-derivation job
- `appview` as a source-mounted Rust container running `cargo run -p divine-appview`
- `feedgen` as a source-mounted Rust container running `cargo run -p divine-feedgen`
- `viewer` as a Node container running the JS app

The compose file must keep the PDS external and accept `DIVINE_PDS_URL` from env.

- [ ] **Step 4: Implement the lightweight viewer**

The viewer should provide:

- a latest feed view using `divine-feedgen`
- a trending feed view using `divine-feedgen`
- author pages using `app.bsky.actor.getProfile` plus `getAuthorFeed`
- post detail using `app.bsky.feed.getPostThread` or `getPosts`
- search using `app.bsky.feed.searchPosts`
- HTML5 playback from the returned `playlist` URL

Keep the app intentionally small. No auth, no mutations, no styling sprawl.

- [ ] **Step 5: Document operator flow**

Write `deploy/appview-lab/README.md`, `docs/runbooks/appview-lab.md`, and update the existing runbooks with:

1. required env setup
2. compose startup
3. expected health endpoints
4. smoke command
5. viewer URL
6. troubleshooting for stale index, relay lag, and missing playlist derivatives

- [ ] **Step 6: Validate the lab**

Run: `docker compose -f deploy/appview-lab/docker-compose.yml --env-file deploy/appview-lab/env.example config`
Expected: exit code `0`

Run: `cd apps/divine-blacksky-viewer && npm install && npm run build`
Expected: PASS

Run: `bash scripts/appview-lab-up.sh`
Expected: compose services start successfully

Run: `bash scripts/appview-lab-smoke.sh`
Expected: PASS and explicitly mention:
- `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB`
- `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU`

- [ ] **Step 7: Run final verification**

Run: `cargo check --workspace`
Expected: PASS

Run: `bash scripts/test-workspace.sh`
Expected: PASS

Run: `cd apps/divine-blacksky-viewer && npm run build`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add deploy/appview-lab apps/divine-blacksky-viewer scripts/appview-lab-up.sh scripts/appview-lab-down.sh scripts/appview-lab-smoke.sh docs/runbooks/appview-lab.md README.md docs/runbooks/dev-bootstrap.md docs/runbooks/pds-operations.md
git commit -m "feat: add divine appview lab runtime and viewer"
```

## Execution Notes

- The external relay behavior belongs in `divinevideo/rsky-relay`, not in this repository.
- If the relay fork needs Divine-only flags or host allowlisting, document those flags in `deploy/appview-lab/env.example` and `deploy/appview-lab/README.md`, but keep the implementation in the fork.
- Treat the media-view layer as a first-class contract. Do not leak raw `getBlob` URLs into viewer code just because it is easy.
- Keep the first appview indexer simple and explicit: relay events indicate which DID changed, and the indexer re-reads the relevant PDS records instead of trying to decode the full firehose payload into a generic network-wide dataplane.
- Do not broaden scope into auth, likes, replies, or wider-network indexing before the known demo posts render end to end.
- Do not rewrite legacy fixture URIs as part of this plan. Index them correctly, but make new bridge publishes spec-friendlier going forward.
