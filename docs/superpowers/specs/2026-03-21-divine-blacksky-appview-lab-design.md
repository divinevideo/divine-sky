# Divine Blacksky AppView Lab Design

**Date:** 2026-03-21
**Status:** Approved

## Purpose

Build an additive, Divine-only ATProto read stack that lets a tiny JS viewer browse Divine-hosted content through ATProto-shaped endpoints while tightening the Divine write path to better match official ATProto and Bluesky record, media, and appview practices.

This design keeps the existing Divine PDS as the source of truth. The new lab only reads from that PDS and exposes appview-style read surfaces for Divine content.

This is specifically for viewing Divine content, not for indexing the wider AT Protocol network.

## Why This Revision Exists

The earlier draft was directionally right about adding an indexer, an appview, a feed generator, and a small viewer, but it blurred a few boundaries that matter if we want the stack to behave more like real ATProto clients and servers expect:

- repo records should store blob refs, not playback URLs
- feed generators should return post AT-URIs, not hydrated media data
- appviews should hydrate post/profile views and return video view objects
- browser playback should use a media/view layer, not direct PDS blob fetches
- new `app.bsky.feed.post` writes should use TID-backed record keys and explicit validation

The lab should reflect those boundaries clearly.

## Scope And Guardrails

- Keep `config/docker-compose.yml` as the default fast local bridge stack.
- Add a second, additive lab under `deploy/appview-lab/`.
- Do not vendor `rsky` source code into this repository.
- Use the external `divinevideo/rsky-relay` fork as a dependency owned outside this repo.
- Limit relay and indexing scope to repos hosted on the Divine PDS.
- Backfill every repo on the Divine PDS on first boot so the viewer is useful immediately.
- Keep the lab read-only from the viewer's perspective: no login, no mutations, no network-wide crawl.
- Add a tiny lab-only JS viewer under `apps/divine-blacksky-viewer/`; this is not a replacement for `divine-web`.
- Hardening the Divine write path is in scope because the lab depends on lexicon-valid posts, profile records, and media metadata.
- Historical Divine posts already on the PDS are indexed as they exist today. The compliance hardening in this design applies prospectively to new writes.

Known fixture content that must show up in the lab:

- repo DID: `did:plc:ebt5msdpfavoklkap6gl54bm`
- post URI: `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/MA6mjTWZKEB`
- post URI: `at://did:plc:ebt5msdpfavoklkap6gl54bm/app.bsky.feed.post/hFxlUuKIIqU`

## ATProto Compliance Rules

These rules are the design contract for the revised lab:

- New `app.bsky.feed.post` records use TID-compatible record keys. Do not derive post rkeys from Nostr `d` tags or raw event IDs.
- New post writes use `com.atproto.repo.createRecord` with validation enabled. The bridge persists the returned AT-URI and rkey for later delete handling.
- The bridge record stores only a blob ref in `app.bsky.embed.video`. Playback URLs never live in the repo record.
- `app.bsky.embed.video` input blobs must be MP4 and within official size limits before publish. Non-MP4 inputs are transcoded or rejected before the post record is written.
- `app.bsky.actor.profile` uses official fields like `website` and `createdAt` instead of stuffing extra data into `description`.
- Avatar and banner uploads respect the official image MIME and size constraints before they are embedded in profile records.
- `langs` is omitted when unknown. Do not hardcode `en`.
- `divine-feedgen` returns post AT-URIs only.
- `divine-appview` hydrates those URIs into profile views, post views, and `app.bsky.embed.video#view` objects.
- Browser playback goes through a Divine media/view layer that serves playlist and thumbnail URLs. The viewer does not fetch `com.atproto.sync.getBlob` directly.

## Service Model

The revised lab keeps the Blacksky-shaped boundaries, but with clearer writer, reader, and media responsibilities:

1. **Divine PDS**
   - existing host
   - source of truth for repos, posts, profiles, and blobs

2. **`divine-atbridge`**
   - existing crate in this repo
   - hardens prospective Nostr-to-ATProto publishing
   - creates validated `app.bsky.feed.post` records and persists the returned AT-URI mapping

3. **`divine-video-worker`**
   - existing crate in this repo
   - normalizes source video into publishable MP4 when needed
   - builds view-facing derivatives such as playlist and thumbnail assets
   - records media-view metadata keyed by Divine DID and blob CID

4. **`divinevideo/rsky-relay`**
   - local AT firehose source
   - scoped to Divine PDS content only
   - owned by the external fork, not by this repo

5. **`divine-appview-indexer`**
   - new crate in this repo
   - backfills every Divine PDS repo
   - tails the relay for repo changes
   - refreshes the local read model with profile, post, and blob-view state

6. **`divine-appview`**
   - new Axum service in this repo
   - exposes the read endpoints the viewer needs
   - returns ATProto-shaped profile and post views backed by the local read model

7. **`divine-feedgen`**
   - existing crate in this repo
   - upgraded to query the local read model instead of returning static feed skeletons
   - returns discovery feed skeletons such as `latest` and `trending`

8. **`apps/divine-blacksky-viewer`**
   - new tiny JS app
   - consumes the local appview and feedgen endpoints
   - proves that Divine videos can be viewed through ATProto-style surfaces without dragging in full product scope

## Write-Side Data Flow

### New Post Publish Path

For new Nostr video events:

1. `divine-atbridge` verifies the Nostr event and resolves the linked Divine DID.
2. `divine-video-worker` fetches the source asset, verifies the source hash, and normalizes the output to a publishable MP4 if required.
3. The normalized MP4 is uploaded to the Divine PDS, yielding a blob ref.
4. `divine-atbridge` creates an `app.bsky.feed.post` record with `app.bsky.embed.video`, validation enabled, and a TID-backed record key.
5. The bridge persists the returned AT-URI, record CID, and rkey in its existing record-mapping store for deletes and lineage.
6. The asset manifest keeps the source hash, the stored blob CID, and enough metadata for later appview/media derivation.

This is forward-looking hardening. Historical posts already on the Divine PDS are read as-is.

### Profile Publish Path

For Nostr kind-0 profile events:

1. `divine-atbridge` parses profile JSON into official ATProto profile fields.
2. `divine-video-worker` or an equivalent helper normalizes avatar and banner inputs to allowed image formats and size limits.
3. `divine-atbridge` writes `app.bsky.actor.profile` using official fields such as `displayName`, `description`, `website`, `avatar`, `banner`, and `createdAt`.

## Read-Side Data Flow

### Bootstrap And Backfill

On first boot:

1. The indexer calls `com.atproto.sync.listRepos` against the Divine PDS.
2. For each repo DID, the indexer fetches the relevant collections from the PDS.
3. The indexer writes normalized rows into the local read-model tables.
4. The media worker backfills playlist and thumbnail metadata for any indexed video blobs that do not already have ready view assets.
5. The viewer becomes immediately useful because the known demo posts are already present.

The indexer should favor simple, explicit PDS reads over building a full CAR-decoding dataplane in v1. This lab is for Divine content only, so clarity matters more than network-wide efficiency.

### Live Updates

After bootstrap:

1. The indexer subscribes to the local `divinevideo/rsky-relay` firehose.
2. Relay events identify which repo changed.
3. The indexer re-syncs only the affected DID from the Divine PDS.
4. Changed posts and profiles are upserted into the read model.
5. New or changed video blobs are queued for playlist and thumbnail derivation.
6. `divine-appview` begins serving updated view data once the read model and media-view rows are fresh.

This keeps the relay in the architecture while avoiding full repo-block parsing in the first lab.

### Search, Discovery, And Playback

- `divine-feedgen` serves feed skeletons backed by indexed data. It returns only post AT-URIs.
- `divine-appview` hydrates those URIs into full post views, profile views, author feeds, post detail views, and search results.
- For video posts, `divine-appview` returns `app.bsky.embed.video#view` with `cid`, `playlist`, and optional `thumbnail`.
- The JS viewer calls the playlist URL in the returned embed view to play video. It never constructs a blob URL itself.

## Read-Model Schema

The local PostgreSQL read model should stay intentionally small and Divine-specific, but it needs one additional concept the earlier draft skipped: view-facing media derivatives.

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
- `website`
- `avatar_cid`
- `banner_cid`
- `created_at`
- `raw_json`
- `indexed_at`
- `updated_at`

### `appview_posts`

Stores normalized post data for feed, search, author pages, and post detail views.

Suggested fields:

- `uri`
- `did`
- `rkey`
- `record_cid`
- `created_at`
- `text`
- `langs_json`
- `embed_blob_cid`
- `embed_alt`
- `aspect_ratio_width`
- `aspect_ratio_height`
- `raw_json`
- `search_text`
- `indexed_at`
- `deleted_at`

Indexes should include:

- `did + created_at desc`
- `created_at desc`
- text search on `search_text`
- lookup by `embed_blob_cid`

### `appview_media_views`

Stores the browser-facing media view state keyed by the indexed blob.

Suggested fields:

- `did`
- `blob_cid`
- `playlist_url`
- `thumbnail_url`
- `mime_type`
- `bytes`
- `ready`
- `last_derived_at`
- `updated_at`

This table exists specifically so the appview can return video view objects without pretending that the PDS record itself contains playback URLs.

### `appview_service_state`

Stores process-level cursors and freshness metadata.

Suggested fields:

- `state_key`
- `state_value`
- `updated_at`

Examples:

- relay cursor
- last successful full backfill timestamp
- last indexed repo refresh timestamp
- last successful media-derivation sweep timestamp

## API Surface

`divine-appview` should expose only the endpoints the viewer needs first, but the response shapes should track official ATProto appview conventions as closely as practical.

### Required Endpoints

- `GET /health`
- `GET /health/ready`
- `GET /xrpc/app.bsky.actor.getProfile?actor=<did-or-handle>`
- `GET /xrpc/app.bsky.feed.getAuthorFeed?actor=<did-or-handle>&limit=<n>&cursor=<cursor>`
- `GET /xrpc/app.bsky.feed.getPosts?uris=<at-uri>&uris=<at-uri>`
- `GET /xrpc/app.bsky.feed.getPostThread?uri=<at-uri>`
- `GET /xrpc/app.bsky.feed.searchPosts?q=<query>&limit=<n>&cursor=<cursor>`

### Response Contract

- Profile responses should use official profile fields such as `displayName`, `description`, `avatar`, `banner`, and `viewer`-safe handles.
- Post responses should return AT-URIs, record CIDs, author views, indexed timestamps, and embed views instead of bespoke Divine-only JSON.
- Video post embeds should be returned as `app.bsky.embed.video#view`.
- `getPostThread` can be shallow in v1. The viewer only needs a single-post detail view, not full reply trees.

### Discovery Surface

Discovery should flow through the existing `divine-feedgen` service, not through a second custom discovery API:

- `GET /xrpc/app.bsky.feed.describeFeedGenerator`
- `GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/latest`
- `GET /xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:plc:divine.feed/app.bsky.feed.generator/trending`

`latest` can be pure recency.

`trending` can stay intentionally simple in v1:

- rank by any indexed engagement fields that actually exist
- fall back to recency when they do not

If Bluesky-client discovery outside the lab becomes important later, we can add a proper `app.bsky.feed.generator` record publication step and video content-mode metadata. That is not required for the first local viewer proof.

## Runtime Contract

The additive lab under `deploy/appview-lab/` should run these local services:

- `postgres`
- `relay`
- `indexer`
- `media-worker`
- `appview`
- `feedgen`
- `viewer`

The write path remains owned by the existing Divine PDS and bridge services. The appview-lab compose file does not need to duplicate those services unless a smoke scenario explicitly exercises end-to-end publishing.

The lab must accept an external PDS URL through env instead of assuming the PDS is inside the same compose file.

Key env values:

- `DIVINE_PDS_URL`
- `DIVINE_PDS_HOST`
- `APPVIEW_DATABASE_URL`
- `RSKY_RELAY_IMAGE`
- `MEDIA_VIEW_BASE_URL`
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
  divine-atbridge/
    src/
      pipeline.rs
      profile_sync.rs
      publisher.rs
      translator.rs
    tests/
      post_record_contract.rs
      publish_path_integration.rs
      profile_record_contract.rs
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

## Verification

The lab is successful when all of the following are true:

- new bridge writes use validation-enabled post creation and do not derive custom post rkeys from Nostr metadata
- non-MP4 sources are normalized or rejected before publish
- profile records use official website and image fields instead of stuffing URLs into description
- the additive compose stack validates with `docker compose config`
- the known Cameron Dallas and LiveLikeDavis post URIs are returned by the local read API
- actor profile lookup works for the demo DID
- discovery feeds return Divine-hosted URIs
- search returns Divine content from indexed rows
- `divine-appview` returns video embed views with `playlist` and optional `thumbnail`
- a new post mirrored into the Divine PDS appears after relay-driven re-sync
- the JS viewer loads the global feed, trending feed, author pages, post detail pages, and plays indexed videos

## Deferred

These are intentionally out of scope for the first lab:

- wider-network indexing
- full `rsky-wintermute` dataplane parity
- auth and logged-in timelines
- likes, reposts, replies, and notifications
- labeler ingestion
- production deployment design for the appview stack
- rewriting historical non-TID Divine posts already on the PDS
- publishing a globally discoverable feed-generator record for third-party clients
