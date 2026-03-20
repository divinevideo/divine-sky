# divine-labeler: Go-Live Remaining Work

**Date:** 2026-03-20
**Status:** In progress

## What's Done

- Bidirectional label vocabulary mapping (Rust + JS)
- ATProto label types (divine-bridge-types)
- DB migration: labeler_events + inbound_labels tables
- Outbound/inbound label pipelines
- JS webhook + pipeline wiring in divine-moderation-service
- ClickHouse writer + Nostr publisher for inbound labels
- Admin review stub endpoints
- Integration tests (21 Rust + 10 JS)
- divine-labeler HTTP service: config, DAG-CBOR signing, queryLabels, webhook

## Remaining Work (Critical Path)

### 1. AT URI Mapping (Engineering — divine-sky)

**Problem:** The webhook currently uses `at://sha256:{hash}` as a placeholder URI. Real ATProto labels must reference actual AT URIs from `record_mappings` (e.g., `at://did:plc:user/app.bsky.feed.post/rkey`).

**What to do:**
- In the webhook handler (`crates/divine-labeler/src/routes/webhook.rs`), look up the sha256 in `record_mappings` to get the real AT URI
- Add a `get_record_by_sha256()` query to divine-bridge-db (join asset_manifest on sha256 → get nostr_event_id → join record_mappings → get at_uri)
- Fall back to `at://sha256:{hash}` if no mapping exists (content not yet mirrored)
- Also look up the subject's DID from the mapping for proper label targeting

**Depends on:** divine-atbridge actually mirroring content to ATProto (creating record_mappings entries).

### 2. Deployment (Engineering — divine-iac-coreconfig)

**What to do:**
- Dockerfile for divine-labeler (multi-stage Rust build)
- K8s Deployment manifest in divine-iac-coreconfig
- ExternalSecret for: DATABASE_URL, LABELER_SIGNING_KEY, WEBHOOK_TOKEN
- ConfigMap for: LABELER_DID, PORT
- HTTPRoute for `labels.divine.video` (production) / `labels.dvines.org` (staging)
- Service + health check on `/health`
- 2 replicas, resource requests/limits

**Env contract:**
```
LABELER_DID=did:plc:...          # plain env
LABELER_SIGNING_KEY=<hex>        # secret
DATABASE_URL=postgres://...      # secret (shared bridge DB)
WEBHOOK_TOKEN=<shared-secret>    # secret
PORT=3001                        # plain env
```

### 3. DID Creation (Manual Ops — one-time)

**What to do:**
1. Generate a new secp256k1 keypair for the labeler
2. Create a `did:plc` document with:
   - `verificationMethods`: the labeler's public key (secp256k1, `did:key` multicodec format)
   - `services`: `atproto_labeler` service endpoint pointing to `https://labels.divine.video`
3. Publish the PLC operation to `plc.directory`
4. Store the private key as `LABELER_SIGNING_KEY` secret in divine-iac-coreconfig

**Tools:** Can use `rsky` CLI or write a one-off script using divine-atbridge's existing secp256k1 + PLC provisioning code.

### 4. Labeler Declaration (Manual Ops — one-time)

**What to do:**
1. Create an ATProto account for the labeler DID on the divine PDS (or Bluesky's PDS)
2. Publish an `app.bsky.labeler.service` record to the labeler's repo:
   ```json
   {
     "$type": "app.bsky.labeler.service",
     "policies": {
       "labelValues": ["porn", "sexual", "nudity", "graphic-media", "violence", "self-harm", "spam", "!takedown", "!warn"],
       "labelValueDefinitions": []
     },
     "createdAt": "2026-03-20T00:00:00Z"
   }
   ```
3. This makes the labeler discoverable by Bluesky clients

**Note:** Bluesky may have a registration process for third-party labelers. Check current docs at https://docs.bsky.app/docs/advanced-guides/moderation

## Deferred (Not Blocking Go-Live)

### 5. Admin Review UI Wiring
Stub endpoints in divine-moderation-service need to query the bridge DB. Requires divine-labeler to expose admin API endpoints for listing/approving inbound labels.

### 6. subscribeLabels WebSocket
Real-time label streaming for ATProto consumers. queryLabels polling works initially. Add when there's consumer demand.

### 7. Inbound Label Consumption
Subscribe to Bluesky Ozone's labels. All processing code exists, just needs the WebSocket client or queryLabels poller pointed at Ozone. Deferred until DiVine wants to consume external moderation signals.

## Domains

| Environment | Feed | Labeler |
|---|---|---|
| Production | feed.divine.video | labels.divine.video |
| Staging | feed.dvines.org | labels.dvines.org |
