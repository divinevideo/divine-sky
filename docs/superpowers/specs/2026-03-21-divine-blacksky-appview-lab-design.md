# Divine Blacksky AppView Lab Design

**Date:** 2026-03-21
**Status:** Approved

## Purpose

Build an additive, Divine-only ATProto read stack that lets a tiny React video app browse Divine-hosted content through ATProto-shaped endpoints.

The write path and source of truth remain unchanged:

- Divine content is authored and mirrored into the Divine PDS.
- The existing Divine PDS remains the canonical repo host.
- The new local lab only reads from that PDS and exposes appview-style read surfaces.

This design is specifically for viewing Divine content, not for indexing the wider AT Protocol network.

## Scope And Guardrails

- Keep `config/docker-compose.yml` as the default fast local bridge stack.
- Add a second, additive lab under `deploy/appview-lab/`.
- Do not vendor `rsky` source code into this repository.
- Use the external `divinevideo/rsky-relay` fork as a dependency owned outside this repo.
- Limit the relay and indexing scope to repos hosted on the Divine PDS.
- Backfill every repo on the Divine PDS on first boot so the viewer is useful immediately.
- Use real Divine data already on the PDS as the acceptance anchor for v1.

Known fixture content that must show up in the lab:

- repo DID: `did:plc:ebt5msdpfavoklkap6gl54bm`
- post URI: `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB`
- post URI: `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU`

## Service Model

The lab keeps the Blacksky-shaped boundaries, but only for Divine data:

1. **Divine PDS**
   - existing host
   - source of truth for repos, posts, and profiles

2. **`divinevideo/rsky-relay`**
   - local AT firehose source
   - scoped to Divine PDS content only
   - owned by the external fork, not by this repo

3. **`divine-appview-indexer`**
   - new crate in this repo
   - backfills every Divine PDS repo
   - tails the relay for repo changes
   - refreshes the local read model

4. **`divine-appview`**
   - new Axum service in this repo
   - exposes the read endpoints the viewer needs
   - reads only from the local PostgreSQL read model

5. **`divine-feedgen`**
   - existing crate in this repo
   - upgraded to query the local read model instead of returning static feed skeletons
   - provides discovery feeds such as `latest` and `trending`

6. **`apps/divine-video-viewer`**
   - new tiny React app
   - consumes the local appview and feedgen endpoints
   - proves that Divine videos can be viewed through ATProto-style surfaces

## Data Flow

### Bootstrap And Backfill

On first boot:

1. The indexer calls `com.atproto.sync.listRepos` against the Divine PDS.
2. For each repo DID, the indexer fetches the relevant collections from the PDS.
3. The indexer writes normalized rows into the local read-model tables.
4. The viewer becomes immediately useful because known demo posts are already present.

The indexer should favor simple, explicit PDS reads over building a full CAR-decoding appview dataplane in v1. This lab is for Divine content only, so clarity matters more than network-wide efficiency.

### Live Updates

After bootstrap:

1. The indexer subscribes to the local `divinevideo/rsky-relay` firehose.
2. Relay events identify which repo changed.
3. The indexer re-syncs only the affected DID from the Divine PDS.
4. Updated posts and profiles are upserted into the read model.

This keeps the relay in the architecture, but avoids forcing the first lab to implement full repo-block parsing just to stay current.

### Search And Discovery

- `divine-appview` serves profile lookup, author feeds, post lookup, and text search.
- `divine-feedgen` serves discovery feed skeletons backed by the same indexed data.
- The React viewer hydrates discovery feed URIs through `divine-appview`.

## Read-Model Schema

The local PostgreSQL read model should be intentionally small and Divine-specific.

### `appview_repos`

Tracks which Divine PDS repos are known and how fresh they are.

Suggested fields:

- `did`
- `handle`
- `head`
- `rev`
- `active`
- `last_backfilled_at`
- `last_seen_seq`
- `created_at`
- `updated_at`

### `appview_profiles`

Stores the latest actor profile data needed by the viewer.

Suggested fields:

- `did`
- `handle`
- `display_name`
- `description`
- `avatar_cid`
- `banner_cid`
- `raw_json`
- `indexed_at`
- `updated_at`

### `appview_posts`

Stores normalized post data for feed, search, and post detail views.

Suggested fields:

- `uri`
- `did`
- `rkey`
- `cid`
- `created_at`
- `text`
- `embed_json`
- `raw_json`
- `search_text`
- `indexed_at`
- `deleted_at`

The table should include indexes for:

- `did + created_at desc`
- `created_at desc`
- text search on `search_text`

### `appview_service_state`

Stores process-level cursors and freshness metadata.

Suggested fields:

- `state_key`
- `state_value`
- `updated_at`

Examples:

- relay cursor
- last successful full backfill timestamp
- last indexed event timestamp

## API Surface

`divine-appview` should expose only the endpoints the viewer needs first.

### Required Endpoints

- `GET /health`
- `GET /health/ready`
- `GET /xrpc/app.bsky.actor.getProfile?actor=<did-or-handle>`
- `GET /xrpc/app.bsky.feed.getAuthorFeed?actor=<did-or-handle>&limit=<n>&cursor=<cursor>`
- `GET /xrpc/app.bsky.feed.getPosts?uris=<at-uri>&uris=<at-uri>`
- `GET /xrpc/app.bsky.feed.getPostThread?uri=<at-uri>`
- `GET /xrpc/app.bsky.feed.searchPosts?q=<query>&limit=<n>&cursor=<cursor>`

### Discovery Surface

Discovery should flow through the existing `divine-feedgen` service, not through a second custom discovery API:

- `GET /xrpc/app.bsky.feed.describeFeedGenerator`
- `GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/latest`
- `GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/trending`

`latest` can be pure recency.

`trending` can use a simple deterministic heuristic in v1:

- prefer parsed demo stats if present in migrated archive text
- otherwise fall back to newest indexed posts

That is good enough for a lab whose purpose is proving the read path.

## Runtime Contract

The additive lab under `deploy/appview-lab/` should run these local services:

- `postgres`
- `relay`
- `indexer`
- `appview`
- `feedgen`
- `viewer`

The lab must accept an external PDS URL through env instead of assuming the PDS is inside the same compose file.

Key env values:

- `DIVINE_PDS_URL`
- `DIVINE_PDS_HOST`
- `APPVIEW_DATABASE_URL`
- `RSKY_RELAY_IMAGE`
- `APPVIEW_CORS_ORIGIN`
- `VIEWER_APPVIEW_URL`
- `VIEWER_FEEDGEN_URL`

The relay image should default to the external fork location, for example:

- `ghcr.io/divinevideo/rsky-relay:latest`

## Repository Layout

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
      store.rs
      views.rs
      routes/
        actor.rs
        feed.rs
        health.rs
        search.rs
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

## Verification

The lab is successful when all of the following are true:

- the additive compose stack validates with `docker compose config`
- the known Cameron Dallas and LiveLikeDavis post URIs are returned by the local read API
- actor profile lookup works for the demo DID
- discovery feeds return Divine-hosted URIs
- search returns Divine content from indexed rows
- a new post mirrored into the Divine PDS appears after relay-driven re-sync
- the React app builds and renders the indexed posts

## Deferred

These are intentionally out of scope for the first lab:

- wider-network indexing
- full `rsky-wintermute` dataplane parity
- auth and logged-in timelines
- likes, reposts, replies, and notifications
- labeler ingestion
- production deployment design for the appview stack
- vendoring or forking more `rsky` components into this repo
