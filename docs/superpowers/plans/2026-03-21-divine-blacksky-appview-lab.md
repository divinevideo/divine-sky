# Divine Blacksky AppView Lab Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Divine-only local ATProto read stack that indexes every repo hosted on the Divine PDS via a local `divinevideo/rsky-relay`, serves appview-style endpoints, and powers a tiny React video viewer against real PDS data.

**Architecture:** Keep the existing PDS and bridge path as the write/source-of-truth plane. Add an additive `deploy/appview-lab/` stack that runs PostgreSQL, the external `divinevideo/rsky-relay` image, a new `divine-appview-indexer` crate for PDS backfill plus relay-triggered refresh, a new `divine-appview` Axum read service, the existing `divine-feedgen` crate backed by the indexed data, and a small React viewer. The stack is Divine-only: it indexes all repos on the configured Divine PDS and nothing else.

**Tech Stack:** Docker Compose, PostgreSQL, Rust, Axum, Diesel, Bash, WebSocket firehose consumption, Vite, React, TypeScript.

---

## Scope And Guardrails

- This is a second local dev path, not a replacement for `config/docker-compose.yml`.
- The Divine PDS remains the source of truth. The new stack is read-only against it.
- Do not vendor `rsky` source code into this repo.
- Use the external `divinevideo/rsky-relay` fork via image or checked-out dependency configuration only.
- Backfill all repos on the configured Divine PDS on first boot.
- Use real Divine data already on the PDS as the must-pass acceptance anchor.
- The first milestone is read-only: no login, no repo writes, no network-wide crawl.
- Discovery should use the existing `divine-feedgen` crate once it is wired to the read model.

## Planned Repository Layout

```text
deploy/
  appview-lab/
    README.md
    docker-compose.yml
    env.example
crates/
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
  divine-video-viewer/
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
        FeedGrid.tsx
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

## Chunk 1: Contracts, Docs, And Local Orchestration

### Task 1: Define The AppView Lab Contract And Document The Second Dev Path

**Files:**
- Create: `crates/divine-atbridge/tests/appview_lab_contract.rs`
- Create: `deploy/appview-lab/README.md`
- Modify: `README.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/pds-operations.md`

- [ ] **Step 1: Write the failing contract test**

```rust
#[test]
fn appview_lab_docs_and_layout_are_present() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();

    assert!(repo_root.join("deploy/appview-lab/README.md").exists());

    let readme = std::fs::read_to_string(repo_root.join("README.md")).unwrap();
    let bootstrap = std::fs::read_to_string(repo_root.join("docs/runbooks/dev-bootstrap.md")).unwrap();
    let ops = std::fs::read_to_string(repo_root.join("docs/runbooks/pds-operations.md")).unwrap();

    assert!(readme.contains("deploy/appview-lab"));
    assert!(bootstrap.contains("deploy/appview-lab"));
    assert!(ops.contains("divinevideo/rsky-relay"));
}
```

- [ ] **Step 2: Run the new contract test and verify it fails**

Run: `cargo test -p divine-atbridge appview_lab_docs_and_layout_are_present -- --nocapture`
Expected: FAIL because the appview-lab docs and references do not exist yet.

- [ ] **Step 3: Add the lab docs**

Write `deploy/appview-lab/README.md` with:

- the service list: `relay`, `indexer`, `appview`, `feedgen`, `viewer`
- the rule that the Divine PDS stays external and authoritative
- the rule that `divinevideo/rsky-relay` remains an external dependency
- the real acceptance fixtures:
  - `did:plc:ebt5msdpfavoklkap6gl54bm`
  - `MA6mjTWZKEB`
  - `hFxlUuKIIqU`

Update `README.md`, `docs/runbooks/dev-bootstrap.md`, and `docs/runbooks/pds-operations.md` to distinguish:

- the existing bridge/PDS local stack in `config/docker-compose.yml`
- the new Divine-only read lab in `deploy/appview-lab/`

- [ ] **Step 4: Re-run the contract test**

Run: `cargo test -p divine-atbridge appview_lab_docs_and_layout_are_present -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/tests/appview_lab_contract.rs deploy/appview-lab/README.md README.md docs/runbooks/dev-bootstrap.md docs/runbooks/pds-operations.md
git commit -m "docs: define divine appview lab contract"
```

### Task 2: Add The Additive Compose Stack And Helper Scripts

**Files:**
- Create: `deploy/appview-lab/docker-compose.yml`
- Create: `deploy/appview-lab/env.example`
- Create: `scripts/appview-lab-up.sh`
- Create: `scripts/appview-lab-down.sh`
- Modify: `crates/divine-atbridge/tests/appview_lab_contract.rs`

- [ ] **Step 1: Extend the contract test with compose assertions**

```rust
#[test]
fn appview_lab_compose_defines_required_services() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    let compose = std::fs::read_to_string(repo_root.join("deploy/appview-lab/docker-compose.yml")).unwrap();
    let env = std::fs::read_to_string(repo_root.join("deploy/appview-lab/env.example")).unwrap();

    for service in ["postgres:", "relay:", "indexer:", "appview:", "feedgen:", "viewer:"] {
        assert!(compose.contains(service), "missing {service}");
    }

    assert!(compose.contains("RSKY_RELAY_IMAGE"));
    assert!(env.contains("DIVINE_PDS_URL="));
    assert!(env.contains("VIEWER_APPVIEW_URL="));
}
```

- [ ] **Step 2: Run the compose contract test and verify it fails**

Run: `cargo test -p divine-atbridge appview_lab_compose_defines_required_services -- --nocapture`
Expected: FAIL because the compose files do not exist yet.

- [ ] **Step 3: Create the lab compose stack**

Implement `deploy/appview-lab/docker-compose.yml` with:

- `postgres` for the read model
- `relay` using `${RSKY_RELAY_IMAGE:-ghcr.io/divinevideo/rsky-relay:latest}`
- `indexer` as a source-mounted Rust container running `cargo run -p divine-appview-indexer`
- `appview` as a source-mounted Rust container running `cargo run -p divine-appview`
- `feedgen` as a source-mounted Rust container running `cargo run -p divine-feedgen`
- `viewer` as a Node container running the React app

The compose file must keep the PDS external and accept `DIVINE_PDS_URL` from env.

- [ ] **Step 4: Document the env contract**

Write `deploy/appview-lab/env.example` with:

- `DIVINE_PDS_URL`
- `DIVINE_PDS_HOST`
- `APPVIEW_DATABASE_URL`
- `RSKY_RELAY_IMAGE`
- `APPVIEW_CORS_ORIGIN`
- `VIEWER_APPVIEW_URL`
- `VIEWER_FEEDGEN_URL`

- [ ] **Step 5: Add helper scripts**

Write:

- `scripts/appview-lab-up.sh`
- `scripts/appview-lab-down.sh`

They should wrap the compose file and env example cleanly instead of requiring long manual commands.

- [ ] **Step 6: Validate the compose stack**

Run: `docker compose -f deploy/appview-lab/docker-compose.yml --env-file deploy/appview-lab/env.example config`
Expected: exit code `0`

- [ ] **Step 7: Re-run the contract test**

Run: `cargo test -p divine-atbridge appview_lab_compose_defines_required_services -- --nocapture`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add deploy/appview-lab/docker-compose.yml deploy/appview-lab/env.example scripts/appview-lab-up.sh scripts/appview-lab-down.sh crates/divine-atbridge/tests/appview_lab_contract.rs
git commit -m "feat: add divine appview lab compose stack"
```

## Chunk 2: Read-Model Storage And Divine-Scoped Indexing

### Task 3: Add The AppView Read-Model Migration And Database Helpers

**Files:**
- Create: `migrations/003_appview_read_model/up.sql`
- Create: `migrations/003_appview_read_model/down.sql`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`
- Modify: `crates/divine-atbridge/tests/appview_lab_contract.rs`

- [ ] **Step 1: Add a failing schema contract test**

```rust
#[test]
fn appview_lab_schema_files_define_required_tables() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();

    let up = std::fs::read_to_string(repo_root.join("migrations/003_appview_read_model/up.sql")).unwrap();
    let queries = std::fs::read_to_string(repo_root.join("crates/divine-bridge-db/src/queries.rs")).unwrap();

    for table in ["appview_repos", "appview_profiles", "appview_posts", "appview_service_state"] {
        assert!(up.contains(table), "missing {table}");
    }

    assert!(queries.contains("upsert_appview_profile"));
    assert!(queries.contains("search_appview_posts"));
}
```

- [ ] **Step 2: Run the schema contract test and verify it fails**

Run: `cargo test -p divine-atbridge appview_lab_schema_files_define_required_tables -- --nocapture`
Expected: FAIL because the migration and query helpers do not exist yet.

- [ ] **Step 3: Add the migration**

Implement `migrations/003_appview_read_model/up.sql` with:

- `appview_repos`
- `appview_profiles`
- `appview_posts`
- `appview_service_state`
- indexes for author feed ordering and text search

Implement `down.sql` to drop those tables in reverse dependency order.

- [ ] **Step 4: Extend `divine-bridge-db`**

Update `schema.rs`, `models.rs`, and `queries.rs` so the read stack can:

- upsert repos
- upsert profiles
- upsert posts
- soft-delete posts missing from a repo re-sync
- fetch profile by handle or DID
- fetch author feeds with cursor pagination
- fetch posts by URI
- search post text
- store and read service-state cursors

- [ ] **Step 5: Run workspace compile verification**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Re-run the schema contract test**

Run: `cargo test -p divine-atbridge appview_lab_schema_files_define_required_tables -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add migrations/003_appview_read_model crates/divine-bridge-db/src/schema.rs crates/divine-bridge-db/src/models.rs crates/divine-bridge-db/src/queries.rs crates/divine-atbridge/tests/appview_lab_contract.rs
git commit -m "feat: add appview read model schema"
```

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
async fn backfill_syncs_profiles_and_posts_from_pds() {
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
}
```

- [ ] **Step 2: Write the failing relay-refresh test**

```rust
#[tokio::test]
async fn relay_event_triggers_repo_resync() {
    let relay = FakeRelayStream::with_commit_for("did:plc:ebt5msdpfavoklkap6gl54bm");
    let pds = FakePdsClient::with_single_post("did:plc:ebt5msdpfavoklkap6gl54bm", "MA6mjTWZKEB");
    let store = MemoryStore::default();

    run_single_event_loop(&relay, &pds, &store).await.unwrap();

    assert_eq!(store.posts()[0].rkey, "MA6mjTWZKEB");
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

## Chunk 3: AppView Read API And Discovery Feeds

### Task 5: Add The `divine-appview` Read Service

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
async fn get_profile_returns_divine_actor() {
    let app = app_with_store(FakeStore::with_profile("did:plc:ebt5msdpfavoklkap6gl54bm"));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.actor.getProfile?actor=did:plc:ebt5msdpfavoklkap6gl54bm")
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
async fn search_posts_returns_vine_demo_posts() {
    let app = app_with_store(FakeStore::with_posts(vec![
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB",
        "at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU",
    ]));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.searchPosts?q=vine")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
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
- add CORS config for the local React viewer origin

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

### Task 6: Replace Static Feedgen Data With Read-Model Queries

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
  - prefer parsed loop/like counts when present in archived text
  - otherwise fall back to recency

Keep the public feed URIs stable so the viewer can rely on them.

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

## Chunk 4: Tiny React Viewer And End-To-End Verification

### Task 7: Add The Tiny React Video Viewer

**Files:**
- Create: `apps/divine-video-viewer/package.json`
- Create: `apps/divine-video-viewer/vite.config.ts`
- Create: `apps/divine-video-viewer/tsconfig.json`
- Create: `apps/divine-video-viewer/index.html`
- Create: `apps/divine-video-viewer/src/api.ts`
- Create: `apps/divine-video-viewer/src/App.tsx`
- Create: `apps/divine-video-viewer/src/main.tsx`
- Create: `apps/divine-video-viewer/src/styles.css`
- Create: `apps/divine-video-viewer/src/components/FeedGrid.tsx`
- Create: `apps/divine-video-viewer/src/components/SearchBar.tsx`
- Create: `apps/divine-video-viewer/src/components/VideoCard.tsx`

- [ ] **Step 1: Write the failing viewer smoke test**

Create a minimal frontend smoke, either:

- `apps/divine-video-viewer/src/App.test.tsx`, or
- a scripted build-time smoke in `package.json`

The test must assert that the app can render feed cards from mocked ATProto responses.

- [ ] **Step 2: Run the frontend test or build and verify it fails**

Run: `cd apps/divine-video-viewer && npm install && npm run build`
Expected: FAIL because the app does not exist yet.

- [ ] **Step 3: Implement the viewer**

The viewer should provide:

- a latest feed view using `divine-feedgen`
- a trending feed view using `divine-feedgen`
- a search input using `app.bsky.feed.searchPosts`
- post cards that hydrate profile and post data via `divine-appview`
- direct links or detail expansion for the two known Vine demo posts

Keep the app intentionally small. No auth, no mutations, no styling sprawl.

- [ ] **Step 4: Run the frontend build**

Run: `cd apps/divine-video-viewer && npm install && npm run build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/divine-video-viewer
git commit -m "feat: add divine video viewer app"
```

### Task 8: Add The End-To-End Smoke Script And Operator Runbook

**Files:**
- Create: `scripts/appview-lab-smoke.sh`
- Create: `docs/runbooks/appview-lab.md`
- Modify: `deploy/appview-lab/README.md`
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/pds-operations.md`

- [ ] **Step 1: Write the failing smoke script**

Create `scripts/appview-lab-smoke.sh` that verifies:

- the local appview returns the demo profile DID
- `getAuthorFeed` includes `MA6mjTWZKEB` and `hFxlUuKIIqU`
- `searchPosts?q=vine` returns both demo posts
- `divine-feedgen` latest or trending returns Divine-owned URIs

The script should exit non-zero when any expectation fails.

- [ ] **Step 2: Run the smoke script and verify it fails**

Run: `bash scripts/appview-lab-smoke.sh`
Expected: FAIL because the stack is not fully implemented yet.

- [ ] **Step 3: Document operator flow**

Write `docs/runbooks/appview-lab.md` and update the existing runbooks with:

1. required env setup
2. compose startup
3. expected health endpoints
4. smoke command
5. viewer URL
6. troubleshooting for stale index or relay lag

- [ ] **Step 4: Bring the stack up and run the smoke**

Run: `bash scripts/appview-lab-up.sh`
Expected: compose services start successfully

Run: `bash scripts/appview-lab-smoke.sh`
Expected: PASS and explicitly mention:
- `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB`
- `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU`

- [ ] **Step 5: Run final verification**

Run: `cargo check --workspace`
Expected: PASS

Run: `bash scripts/test-workspace.sh`
Expected: PASS

Run: `cd apps/divine-video-viewer && npm run build`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add scripts/appview-lab-smoke.sh docs/runbooks/appview-lab.md deploy/appview-lab/README.md docs/runbooks/dev-bootstrap.md docs/runbooks/pds-operations.md
git commit -m "docs: add divine appview lab runbook"
```

## Execution Notes

- The external relay behavior belongs in `divinevideo/rsky-relay`, not in this repository.
- If the relay fork needs Divine-only flags or host allowlisting, document those flags in `deploy/appview-lab/env.example` and `deploy/appview-lab/README.md`, but keep the implementation in the fork.
- The appview indexer should keep its first version simple and explicit: relay events indicate which DID changed, and the indexer re-reads the relevant PDS records instead of trying to decode the full firehose payload into a generic network-wide dataplane.
- Do not broaden scope into auth, likes, or wider-network indexing before the known demo posts render end to end.
