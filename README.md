# Divine Sky

`divine-sky` is the AT Protocol workspace for [Divine](https://divine.video), a Nostr-native platform for short looping video. It is where Divine tests whether Nostr-authored content, identity, moderation signals, and social reach can move into the ATProto / Bluesky ecosystem without collapsing into a closed platform in the middle.

This is an experiment. Some parts are production-shaped because interoperability has to be tested against real systems, but the repository exists to learn and revise, not to be a finished product surface. Divine remains Nostr-first for authorship and source-of-truth; `divine-sky` is the bridge, not a replacement.

## How it works

Divine users publish signed [NIP-71](https://github.com/nostr-protocol/nips/blob/master/71.md) video events to Nostr. `divine-sky` consumes that event stream, translates each video into an `app.bsky.embed.video` record, and writes it into a per-user ATProto repository on a Divine-operated PDS. Authorization derives from the original Nostr event signatures — users sign once on Nostr, and Divine republishes to ATProto on their behalf.

The bridge is a *consumer* of the Nostr stream, not part of the critical posting path. If it goes down, Nostr-side publishing is unaffected and the bridge catches up by replaying missed events. There is no third-party bridge dependency; Divine controls the whole pipeline. The underlying PDS, relay, and feed-generator building blocks come from [rsky](https://github.com/blacksky-algorithms/rsky), Blacksky's Rust AT Protocol implementation.

## Components

The repository is a Rust workspace (`crates/`) plus one TypeScript viewer app (`apps/`).

| Crate | Role |
|-------|------|
| `divine-atbridge` | Core bridge runtime: consumes NIP-71 events (live ingest via the Funnelcake REST API), translates them to ATProto video records, provisions accounts, and publishes to per-account PDS repos. Handles backfill, replay, deletions, and profile sync. Self-migrates its database on startup. |
| `divine-handle-gateway` | Handle-facing HTTP service for the opt-in, export, disable, and status flows. Coordinates provisioning with the keycast and name-server sibling services. |
| `divine-feedgen` | ATProto feed-generation service (`app.bsky.feed.getFeedSkeleton`). |
| `divine-labeler` | Labeler service and signing, plus a `create-labeler-did` helper binary. |
| `divine-moderation-adapter` | Translation of moderation and label signals across the Nostr/ATProto boundary (early stage). |
| `divine-video-worker` | Media-path processing: normalization, derivative generation, CID computation, Blossom and blob uploads, profile images. |
| `divine-appview` | Axum AppView HTTP service exposing read-model views. |
| `divine-appview-indexer` | Indexes PDS and relay data into the AppView read model. |
| `divine-localnet-admin` | Local DNS/zone admin API used by the localnet lab. |
| `divine-bridge-db` | Diesel-backed database layer (models, queries, and migrations for the bridge tables). |
| `divine-bridge-types` | Shared Nostr and ATProto type definitions used across the crates. |

`apps/divine-blacksky-viewer` is a small React + Vite viewer (using `hls.js` for playback) for browsing bridged feeds, authors, and posts against the AppView.

## Architecture

- **Identity** — Divine users opt in to an ATProto handle under a Divine-controlled domain; DID and key custody are documented in `docs/runbooks/atproto-identity-key-custody.md`. A username claim alone only enables NIP-05; the ATProto path is explicitly opt-in.
- **Ingest** — `divine-atbridge` reads live NIP-71 events through the Funnelcake REST API (which replaced the earlier WebSocket firehose consumer) and replays history oldest-first for backlog fills.
- **Translation** — NIP-71 video events become `app.bsky.embed.video` posts, carrying language, verification, and caption metadata where available.
- **Publish** — records are written to each user's PDS repo using per-account publish auth.
- **Read side** — `divine-appview` and `divine-appview-indexer` build a read model; `divine-feedgen` and `divine-labeler` provide feeds and labels; the viewer app renders them.
- **Storage** — PostgreSQL for the bridge and read-model tables (migrations in `migrations/`), plus S3-compatible object storage (MinIO locally) and Blossom for media blobs.

## Getting started

Prerequisites: a Rust toolchain, Docker with Compose, and the PostgreSQL client libraries.

```bash
# macOS
brew install libpq
# Debian/Ubuntu
sudo apt-get install -y libpq-dev pkg-config
```

Fast compile and full verification:

```bash
cargo check --workspace          # fast compile pass
bash scripts/test-workspace.sh   # workspace checks + tests (configures libpq for Diesel)
```

Start the local stack (PostgreSQL, MinIO, a mock Blossom server, a mock Nostr relay, a local PDS, and the bridge):

```bash
docker compose -f config/docker-compose.yml up -d
```

For a fuller end-to-end ATProto lab with dedicated PLC, PDS, Jetstream, DNS, and handle administration, use `deploy/localnet/` instead. It is additive to the fast stack and keeps local handles on `*.divine.test` rather than the production domain. See `docs/runbooks/dev-bootstrap.md` and `docs/runbooks/localnet-lab.md`.

## Configuration

The bridge runtime is configured from the environment. The local stack sets these for you; the core variables are:

```bash
export PDS_AUTH_TOKEN=local-dev-token
export PLC_DIRECTORY_URL=http://127.0.0.1:2583
export HANDLE_DOMAIN=divine.video
export RELAY_SOURCE_NAME=local-stack-relay
```

The local Compose stack does not ship a PLC mock, so exercising the full opt-in provisioning flow requires pointing `PLC_DIRECTORY_URL` at a real or test PLC endpoint — which is what `deploy/localnet/` provides. Per-service `env.example` files live alongside each `deploy/` and `deploy/localnet/` slice.

## Deployment

Each long-running service ships as its own container image (`Dockerfile.atbridge`, `Dockerfile.feedgen`, `Dockerfile.handle-gateway`, `Dockerfile.labeler`). CI (`.github/workflows/rust.yml`) runs the full workspace verification against a PostgreSQL 16 service on every push and pull request. Staging and production promotion procedures are documented in the runbooks, including:

- `docs/runbooks/staging-production-deploy.md`
- `docs/runbooks/2026-06-05-atproto-prod-promotion.md`
- `docs/runbooks/launch-checklist.md`

## Documentation

- **Canonical plan** — `docs/plans/2026-03-20-divine-atproto-unified-plan.md` is the source of truth for ATProto direction. `docs/runbooks/source-of-truth.md` explains which documents are normative versus supporting.
- **Technical spec** — `divine-sky-technical-spec.md` gives the deep architecture background (identity, PDS, video mapping, feeds, moderation). It is retained as historical synthesis input; the unified plan supersedes it where they differ.
- **Runbooks** — operational setup, smoke tests, and deploy procedures live in `docs/runbooks/`.

---

Part of [Divine](https://divine.video) — your playground for human creativity · [Brand guidelines](https://github.com/divinevideo/brand-guidelines)
