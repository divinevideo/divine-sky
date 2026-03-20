# DiVine ATProto PDS Integration — Technical Specification & Product Plan

> Status: historical synthesis input. The canonical source of truth is `docs/plans/2026-03-20-divine-atproto-unified-plan.md`.

**Version:** 1.0 Draft
**Date:** 2026-03-20
**Author:** DiVine Engineering / Verse Communications

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Identity Architecture](#2-identity-architecture)
3. [PDS Architecture & Implementation](#3-pds-architecture--implementation)
4. [Video Format & Protocol Mapping](#4-video-format--protocol-mapping)
5. [Feed & Discovery Integration](#5-feed--discovery-integration)
6. [Moderation & Trust and Safety](#6-moderation--trust-and-safety)
7. [Product Roadmap & Phasing](#7-product-roadmap--phasing)
8. [Competitive & Strategic Analysis](#8-competitive--strategic-analysis)
9. [Open Questions & Risks](#9-open-questions--risks)
10. [Architecture Diagram](#10-architecture-diagram)
11. [Sources](#11-sources)

---

## 1. Executive Summary

DiVine (divine.video) is a Nostr-native 6-second looping video platform. This specification details how to add native AT Protocol support so that DiVine videos simultaneously appear on Bluesky, Skylight, Flashes, and the broader ATProto ecosystem — without requiring third-party bridges.

### Core Architecture Decision

A DiVine-operated PDS (Personal Data Server) at `pds.divine.video` subscribes to the funnelcake Nostr relay, reads cryptographically signed NIP-71 video events, translates them into `app.bsky.embed.video` records, and writes them to user ATProto repositories.

**Key properties:**
- The PDS is a **consumer** of the Nostr event stream, not in the critical posting path
- Authorization derives from **Nostr event signatures** — users sign once, DiVine publishes to ATProto on their behalf
- If the PDS goes down, Nostr-side publishing is unaffected; the PDS catches up by replaying missed events
- No third-party bridge dependency — DiVine controls the entire pipeline

### Implementation Foundation: rsky

The PDS will be built on [rsky](https://github.com/blacksky-algorithms/rsky), Blacksky's Rust AT Protocol implementation. rsky provides:

| Crate | Purpose |
|-------|---------|
| `rsky-pds` | Personal Data Server (PostgreSQL + S3) |
| `rsky-relay` | Network crawler / firehose aggregator |
| `rsky-feedgen` | Custom algorithmic feed generator |
| `rsky-labeler` | Content labeling service |
| `rsky-repo` | Data storage / MST implementation |
| `rsky-crypto` | Cryptographic signing and key serialization |
| `rsky-identity` | DID and handle resolution |
| `rsky-syntax` | String parsers for identifiers |
| `rsky-firehose` | Firehose protocol consumer |
| `rsky-wintermute` | Indexing service for AppView functionality |
| `rsky-satnav` | Repository explorer and verification |

rsky is Apache 2.0 licensed, 630 stars, actively maintained (commits as of March 2026). Verse/andOtherStuff has an existing relationship with Rudy Fraser (Blacksky's creator).

**rsky-pds technical stack**: Rocket 0.5.1 web framework, Diesel 2.2.0 ORM with PostgreSQL, AWS SDK S3 v1.29.0 (configurable endpoint for R2/MinIO), JWT auth via `jwt-simple`, 16 PostgreSQL tables, 60+ XRPC endpoints including full `com.atproto.sync.*` suite.

**rsky-video**: Dedicated video upload service with Bunny Stream transcoding, HLS playlist generation, thumbnail proxying, and per-user daily quotas. Uses Axum framework.

---

## 2. Identity Architecture

### 2.1 DID Strategy: Recommendation — `did:web` for MVP, `did:plc` for Production

ATProto supports two DID methods:

**`did:plc`** — Self-authenticating method created for ATProto
- Operated via plc.directory (currently run by Bluesky, transitioning to an independent Swiss association)
- Supports account migration between PDS instances
- Requires registering each DID with the PLC directory
- Most Bluesky accounts use did:plc

**`did:web`** — HTTPS/DNS-based method
- DiVine controls resolution entirely via `did:web:divine.video:users:<username>`
- No external dependency on PLC directory
- **Cannot migrate** — if DiVine stops serving, the DID is dead
- Simpler to implement initially
- Only hostname-level did:web is supported (no paths in standard ATProto)

**Recommendation:** Start with **`did:plc`** despite the PLC directory dependency. Reasons:

1. **Account portability** — Users can theoretically migrate their ATProto identity away from DiVine's PDS in the future, aligning with decentralization values
2. **Ecosystem compatibility** — did:plc is the dominant method; all ATProto tooling is optimized for it
3. **PLC independence coming** — ATProto is establishing the PLC directory as an independent Swiss association with WebSocket mirroring and transparency logs
4. **did:web limitations** — No migration path, and the `did:web` spec only supports hostname-level DIDs (e.g., `did:web:alice.divine.video`), which would require a subdomain per user

### 2.2 Handle Resolution

DiVine users get handles in the format `username.divine.video`.

**DNS Configuration Required:**

```
; Wildcard CNAME or A record pointing all subdomains to DiVine's PDS
*.divine.video.  IN  A  <pds-ip-address>

; OR use .well-known resolution
; Serve /.well-known/atproto-did on divine.video returning the user's DID
```

**Two resolution methods (ATProto supports both):**

1. **DNS TXT record** (preferred for subdomains):
   ```
   _atproto.alice.divine.video. IN TXT "did=did:plc:ewvi7nxzyoun6zhxrhs64oiz"
   ```
   For wildcard subdomains, DiVine would need a DNS API to programmatically create TXT records per user, or use a DNS server that supports dynamic responses.

2. **.well-known HTTP resolution**:
   ```
   GET https://alice.divine.video/.well-known/atproto-did
   → did:plc:ewvi7nxzyoun6zhxrhs64oiz
   ```
   Simpler — DiVine's PDS serves this endpoint directly. **This is the recommended approach** since DiVine controls the web server.

### 2.3 Key Management

**ATProto signing keys:**
- Uses `Multikey` format in DID documents
- Supports P-256 (NIST) and K-256 (secp256k1) curves
- The signing key in the DID document (`#atproto`) is used for repo commit signatures

**Nostr keys:**
- secp256k1 (K-256) keypairs — same curve ATProto supports!

**Design: Custodial ATProto Signing Keys**

DiVine holds custodial ATProto signing keys on behalf of users:

```
┌─────────────────────────────────────────────────────┐
│ Key Management Service                               │
├─────────────────────────────────────────────────────┤
│ For each DiVine user:                                │
│   nostr_pubkey  → secp256k1 public key (user holds) │
│   atproto_did   → did:plc:xxxxx                      │
│   atproto_key   → secp256k1 keypair (DiVine holds)   │
│   created_at    → timestamp                           │
│   rotation_keys → [recovery key held by DiVine]       │
└─────────────────────────────────────────────────────┘
```

- **Generation**: When a user opts in to ATProto, DiVine generates a new secp256k1 keypair for ATProto signing
- **Storage**: Private keys stored encrypted in PostgreSQL or a dedicated secrets manager (e.g., HashiCorp Vault, AWS KMS)
- **Rotation**: did:plc supports key rotation via the PLC directory. DiVine can rotate keys without user involvement
- **Recovery**: DiVine holds rotation keys as PDS operator. If a signing key is compromised, DiVine can rotate it via PLC
- **User-held recovery** (optional): For users who want genuine self-custody, DiVine can display a rotation key seed phrase during opt-in, registered as the highest-priority rotation key. Users who save this can recover their ATProto identity even if DiVine disappears — analogous to saving your Nostr nsec

**Key Derivation Option** (advanced, for Nostr-native UX):

Since both Nostr and ATProto support secp256k1, the ATProto signing key can be deterministically derived from the Nostr key:

```rust
// Deterministic ATProto signing key from Nostr key
atproto_signing_key = HKDF(
    IKM  = nostr_privkey_bytes,
    salt = "divine.video/atproto/signing/v1",
    info = user_pubkey_hex,
    length = 32
)
```

This means users who export their Nostr key can always reconstruct their ATProto signing key. A compromised Nostr key also compromises the ATProto signing key — acceptable for a custodial platform. **Never derive the rotation key this way** — keep rotation keys independent so signing key compromise doesn't compromise identity recovery.

For MVP, generate fresh independent keypairs. Key derivation can be explored in Phase 4.

### 2.4 Identity Linking

**Nostr → ATProto mapping storage:**

```sql
CREATE TABLE identity_links (
    nostr_pubkey    TEXT PRIMARY KEY,  -- hex-encoded secp256k1 pubkey
    atproto_did     TEXT UNIQUE NOT NULL,
    atproto_handle  TEXT UNIQUE NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    opted_in        BOOLEAN NOT NULL DEFAULT FALSE,
    signing_key_id  TEXT NOT NULL  -- reference to encrypted key storage
);
```

**Cross-protocol discovery:**

1. **ATProto DID document** — Include the Nostr pubkey in `alsoKnownAs`:
   ```json
   {
     "id": "did:plc:xxxxx",
     "alsoKnownAs": [
       "at://alice.divine.video",
       "nostr:npub1xxxxxx"
     ]
   }
   ```

2. **Nostr kind 0 metadata** — Include the ATProto DID in content and NIP-39 `i` tags:
   ```json
   {
     "kind": 0,
     "content": "{\"name\":\"alice\",\"about\":\"...\",\"nip05\":\"alice@divine.video\",\"atproto_did\":\"did:plc:xxxxx\",\"atproto_handle\":\"alice.divine.video\"}",
     "tags": [
       ["i", "atproto:did:plc:xxxxx", ""],
       ["i", "atproto:alice.divine.video", ""]
     ]
   }
   ```

   NIP-39 `i` tags are the standards-compliant way to include external identity claims. DiVine should publish both the content field and `i` tags for maximum compatibility.

3. **Bidirectional verification** (gold standard): The ATProto DID document claims the Nostr pubkey via `alsoKnownAs`, AND the Nostr kind 0 claims the ATProto DID. A verifier can check both directions — mutual attestation proves intentional linkage.

### 2.5 User Consent Flow

**Recommended: Opt-in with a simple toggle**

```
┌─────────────────────────────────────────┐
│ Settings → Cross-posting                 │
│                                          │
│ ☐ Share videos on Bluesky/ATProto       │
│                                          │
│ When enabled, your DiVine videos will    │
│ automatically appear on Bluesky,         │
│ Skylight, and other ATProto apps.        │
│                                          │
│ Your ATProto handle:                     │
│ alice.divine.video                       │
│                                          │
│ [Enable ATProto Sharing]                 │
└─────────────────────────────────────────┘
```

**On opt-in, the system:**
1. Generates ATProto signing keypair
2. Creates did:plc entry via PLC directory
3. Creates ATProto repo on DiVine's PDS
4. Sets up handle resolution
5. Backfills existing videos (optional, configurable)
6. Begins real-time cross-posting

---

## 3. PDS Architecture & Implementation

### 3.1 Deployment Model

**Single multi-tenant PDS at `pds.divine.video`**

```
Infrastructure:
├── Compute: 2-4 vCPU, 8-16GB RAM (Kubernetes pod or VM)
├── Database: PostgreSQL 15+ (managed, e.g., Cloud SQL)
├── Blob Storage: S3-compatible (GCS, R2, MinIO)
├── Cache: Redis (optional, for hot path optimization)
└── DNS: Wildcard *.divine.video → PDS
```

Based on self-hosting reports, a PDS is lightweight for moderate user counts. The critical scaling factor is blob storage (video files) and firehose bandwidth.

**Federation requirements:**
- PDS must implement `com.atproto.sync.*` endpoints for the relay network to crawl
- The Bluesky relay (BGS) crawls PDS instances and aggregates into the firehose
- Current rate limits: **1,500 events/hour, 10,000 events/day** per PDS (for 10 accounts)
- ATProto team plans auto-scaling rate limits for established PDS hosts
- DiVine should request elevated limits given the multi-tenant nature

### 3.2 Nostr Relay Subscription

The PDS includes a **Nostr bridge service** that subscribes to funnelcake:

```rust
// Nostr WebSocket subscription to funnelcake relay
let filter = Filter::new()
    .kinds(vec![
        Kind::Custom(34235),  // Addressable normal video
        Kind::Custom(34236),  // Addressable short video
        Kind::Custom(5),      // Deletion events (NIP-09)
    ]);

// Subscribe with REQ
relay.subscribe(filter).await?;
```

**Subscription strategy:**
- Subscribe to **addressable video events** (kinds 34235, 34236) — these are the primary content types
- Also subscribe to **kind 5 deletion events** from registered users
- Filter server-side to only process events from users who have opted in to ATProto
- Maintain a cursor/timestamp of last processed event for catch-up after downtime

**Reconnection and backfill:**
```rust
struct BridgeState {
    last_processed_event_id: String,
    last_processed_timestamp: u64,
    // On reconnection, request events since last_processed_timestamp
    // funnelcake supports SINCE filter for backfill
}
```

### 3.3 Event Translation Pipeline

```
┌──────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ Nostr Event   │────▶│ Translation       │────▶│ ATProto Record  │
│ (NIP-71)      │     │ Service           │     │ (app.bsky.*)    │
└──────────────┘     └──────────────────┘     └─────────────────┘
                            │
                            ├── Verify Nostr signature
                            ├── Look up user's ATProto identity
                            ├── Map metadata fields
                            ├── Handle blob (video file)
                            ├── Construct post record
                            └── Write to user's ATProto repo
```

**Signature verification** (key innovation):
```rust
// Before processing any event, verify the Nostr signature
fn verify_and_translate(event: NostrEvent) -> Result<AtProtoRecord> {
    // 1. Verify schnorr signature on the event
    if !event.verify_signature() {
        return Err(Error::InvalidSignature);
    }

    // 2. Check the pubkey is registered for ATProto
    let identity = db.get_identity_link(&event.pubkey)?;
    if identity.is_none() || !identity.opted_in {
        return Ok(());  // Skip unregistered users
    }

    // 3. Check for duplicate (idempotency)
    if db.event_already_processed(&event.id) {
        return Ok(());
    }

    // 4. Translate and write
    let record = translate_nip71_to_atproto(&event, &identity)?;
    pds.write_record(&identity.atproto_did, record).await?;

    // 5. Mark as processed
    db.mark_event_processed(&event.id).await?;

    Ok(())
}
```

### 3.4 Blob Management

**Recommendation: Option A — Fetch from Blossom, re-upload to S3**

This is the only fully spec-compliant approach:

```
Blossom Server                    DiVine PDS (S3)
┌─────────────┐                  ┌─────────────────┐
│ video.mp4   │ ───GET by───▶   │ ATProto blob     │
│ (SHA-256)   │    hash          │ (CID-referenced) │
└─────────────┘                  └─────────────────┘
```

**Why Option A:**
- ATProto requires blobs to be stored by the PDS and referenced by CID (Content Identifier)
- ATProto CIDs use SHA-256 internally but wrap in a CID envelope (multicodec + multihash)
- Blossom uses raw SHA-256 hex hashes
- The PDS must serve blobs via `com.atproto.sync.getBlob` — it must have the actual bytes
- ATProto relays/AppViews expect to fetch blobs from the PDS, not from external URLs

**Why not Option B** (direct Blossom reference): ATProto's blob system requires the PDS to serve blob content. External URLs are not supported in the blob reference format.

**Why not Option C** (shared S3): Would work if both systems can be configured to use the same bucket, but the CID vs SHA-256 addressing mismatch means you'd need a mapping layer anyway. Also couples the systems operationally.

**Implementation:**
```rust
async fn handle_video_blob(event: &NostrEvent) -> Result<BlobRef> {
    // Extract video URL and hash from imeta tags
    let (video_url, sha256_hash) = extract_video_info(event)?;

    // Check if we already have this blob
    if let Some(existing) = blob_store.get_by_sha256(&sha256_hash).await? {
        return Ok(existing.cid_ref());
    }

    // Fetch from Blossom
    let video_bytes = blossom_client.get(&sha256_hash).await?;

    // Verify hash
    assert_eq!(sha256(&video_bytes), sha256_hash);

    // Upload to S3 as ATProto blob
    let cid = compute_cid(&video_bytes);
    blob_store.put(&cid, &video_bytes, "video/mp4").await?;

    Ok(BlobRef { cid, mime_type: "video/mp4", size: video_bytes.len() })
}
```

**Option C upgrade path for production**: Configure both Blossom and rsky-pds to use the same S3 bucket. Blossom stores objects keyed by `<sha256hex>.mp4`; rsky-pds stores by CID. The actual video bytes are stored once with both systems maintaining their own key/metadata. This eliminates storage duplication — recommended after MVP is proven.

**Storage cost estimate** (5 MB average per 6-second video):

| Scale | Corpus Size | AWS S3/mo | Cloudflare R2/mo |
|-------|------------|-----------|-----------------|
| 10K videos | 50 GB | ~$91 (storage + egress) | ~$0.75 (zero egress) |
| 100K videos | 500 GB | ~$460 | ~$7.50 |
| 1M videos | 5 TB | ~$4,600 | ~$75 |

**Recommendation: Use Cloudflare R2 from day 1.** Zero egress costs eliminate the main variable cost driver as video volume grows. CDN-front the blob serving for additional performance.

### 3.5 Repo Management

ATProto repositories use a Merkle Search Tree (MST):

```
Repository Structure:
├── Commit (signed, includes DID, rev, data CID)
│   └── MST Root
│       ├── app.bsky.feed.post/3k...  → Post record CID
│       ├── app.bsky.feed.post/3k...  → Post record CID
│       └── app.bsky.actor.profile/self → Profile record CID
```

**Key operations:**
- **Create repo**: Initialize MST, create first commit, register with relay
- **Write record**: Add record to MST, create new commit, sign with repo signing key
- **Sync**: Relay crawls PDS via `com.atproto.sync.getRepo` and `com.atproto.sync.subscribeRepos`

rsky-pds handles all MST operations. DiVine's bridge service can write records via two paths:

**Path 1 — HTTP API** (simplest, recommended for MVP):
```
POST /xrpc/com.atproto.repo.uploadBlob    → upload video, get BlobRef (CID)
POST /xrpc/com.atproto.repo.createRecord   → write post record to user repo
POST /xrpc/com.atproto.repo.putRecord      → upsert (for NIP-71 event updates)
POST /xrpc/com.atproto.repo.applyWrites    → batch operations
```

**Path 2 — Internal ActorStore API** (for performance):
```rust
let store = ActorStore::new(did, S3BlobStore::new(), db_conn);
store.process_writes(writes, swap_commit_cid)?;
sequencer.sequence_commit(did, commit)?; // publish to firehose
```

**Blob S3 key pattern**: `{did}/{cid}` — configurable via `AWS_ENDPOINT` env var for R2/MinIO.

**Key configuration** (env vars):
- `PDS_HOSTNAME`, `PDS_SERVICE_DID`, `PDS_SERVICE_HANDLE_DOMAINS`
- `DATABASE_URL`, `AWS_ENDPOINT`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`
- `PDS_DID_PLC_URL` (default `https://plc.directory`)
- `PDS_BSKY_APP_VIEW_URL`, `PDS_BSKY_APP_VIEW_DID` (AppView proxy)

### 3.6 Deletion Propagation

```
Nostr NIP-09 (kind 5)          ATProto
┌─────────────────┐           ┌─────────────────┐
│ Deletion Event   │──────────▶│ Delete record    │
│ references       │  Bridge   │ from MST         │
│ event ID         │  Service  │ New commit       │
└─────────────────┘           └─────────────────┘
```

When DiVine receives a NIP-09 deletion event:
1. Look up the corresponding ATProto record by the Nostr event ID
2. Delete the record from the user's ATProto repo
3. The relay network propagates the deletion

---

## 4. Video Format & Protocol Mapping

### 4.1 Field-by-Field Mapping: NIP-71 → ATProto

| NIP-71 Field | ATProto Field | Notes |
|---|---|---|
| `title` tag | `app.bsky.feed.post.text` | Prepended to post text |
| `content` (description) | `app.bsky.feed.post.text` | Combined with title, max 300 graphemes |
| `t` tags (hashtags) | `app.bsky.feed.post.facets[]` | Tag facets with byte offsets |
| `imeta.url` (video) | `app.bsky.embed.video.video` | Blob reference (CID) |
| `imeta.dim` | `app.bsky.embed.video.aspectRatio` | Parse "WxH" → `{width, height}` |
| `imeta.image` (thumbnail) | `app.bsky.embed.video.thumb` (if supported) | May need AppView support |
| `imeta.m` (MIME type) | Blob MIME type | `video/mp4` |
| `imeta.x` (SHA-256 hash) | Blob CID | Convert SHA-256 → CID |
| `duration` tag | No direct equivalent | Include in alt text |
| `alt` tag | `app.bsky.embed.video.alt` | Accessibility description |
| `text-track` tags | `app.bsky.embed.video.captions[]` | Language-tagged VTT files |
| `content-warning` tag | `app.bsky.feed.post.labels` | Self-labels (see §6) |
| `published_at` tag | `app.bsky.feed.post.createdAt` | ISO 8601 datetime |
| `p` tags (participants) | `app.bsky.feed.post.facets[]` | Mention facets (if ATProto identity exists) |
| `r` tags (references) | `app.bsky.feed.post.facets[]` | Link facets |
| Event `created_at` | `app.bsky.feed.post.createdAt` | Unix timestamp → ISO 8601 |

### 4.2 Post Record Construction

```json
{
  "$type": "app.bsky.feed.post",
  "text": "Sunset over the bay 🌅 #sunset #nature #divine",
  "createdAt": "2026-03-20T12:00:00.000Z",
  "langs": ["en"],
  "facets": [
    {
      "index": {"byteStart": 27, "byteEnd": 34},
      "features": [{"$type": "app.bsky.richtext.facet#tag", "tag": "sunset"}]
    },
    {
      "index": {"byteStart": 35, "byteEnd": 42},
      "features": [{"$type": "app.bsky.richtext.facet#tag", "tag": "nature"}]
    },
    {
      "index": {"byteStart": 43, "byteEnd": 50},
      "features": [{"$type": "app.bsky.richtext.facet#tag", "tag": "divine"}]
    }
  ],
  "embed": {
    "$type": "app.bsky.embed.video",
    "video": {
      "$type": "blob",
      "ref": {"$link": "bafyreid..."},
      "mimeType": "video/mp4",
      "size": 3145728
    },
    "alt": "6-second loop: Sunset over the bay",
    "aspectRatio": {"width": 9, "height": 16}
  }
}
```

### 4.3 Blob Hash Mapping

**Critical insight**: The SHA-256 hash of the file bytes is identical in both protocols. Blossom's `x` tag and the ATProto blob CID's multihash component contain the same 32-byte digest. CID construction from the Blossom hash is possible without re-hashing.

```
Blossom (Nostr)                    ATProto
─────────────────────              ─────────────────────
SHA-256 hex hash                   CID (Content Identifier)
e.g., "a1b2c3d4..."              e.g., "bafkreia1b..."

Format: 64-char hex string         Format: multibase(multicodec + multihash)
                                   - multibase: "b" (base32lower)
                                   - CID version: 1
                                   - multicodec: 0x55 (raw bytes)
                                   - multihash: 0x12 (sha2-256) + 0x20 (32 bytes) + digest

Conversion:
  bytes = hex_decode(blossom_x_tag)           // 32 bytes
  multihash = [0x12, 0x20] ++ bytes           // sha2-256 code + length + digest
  cid = CIDv1(codec=0x55, multihash)          // raw bytes codec
  encoded = multibase_encode(base32lower, cid) // "bafkrei..." prefix
```

### 4.3.1 Record Key Strategy

Use the NIP-71 `d` tag as the ATProto record key (`rkey`). This enables:
- **Idempotent writes**: Same Nostr event always maps to the same ATProto record
- **Cross-protocol addressing**: Given a `d` tag, construct the AT-URI directly
- **Update support**: When a user publishes an updated NIP-71 event with the same `d` tag, the PDS uses `putRecord` (upsert) rather than `createRecord`

ATProto rkeys must match `[a-zA-Z0-9._~:@!$&'()*+,;=-]+` and be ≤512 chars. If the `d` tag contains invalid characters, use a base32-encoded hash of the `d` tag value.

### 4.4 Post Text Construction Algorithm

ATProto `feed.post.text` has a **300-grapheme limit**. Facet byte offsets must be computed on **UTF-8 byte positions** (not character offsets).

```rust
fn build_post_text(event: &NostrEvent) -> (String, Vec<Facet>) {
    let title = event.get_tag("title").unwrap_or_default();
    let summary = event.get_tag("summary")
        .or_else(|| if !event.content.is_empty() { Some(&event.content) } else { None })
        .unwrap_or_default();
    let hashtags: Vec<String> = event.get_tags("t")
        .iter().map(|t| format!("#{}", t)).collect();
    let hashtag_str = hashtags.join(" ");

    // Build candidate text
    let candidate = match (title.is_empty(), summary.is_empty()) {
        (false, false) => format!("{}\n\n{}", title, summary),
        (false, true) => title.to_string(),
        (true, false) => summary.to_string(),
        (true, true) => String::new(),
    };

    let full_text = if !hashtag_str.is_empty() {
        format!("{}\n\n{}", candidate, hashtag_str)
    } else {
        candidate
    };

    // Truncate to 300 graphemes if needed (preserving hashtags)
    let text = truncate_graphemes(&full_text, 300);

    // Compute UTF-8 byte-offset facets for hashtags
    let facets = compute_hashtag_facets(&text);

    (text, facets)
}
```

### 4.5 6-Second Loop Considerations

- DiVine videos are 6 seconds, well under ATProto's 100MB / 3-minute limits
- **Aspect ratio**: DiVine vertical video = `{width: 9, height: 16}` (from `dim` tag via GCD reduction)
- **Looping**: ATProto has **no `loop` field** in `app.bsky.embed.video`. However, Bluesky and Skylight clients auto-loop short videos by default — DiVine's 6-second format will loop naturally without any special signaling
- The `app.bsky.embed.video` view includes a `presentationHint` field with values `'default'`, `'gif'`, or custom strings — DiVine should set this to `'gif'` for explicit loop-like behavior
- **Thumbnails**: ATProto video embed has no poster/thumbnail field — clients use the first video frame. DiVine thumbnails from Blossom are not directly representable
- Alt text should mention "6-second loop" for accessibility and context

### 4.5 Engagement Mapping

**Recommendation: One-directional (Nostr → ATProto) for MVP**

| Direction | Action | Recommendation |
|---|---|---|
| Nostr → ATProto | Video post | ✅ Automatic translation |
| Nostr → ATProto | Deletion | ✅ Propagate via NIP-09 |
| Nostr → ATProto | Profile update | ✅ Phase 2 |
| ATProto → Nostr | Like | ❌ Skip for now |
| ATProto → Nostr | Reply | ❌ Skip for now |
| ATProto → Nostr | Repost | ❌ Skip for now |

**Rationale**: Bidirectional engagement sync is complex (requires firehose subscription, identity resolution in reverse, and Nostr event creation with DiVine's keys). The value-add is low compared to the implementation cost. Engagement metrics on ATProto can be displayed in DiVine's UI separately.

### 4.6 Profile Sync (Phase 2)

| Nostr Kind 0 | ATProto app.bsky.actor.profile | Notes |
|---|---|---|
| `name` / `display_name` | `displayName` | Direct mapping |
| `about` | `description` | Max 2560 graphemes |
| `picture` | `avatar` | Blob reference |
| `banner` | `banner` | Blob reference |
| NIP-05 | Handle | Already handled by DNS |

---

## 5. Feed & Discovery Integration

### 5.1 Feed Generators

DiVine should operate ATProto feed generators that surface DiVine content. Feed generators are **external HTTP services** — they return only a list of AT-URIs (no post content). The AppView handles all hydration.

**Three feeds to launch:**

| Feed | Algorithm | Data Source |
|------|-----------|-------------|
| `divine-latest` | Reverse chronological | DiVine PDS PostgreSQL directly |
| `divine-trending` | Engagement velocity scoring | Engagement table (Nostr reactions + ATProto likes) |
| `divine-for-you` | Personalized ML | Gorse HTTP API per user DID |

**Key simplification**: Most feed generators consume the full ATProto firehose (100K+ posts/day). DiVine only needs its own posts — index from DiVine's PDS PostgreSQL directly. This is a massive operational simplification.

**Feed generator DID**: `did:web:feed.divine.video` — no PLC dependency for service DIDs.

**Implementation with rsky-feedgen:**
```rust
// Feed generator skeleton
async fn get_feed_skeleton(params: GetFeedSkeletonParams) -> Result<FeedSkeleton> {
    let feed_uri = &params.feed;

    match feed_uri {
        "at://did:plc:divine-service/app.bsky.feed.generator/trending" => {
            // Query Gorse for trending video recommendations
            let recommendations = gorse_client
                .get_popular_items("video", params.limit)
                .await?;

            // Map Gorse item IDs to AT-URIs
            let posts: Vec<SkeletonItem> = recommendations
                .iter()
                .filter_map(|rec| db.get_atproto_uri_for_item(&rec.item_id))
                .map(|uri| SkeletonItem { post: uri })
                .collect();

            Ok(FeedSkeleton { feed: posts, cursor: None })
        },
        "at://did:plc:divine-service/app.bsky.feed.generator/latest" => {
            // Simple reverse-chronological feed of DiVine videos
            let posts = db.get_recent_divine_posts(params.limit, params.cursor).await?;
            Ok(FeedSkeleton { feed: posts, cursor: posts.last().map(|p| p.cursor.clone()) })
        },
        _ => Err(Error::UnknownFeed),
    }
}
```

**Feed registration flow:**
1. Feed generator runs as a service with its own DID
2. Publishes `app.bsky.feed.generator` record describing available feeds
3. Users on Bluesky/Skylight can "pin" DiVine feeds to their feed tabs
4. When a user requests the feed, their PDS calls DiVine's `getFeedSkeleton` endpoint
5. The PDS hydrates the skeleton with full post data

### 5.2 Skylight Compatibility

Skylight (backed by Mark Cuban, launched April 2025, 55K users in first 24 hours) uses standard `app.bsky.embed.video` in `app.bsky.feed.post`. DiVine's output will render correctly in Skylight because:

- DiVine uses the standard Bluesky video embed lexicon
- Skylight supports videos up to 3 minutes (DiVine's 6-second loops are well within limits)
- Skylight renders the same post format as Bluesky
- Short videos naturally loop in ATProto video players

**Considerations:**
- Skylight has its own recommendation algorithm — DiVine content may or may not surface organically
- DiVine's feed generator gives Skylight users a direct way to discover DiVine content
- The `presentationHint: 'gif'` setting should encourage auto-loop behavior

### 5.3 Flashes Compatibility

Flashes (Instagram-like ATProto app by Sebastian Vogelsang, 68K+ followers within 2 weeks) also renders `app.bsky.embed.video` posts. DiVine videos will appear in Flashes feeds. The vertical video format (`9:16` aspect ratio) aligns well with Flashes' visual feed.

### 5.4 Custom Lexicons vs Standard

**Recommendation: Use standard `app.bsky.embed.video` exclusively for Phase 1-2**

| Approach | Pros | Cons |
|---|---|---|
| Standard `app.bsky.embed.video` | Works in all ATProto apps immediately | Limited metadata fields |
| Custom `video.divine.*` | Full metadata control | Only visible in apps that implement the lexicon |

**Metadata that doesn't fit standard lexicons:**
- Loop flag → Use `presentationHint: 'gif'`
- Nostr event reference → Store in a separate record or omit
- Blossom hash → Internal mapping, not exposed
- Duration (6s) → Include in alt text

**Phase 2+ Dual-Write Strategy**: Write **both** records per video:
1. `app.bsky.feed.post` + `app.bsky.embed.video` — renders in Skylight, Flashes, Bluesky
2. `video.divine.post` custom record — carries Nostr provenance (event ID, Blossom hash, npub, loop flag)

The standard post is what feed generators return. The custom record enables DiVine-aware clients to show full metadata.

**Propose `loop: boolean` to [lexicon.community](https://lexicon.community)** — other short-form video apps (Flashes, Skylight, Reelo/Spark) would benefit from a standard loop flag, raising DiVine's profile in the ATProto ecosystem.

### 5.5 Cold-Start Discovery Strategy

1. **Feed marketplace**: Register all three DiVine feeds in `bsky.app/feeds` with clear descriptions on day one
2. **Starter pack**: Create "DiVine on Bluesky" starter pack bundling top 20 DiVine creators + all three feeds — one shareable link
3. **In-app CTA**: Surface Bluesky follow prompts within DiVine's own app for existing users
4. **Identity linking**: Allow existing Bluesky users to connect their handle to DiVine
5. **Cross-protocol engagement**: When a DiVine video gets ATProto likes, show that signal in DiVine's app to motivate creators

---

## 6. Moderation & Trust and Safety

### 6.1 DiVine as ATProto Labeler

DiVine should operate as an ATProto labeler service, mapping its AI classification to ATProto labels:

**Labeler service setup:**
- Labeler DID with `#atproto_label` signing key
- `#atproto_labeler` service endpoint
- Implements `com.atproto.label.subscribeLabels` WebSocket
- Implements `com.atproto.label.queryLabels` HTTP endpoint

**Label format:**
```json
{
  "ver": 1,
  "src": "did:plc:divine-labeler",
  "uri": "at://did:plc:user123/app.bsky.feed.post/abc123",
  "val": "sexual",
  "cts": "2026-03-20T12:00:00.000Z",
  "sig": "<signature>"
}
```

### 6.2 Content Label Mapping

| DiVine AI Classification | ATProto Label | Behavior |
|---|---|---|
| NSFW / Adult | `porn` | Hidden by default |
| Suggestive | `sexual` | Warning interstitial |
| Nudity (artistic) | `nudity` | Warning interstitial |
| Violence/Gore | `gore` | Hidden by default |
| Sensitive content | `graphic-media` | Warning interstitial |
| Safe | (no label) | Shown normally |

### 6.3 Content Warning Mapping

```
Nostr content-warning tag    →    ATProto self-label
─────────────────────────         ─────────────────────
"NSFW"                       →    val: "porn" or "sexual"
"spoiler"                    →    val: "!warn" (custom)
"violence"                   →    val: "gore"
(any text)                   →    val: "!warn" + custom label text
```

Self-labels are applied by the PDS when writing the record:
```json
{
  "$type": "app.bsky.feed.post",
  "text": "...",
  "labels": {
    "$type": "com.atproto.label.defs#selfLabels",
    "values": [
      {"val": "sexual"}
    ]
  }
}
```

### 6.4 Incoming ATProto Moderation

When Bluesky's Ozone or other labelers flag DiVine content:

```
ATProto Labeler (e.g., Ozone)
       │
       ▼
Labels DiVine content with "!takedown"
       │
       ▼
DiVine PDS receives label via subscription
       │
       ├── If "!takedown": Remove record from ATProto repo
       ├── If "!suspend": Disable user's ATProto account
       └── Feed back to DiVine moderation queue for Nostr-side review
```

**PDS requirements:**
- PDS MUST honor `!takedown` labels from the relay's configured labeler
- The `atproto-accept-labelers` header with `redact` parameter triggers content removal
- DiVine should propagate ATProto takedowns to its Nostr moderation queue

### 6.5 Blocking / Muting Sync

| Action | ATProto | Nostr |
|---|---|---|
| User blocks user | `app.bsky.graph.block` record | `kind: 10000` mute list (NIP-51) |
| Subscribe to block list | `app.bsky.graph.listblock` | Mute list with list pubkey |
| Operator bans account | `com.atproto.admin.updateAccountStatus` (takendown) | Relay-level pubkey ban |
| Report content | `com.atproto.moderation.createReport` | `kind: 1984` (NIP-56) |

### 6.6 Legal Compliance

| Requirement | Nostr Side | ATProto Side |
|---|---|---|
| DMCA takedown | Advisory deletion (NIP-09) — can't force all relays | PDS can enforce deletion, remove from repo |
| Right to be forgotten | Cannot guarantee on Nostr | PDS can delete all records and revoke DID |
| Geographic restrictions | Not enforceable at protocol level | PDS can geo-block at serving layer |
| CSAM | Immediate removal from own relay, **do not bridge** | Immediate PDS deletion + NCMEC report |

**Key asymmetry**: Nostr deletion is advisory; ATProto deletion is authoritative (the PDS controls the repo). DiVine's PDS should prioritize compliance on the ATProto side, where it has enforcement power.

**DMCA Safe Harbor (17 U.S.C. § 512(c))**: DiVine qualifies as a service provider storing content at user direction. Requirements: designated DMCA agent registered with US Copyright Office, expeditious response to valid takedowns, repeat infringer policy. DiVine should maintain a SHA-256 content hash registry of taken-down material for repeat-upload detection across both protocols.

**Privacy policy must disclose**: Content bridged to Nostr propagates to a decentralized network not under DiVine's control. Complete deletion from all third-party relays cannot be guaranteed.

### 6.7 Recommended Labeler Architecture

DiVine should operate **two labeler identities**:

1. **`did:web:labeler.divine.video`** — Public labeler for AI-classified content. Bluesky users can subscribe to get DiVine's content classifications applied.
2. **PDS admin account** — For platform-level enforcement (takedowns, account bans) not appropriate for public labeler distribution.

**Use Ozone** (Bluesky's open-source moderation tool, MIT-licensed) as the moderation backend. Extend it with Nostr integration for bidirectional report handling and NIP-09 deletion publishing.

---

## 7. Product Roadmap & Phasing

### Phase 1 — MVP (8-10 person-weeks)

**Goal**: DiVine videos appear on ATProto network

| Task | Effort | Dependencies |
|---|---|---|
| did:plc registration flow | 1 week | PLC directory API |
| Handle resolution (`.well-known`) | 0.5 weeks | DNS setup |
| Custodial key management | 1 week | Secrets storage |
| rsky-pds deployment & configuration | 2 weeks | PostgreSQL, S3 |
| Nostr relay subscription (funnelcake) | 1 week | funnelcake WebSocket |
| NIP-71 → ATProto translation | 1.5 weeks | Video mapping spec |
| Blob management (Blossom → S3) | 1 week | Blossom API |
| Deletion propagation (NIP-09) | 0.5 weeks | — |
| User opt-in UI | 0.5 weeks | Mobile app update |

**Infrastructure costs**: ~$200-500/month (PDS compute + PostgreSQL + S3 + bandwidth)

**Success metrics:**
- Videos posted on DiVine appear on Bluesky within 60 seconds
- 95%+ translation success rate
- PDS stays in sync with relay network

### Phase 2 — Discovery (4-6 person-weeks)

**Goal**: DiVine content is discoverable on ATProto

| Task | Effort |
|---|---|
| Feed generator (trending + latest) | 2 weeks |
| Gorse → feed generator integration | 1 week |
| Profile sync (Nostr → ATProto) | 1 week |
| Improved metadata mapping | 1 week |
| Content labeling (self-labels) | 0.5 weeks |

**Additional infrastructure**: Feed generator service (~$100/month)

**Success metrics:**
- "DiVine Trending" feed has 1,000+ subscribers on Bluesky/Skylight
- Profile data syncs within 5 minutes of Nostr update
- Content appears correctly on Skylight and Flashes

### Phase 3 — Moderation & Trust (3-4 person-weeks)

**Goal**: Robust cross-protocol moderation

| Task | Effort |
|---|---|
| DiVine ATProto labeler service | 2 weeks |
| AI classification → ATProto label mapping | 1 week |
| Incoming moderation (Ozone integration) | 1 week |
| DMCA / takedown workflow | 0.5 weeks |

**Success metrics:**
- All DiVine AI labels propagated to ATProto within 30 seconds
- 100% compliance with ATProto takedown requests
- Bidirectional moderation feedback loop operational

### Phase 4 — Advanced (6-8 person-weeks)

**Goal**: Deep integration and experimentation

| Task | Effort |
|---|---|
| Engagement display (ATProto likes/replies in DiVine UI) | 2 weeks |
| Custom lexicon exploration (with lexicon.community) | 2 weeks |
| Bidirectional engagement sync (if justified) | 3 weeks |
| Non-video event translation (kind 1, kind 30023) | 1 week |

---

## 8. Competitive & Strategic Analysis

### 8.1 Landscape

| Platform | Approach | Video Support | Network | Status |
|---|---|---|---|---|
| **DiVine** | Nostr-native + ATProto PDS | 6-sec loops | Nostr + ATProto | Adding ATProto |
| **Skylight** | ATProto-native | Up to 3 min | ATProto only | Launched Apr 2025, Mark Cuban backed |
| **Flashes** | ATProto-native | Photo + video | ATProto only | Launched Feb 2025, 68K+ followers |
| **Reelo/Spark** | Custom ATProto lexicon | Up to 3 min | ATProto (custom) | Pre-launch, sole developer |
| **Bridgy Fed** | Bridge service | N/A | ActivityPub ↔ ATProto | Live, Nostr planned |
| **Mostr** | Bridge | N/A | Nostr ↔ ActivityPub | Live |

### 8.2 DiVine vs Skylight

| Dimension | DiVine | Skylight |
|---|---|---|
| Protocol | Dual (Nostr + ATProto) | ATProto only |
| Content format | 6-sec loops (distinctive) | General short-form video |
| User identity | Nostr keypairs (user-sovereign) | ATProto DIDs |
| Infrastructure | Own relay + PDS | Bluesky infrastructure |
| Funding | Jack Dorsey / andOtherStuff | Mark Cuban |
| Unique angle | Nostr-first, censorship-resistant | TikTok replacement on ATProto |

**Strategic advantage**: DiVine's dual-protocol approach means content is available on both Nostr (censorship-resistant, user-sovereign keys) and ATProto (large user base, rich app ecosystem). If either network has issues, the other continues to work.

### 8.3 Bridge vs Native PDS

| Factor | Bridgy Fed / Bridge | DiVine's Own PDS |
|---|---|---|
| Control | Third-party dependency | Full control |
| Identity | Bridge-assigned identity | `username.divine.video` handles |
| Reliability | Depends on bridge uptime | DiVine-operated |
| Customization | Generic translation | DiVine-optimized mapping |
| Branding | Bridge attribution | Native DiVine branding |
| Video support | Limited / no video bridging | Full video support |
| Nostr support | Not yet (planned for 2026) | N/A (native integration) |

**Verdict**: Running a native PDS is clearly superior for DiVine's use case. Bridgy Fed doesn't even support Nostr yet, and generic bridges can't optimize for DiVine's specific video format.

### 8.4 Network Effects

- **Bluesky**: ~40.2M registered users, ~3.5M DAU
- **Skylight**: 55K+ users in first 24 hours (April 2025)
- **Flashes**: 68K+ followers, 10K beta limit hit in hours
- **Nostr**: Estimated 500K-1M active users

ATProto integration gives DiVine access to a network 40-80x larger than the Nostr ecosystem. Even capturing 0.1% of Bluesky's user base through feed subscriptions would be significant.

### 8.5 studio.coop Implications

The ATProto integration strengthens the cooperative model by:
- Demonstrating protocol interoperability as a cooperative value
- Creating a reusable pattern other Nostr projects can adopt
- Positioning studio.coop as a multi-protocol infrastructure provider

---

## 9. Open Questions & Risks

### 9.1 ATProto Spec Stability

**Risk: Medium**

- ATProto is approaching "AT 1.0" — the team is focused on maturity, not new features
- `app.bsky.embed.video` is stable and widely used (Bluesky, Skylight, Flashes all depend on it)
- Lexicon versioning prevents breaking changes (only additive changes allowed)
- The Patent Non-Aggression Pledge provides additional ecosystem safety

### 9.2 PDS Operational Burden

**Risk: Medium-Low**

- Self-hosted PDS setup is "frighteningly easy" per community reports
- rsky-pds uses PostgreSQL + S3 — familiar infrastructure
- Main challenges: account migration (one-way currently), domain configuration
- ATProto team is improving non-Bluesky PDS hosting with auto-scaling rate limits
- Alternative PDS implementations exist: Tranquil PDS (Rust), Cocoon (Go)

### 9.3 Rate Limits & Anti-Spam

**Risk: Medium-High**

- Current: 1,500 events/hour, 10,000 events/day per PDS (for 10 accounts)
- DiVine's 6-second video format means lower volume than a general social app
- If DiVine has 1,000 active posters at 5 videos/day = 5,000 events/day (within limits)
- ATProto plans auto-scaling rate limits for established PDS hosts
- **Anti-spam concern**: A PDS generating thousands of automated video posts could be flagged as spam by the BGS. Community reports suggest high-volume automated PDSes draw moderation attention
- **BGS crawl initiation**: New PDSes must request crawling from Bluesky's BGS — this is a manual/semi-manual step. Without it, content won't appear in Bluesky feeds
- Mitigation: Implement per-user rate limiting in bridge; require explicit opt-in; request elevated limits from relay team

### 9.4 Blob Storage Costs

**Risk: Low**

- 6-second videos are small (2-5 MB typically)
- Annual storage for 365K videos ≈ 1-2 TB ≈ $23-46/month on S3
- Bandwidth is the larger cost — CDN caching helps
- If DiVine grows to 10M videos, storage is still manageable (~$500-1000/month)

### 9.5 rsky Maturity

**Risk: Medium-High**

- rsky is in active development, not yet 1.0 — maintained primarily by a small Blacksky team
- Protocol parity gap: rsky may lag the reference PDS on newer lexicons (including video embeds)
- Apache 2.0 license means DiVine can fork if needed
- **However**: rsky-pds is actively maintained (commits March 2026), runs Blacksky in production, has full `com.atproto.sync.*` suite, and includes a dedicated video upload service (`rsky-video` with Bunny Stream transcoding)
- rsky-feedgen is tightly coupled to Blacksky's feed URIs — would need refactoring for DiVine's multi-tenant feeds
- **Alternative to evaluate**: Bluesky's official TypeScript PDS (`bluesky-social/pds`) has better protocol parity. The Rust efficiency gains from rsky may not justify the parity risk unless DiVine has deep Rust expertise
- Mitigation: Maintain close relationship with Rudy Fraser / Blacksky; plan for fallback to reference PDS

### 9.6 Nostr Signature Replay & Security

**Risk: Low-Medium**

Attack vectors and mitigations:

| Attack | Mitigation |
|--------|------------|
| Replay (rebroadcast old event) | Event ID dedup database; reject `created_at` older than 10-minute window |
| Key compromise | Emergency revocation mechanism to delist ATProto DID from bridge |
| Event type confusion | Strict kind validation — only accept kinds 34235, 34236, 5 |
| DoS via flood of valid events | Per-key rate limiting at bridge layer |
| PLC document manipulation | Monitor for unexpected DID document changes; alert on rotation |

### 9.7 Scope Expansion

**Recommendation**: Resist scope creep beyond video events initially

- Kind 1 (text notes) → app.bsky.feed.post translation is tempting but changes DiVine's identity from a video platform to a general Nostr→ATProto bridge
- Kind 30023 (long-form) has different mapping challenges
- Focus on doing video translation excellently before expanding

### 9.8 Legal Entity

**Recommendation**: andOtherStuff (nonprofit) operates the PDS

- Aligns with decentralization and cooperative values
- Nonprofit status provides some liability protection
- Consistent with studio.coop model
- Verse Communications handles commercial aspects

---

## 10. Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           DiVine User's Device                              │
│                                                                              │
│  ┌──────────────┐     ┌──────────────────┐                                  │
│  │ DiVine App    │────▶│ Video Recording   │                                 │
│  │ (Flutter)     │     │ + Nostr Signing    │                                │
│  └──────┬───────┘     └──────────────────┘                                  │
│         │                                                                    │
│         │ Signed NIP-71 Event + Video Blob                                  │
└─────────┼───────────────────────────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        DiVine Backend Infrastructure                         │
│                                                                              │
│  ┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐     │
│  │ Blossom Server    │     │ Funnelcake Relay  │     │ Gorse Recommend  │     │
│  │ (Video Storage)   │     │ (ClickHouse+NATS) │     │ Engine           │     │
│  │ SHA-256 addressed │     │ Nostr Events      │     │                  │     │
│  └────────┬─────────┘     └────────┬─────────┘     └────────┬─────────┘     │
│           │                         │                         │               │
│           │    ┌────────────────────┼─────────────────────────┘               │
│           │    │                    │                                          │
│           ▼    ▼                    ▼                                          │
│  ┌──────────────────────────────────────────────────────────────────────┐    │
│  │                    DiVine ATProto Bridge Service                      │    │
│  │                                                                       │    │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────┐  │    │
│  │  │ Nostr Sub    │  │ Signature    │  │ Event         │  │ Blob     │  │    │
│  │  │ (WebSocket   │  │ Verifier     │  │ Translator    │  │ Fetcher  │  │    │
│  │  │  to          │  │ (secp256k1)  │  │ (NIP-71 →     │  │ (Blossom │  │    │
│  │  │  funnelcake) │  │              │  │  ATProto)      │  │  → S3)   │  │    │
│  │  └──────┬───────┘  └──────┬───────┘  └──────┬────────┘  └────┬─────┘  │    │
│  │         │                  │                  │                │        │    │
│  │         └──────────────────┴──────────────────┴────────────────┘        │    │
│  │                                    │                                    │    │
│  └────────────────────────────────────┼────────────────────────────────────┘    │
│                                       │                                         │
│                                       ▼                                         │
│  ┌──────────────────────────────────────────────────────────────────────┐       │
│  │                         rsky-pds (Rust)                               │      │
│  │                         pds.divine.video                              │      │
│  │                                                                       │      │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────┐                │      │
│  │  │ Repo Manager │  │ Identity      │  │ Sync          │               │      │
│  │  │ (MST, CBOR,  │  │ (DID, Handle  │  │ (subscribeRe- │              │      │
│  │  │  Commits)    │  │  Resolution)  │  │  pos, getRepo) │              │      │
│  │  └──────────────┘  └──────────────┘  └───────┬────────┘              │      │
│  │                                               │                       │      │
│  │  ┌──────────────┐  ┌──────────────┐          │                       │      │
│  │  │ PostgreSQL    │  │ S3 Blob Store │          │                      │      │
│  │  │ (repos, keys) │  │ (video files) │          │                      │      │
│  │  └──────────────┘  └──────────────┘          │                       │      │
│  └──────────────────────────────────────────────┼───────────────────────┘      │
│                                                  │                              │
│  ┌──────────────────────────────────────────────┼───────────────────────┐      │
│  │                    Feed Generator + Labeler    │                      │      │
│  │                                                │                      │      │
│  │  ┌──────────────┐  ┌──────────────┐           │                      │      │
│  │  │ rsky-feedgen  │  │ rsky-labeler  │          │                      │      │
│  │  │ (Trending,    │  │ (AI labels    │          │                      │      │
│  │  │  Latest feeds)│  │  → ATProto)   │          │                      │      │
│  │  └──────────────┘  └──────────────┘           │                      │      │
│  └───────────────────────────────────────────────┼──────────────────────┘      │
│                                                   │                             │
└───────────────────────────────────────────────────┼─────────────────────────────┘
                                                    │
                    com.atproto.sync.subscribeRepos  │
                                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        ATProto Relay Network                                 │
│                                                                              │
│  ┌──────────────────┐                                                       │
│  │ Bluesky Relay     │ ◀── Crawls pds.divine.video                         │
│  │ (BGS)             │     via com.atproto.sync.*                           │
│  └────────┬──────────┘                                                      │
│           │                                                                  │
│           │ Firehose (com.atproto.sync.subscribeRepos)                      │
│           │                                                                  │
│  ┌────────┼──────────┬─────────────────┬───────────────────┐                │
│  │        ▼          ▼                 ▼                   ▼                │
│  │  ┌──────────┐ ┌──────────┐  ┌──────────────┐  ┌──────────────┐         │
│  │  │ Bluesky   │ │ Skylight  │  │ Flashes       │  │ Other ATProto │        │
│  │  │ AppView   │ │ (Video)   │  │ (Photo/Video) │  │ Apps          │        │
│  │  │ + Client  │ │           │  │               │  │               │        │
│  │  └──────────┘ └──────────┘  └──────────────┘  └──────────────┘         │
│  │                                                                          │
│  │  ┌──────────────┐                                                       │
│  │  │ PLC Directory │ ◀── DID registration and resolution                  │
│  │  │ (Swiss Assoc) │                                                      │
│  │  └──────────────┘                                                       │
│  │                                                                          │
│  │  ┌──────────────┐                                                       │
│  │  │ Ozone         │ ◀── Moderation labels → DiVine PDS                   │
│  │  │ (Moderation)  │                                                      │
│  │  └──────────────┘                                                       │
│  │                                                                          │
└──┴──────────────────────────────────────────────────────────────────────────┘
```

### Data Flow Summary

1. **User creates video** → DiVine app records 6-second loop, signs as NIP-71 Nostr event
2. **Video uploaded** → Blossom server (content-addressed by SHA-256)
3. **Event published** → Funnelcake relay (ClickHouse + NATS)
4. **Bridge subscribes** → Receives NIP-71 event via WebSocket
5. **Bridge verifies** → Checks Nostr signature, confirms user opted in
6. **Bridge translates** → Maps NIP-71 fields to `app.bsky.embed.video` + `app.bsky.feed.post`
7. **Blob fetched** → Downloads video from Blossom, uploads to PDS S3 storage
8. **Record written** → rsky-pds writes to user's ATProto repo (MST + commit)
9. **Relay crawls** → Bluesky BGS crawls PDS, adds to firehose
10. **Apps display** → Bluesky, Skylight, Flashes render the video post

---

## 11. Sources

### AT Protocol Specifications
- [AT Protocol DID Specification](https://atproto.com/specs/did)
- [AT Protocol Repository Specification](https://atproto.com/specs/repository)
- [AT Protocol Label Specification](https://atproto.com/specs/label)
- [AT Protocol Lexicons Guide](https://atproto.com/guides/lexicon)
- [AT Protocol Self-Hosting Guide](https://atproto.com/guides/self-hosting)
- [AT Protocol Fall 2025 Status Report](https://atproto.com/blog/protocol-check-in-fall-2025)
- [Bluesky Rate Limits](https://docs.bsky.app/docs/advanced-guides/rate-limits)
- [Custom Feed Tutorial](https://atproto.com/guides/custom-feed-tutorial)
- [Using Ozone](https://atproto.com/guides/using-ozone)

### Nostr Specifications
- [NIP-71: Video Events](https://raw.githubusercontent.com/nostr-protocol/nips/master/71.md)
- [NIP-B7: Blossom Media](https://nips.nostr.com/B7)
- [Blossom Protocol](https://github.com/hzrd149/blossom)

### Implementation References
- [rsky - Rust AT Protocol Implementation](https://github.com/blacksky-algorithms/rsky)
- [Bluesky ATProto Repository](https://github.com/bluesky-social/atproto)
- [Bluesky Feed Generator Starter Kit](https://github.com/bluesky-social/feed-generator)
- [Ozone Moderation Tool](https://github.com/bluesky-social/ozone)

### Competitive Analysis
- [Mark Cuban backs Skylight (TechCrunch)](https://techcrunch.com/2025/04/01/mark-cuban-backs-skylight-a-tiktok-alternative-built-on-blueskys-underlying-technology/)
- [Reelo/Spark stands out (TechCrunch)](https://techcrunch.com/2025/01/28/reelo-stands-out-among-the-apps-building-a-tiktok-for-bluesky/)
- [Flashes opens beta (TechCrunch)](https://techcrunch.com/2025/02/06/flashes-a-photo-sharing-app-for-bluesky-opens-beta/)
- [ATProto Explained – Lexicons and Video](https://fediversereport.com/atproto-explained-lexicons-and-video/)
- [Bridgy Fed Documentation](https://fed.brid.gy/docs)
- [Bluesky Statistics 2026](https://backlinko.com/bluesky-statistics)
- [Self-Hosting PDS Experience](https://blog.bront.rodeo/setting-up-your-own-pds/)
- [Running a Full-Network ATProto Relay](https://whtwnd.com/bnewbold.net/3kwzl7tye6u2y)

### Cross-Protocol
- [Mostr Bridge](https://soapbox.pub/blog/mostr-fediverse-nostr-bridge/)
- [nipy-bridge (Nostr to AT/ActivityPub)](https://github.com/0n4t3/nipy-bridge)
- [Follow Bluesky from Nostr via Mostr + Bridgy Fed](https://soapbox.pub/blog/follow-bluesky/)
