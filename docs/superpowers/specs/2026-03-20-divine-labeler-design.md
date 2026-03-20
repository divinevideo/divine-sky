# divine-labeler: ATProto Labeler HTTP Server

**Date:** 2026-03-20
**Status:** Approved

## Purpose

Standalone Axum HTTP server that operates as an ATProto labeler service at `labeler.divine.video`. Receives moderation results from the JS moderation service via webhook, signs them as ATProto labels, stores them in `labeler_events`, and serves them to ATProto consumers via the standard `queryLabels` XRPC endpoint.

## Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/xrpc/com.atproto.label.queryLabels` | Serve signed labels from `labeler_events` |
| POST | `/webhook/moderation-result` | Receive webhook from JS moderation service |
| GET | `/health` | Health check |

## Components

- **`main.rs`** — Axum server, env config, DB pool setup
- **`config.rs`** — Env var loading: `LABELER_DID`, `LABELER_SIGNING_KEY`, `DATABASE_URL`, `WEBHOOK_TOKEN`, `PORT`
- **`routes/query_labels.rs`** — Query `labeler_events` via divine-bridge-db, format with existing `labeler_service::format_query_labels_response()`
- **`routes/webhook.rs`** — Bearer token auth, map webhook payload → `OutboundLabel`, sign, insert into `labeler_events`
- **`signing.rs`** — DAG-CBOR encode label fields → secp256k1 sign → base64 sig

## Dependencies

- `divine-bridge-db` — DB queries and models
- `divine-moderation-adapter` — OutboundLabel, vocabulary
- `divine-bridge-types` — AtprotoLabel type
- `axum 0.7` — HTTP framework (matches divine-handle-gateway, divine-feedgen)
- `k256` — secp256k1 signing
- `serde_ipld_dagcbor` — DAG-CBOR encoding for label signing
- `diesel` + `deadpool-diesel` — DB connection pool

## Data Flow

```
JS moderation service
    → POST /webhook/moderation-result (Bearer token auth)
    → OutboundLabel::from_moderation_result()
    → sign each label (CBOR encode → secp256k1 sign)
    → insert_labeler_event() → labeler_events table

Bluesky/clients
    → GET /xrpc/com.atproto.label.queryLabels?uriPatterns=...&cursor=...
    → get_labeler_events_after() → format_query_labels_response()
    → JSON response with signed labels
```

## Configuration (env vars)

```
LABELER_DID=did:plc:...
LABELER_SIGNING_KEY=hex-encoded-secp256k1-private-key
DATABASE_URL=postgres://...
WEBHOOK_TOKEN=shared-secret-for-js-service
PORT=3001
```

## Identity

- New `did:plc` created for the labeler (one-time manual setup)
- Signing key provided via env var, must match DID document
- `app.bsky.labeler.service` record published to labeler's repo (one-time manual setup)

## Deferred

- `subscribeLabels` WebSocket endpoint
- DID/PLC registration tooling
- `app.bsky.labeler.service` record publication tooling
