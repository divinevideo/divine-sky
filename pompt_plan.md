# Deep Research Prompt: DiVine ATProto PDS Integration — Technical Spec & Product Plan

## Context

You are helping design the technical architecture and product plan for integrating AT Protocol support into DiVine (divine.video), a Nostr-native 6-second looping video platform built by Verse Communications / andOtherStuff.

DiVine currently publishes video content as signed Nostr events (NIP-71 video events, NIP-92 file metadata) with video files stored and served via Blossom (a Nostr-native content-addressed media hosting layer). The backend is built in Rust, using a custom relay called "funnelcake" backed by ClickHouse and NATS, with a Gorse-based recommendation engine and an AI-powered moderation/classification service.

The goal is to add native AT Protocol support so that DiVine videos are simultaneously available on ATProto-based apps (Bluesky, Skylight, Flashes, and the broader "Atmosphere" ecosystem) without requiring third-party bridges like Bridgy Fed or Eclipse. DiVine users should not need to understand or interact with ATProto directly — their existing Nostr-signed video posts should automatically appear on ATProto networks.

## Core Architecture Decision

Rather than dual-writing from the application backend, the design uses DiVine's Nostr relay as the source of truth. A DiVine-operated PDS (Personal Data Server) subscribes to the funnelcake Nostr relay, reads cryptographically signed NIP-71 video events, translates them into `app.bsky.embed.video` records, and writes them to the user's ATProto repository. This means:

- The PDS is a **consumer** of the Nostr event stream, not in the critical posting path
- Authorization comes from **Nostr event signatures** — the user signs once, and DiVine's PDS publishes to ATProto on their behalf
- If the PDS goes down, Nostr-side publishing is unaffected; the PDS catches up by replaying missed events
- No third-party bridge dependency — DiVine controls the entire pipeline

## Implementation Foundation: Blacksky's rsky

The PDS implementation should be based on or heavily informed by [blacksky-algorithms/rsky](https://github.com/blacksky-algorithms/rsky), a Rust-based AT Protocol implementation that includes:

- **rsky-pds**: A Rust PDS using PostgreSQL (not SQLite) and S3-compatible blob storage (not on-disk). This aligns with DiVine's existing infrastructure choices.
- **rsky-relay**: A Rust relay/firehose provider
- **rsky-feedgen**: Custom algorithmic feed generator

Blacksky also operates a custom AppView (api.blacksky.community) with a Rust indexer (rsky-wintermute) replacing the TypeScript firehose consumer, custom community lexicons, and a video upload service with its own DID. All code is open source (MIT/Apache 2.0 dual-licensed).

Verse/andOtherStuff has an existing relationship with Rudy Fraser (Blacksky's creator), who has appeared on the Revolution.social podcast twice.

## Research Tasks

Please produce a comprehensive technical specification and product plan covering the following areas. For each section, provide concrete implementation details, not just high-level descriptions.

---

### 1. Identity Architecture

Design the identity binding between Nostr and ATProto for DiVine users.

- **DID strategy**: Should DiVine use `did:plc` or `did:web` for its users? Consider: account portability/migration, dependency on Bluesky's PLC directory, operational complexity, and the fact that DiVine already serves NIP-05 identifiers (user@divine.video → Nostr pubkey). Recommend one approach with rationale.
- **Handle resolution**: How DiVine users get ATProto handles (e.g., `username.divine.video`). Detail the DNS and .well-known configuration required.
- **Key management**: DiVine users have Nostr secp256k1 keypairs. ATProto uses different signing keys for repo operations. Design the key management model — does DiVine hold custodial ATProto signing keys on behalf of users? How are these generated, stored, and rotated?
- **Identity linking**: How to store and expose the mapping between a user's Nostr pubkey and their ATProto DID. Should the Nostr pubkey be stored in the DID document? Should the ATProto DID be published in a Nostr event (kind 0 metadata, or a new kind)?
- **User consent flow**: What does the user experience look like? Do users opt-in to ATProto cross-posting? Is it on by default? What controls do they have?

### 2. PDS Architecture & Implementation

Detail the PDS service design based on rsky-pds.

- **Deployment model**: Single multi-tenant PDS at `pds.divine.video` hosting all DiVine user repos. Describe the infrastructure: where it runs, how it scales, resource requirements based on Blacksky's operational experience.
- **Nostr relay subscription**: How the PDS subscribes to funnelcake relay's event stream. Filtering strategy (subscribe to NIP-71 video events only? All events from registered users?). Handling of reconnection, backfill, and catch-up after downtime.
- **Event translation pipeline**: Detailed mapping from NIP-71/NIP-92 Nostr events to `app.bsky.embed.video` records. Cover:
  - Video metadata mapping (title/description → post text, hashtags → facets, etc.)
  - Aspect ratio and duration handling (DiVine is 6-second loops; ATProto video embeds support aspectRatio)
  - Thumbnail/poster handling
  - Caption/subtitle support (NIP-71 vs ATProto caption tracks)
  - How to handle DiVine-specific metadata that has no ATProto equivalent
- **Blob management**: The critical question of video file storage.
  - Option A: PDS fetches video from Blossom by hash, re-uploads as an ATProto blob to its own S3 storage
  - Option B: PDS registers the Blossom-hosted file directly as an ATProto blob (is this possible within the spec?)
  - Option C: Shared S3 backend where both Blossom and rsky-pds reference the same underlying object
  - Evaluate each option for spec compliance, storage costs, latency, and operational complexity
- **Repo management**: How user repositories are created, how records are written, MST (Merkle Search Tree) maintenance, and sync with the ATProto relay network via `com.atproto.sync.*` endpoints.
- **Deletion and moderation**: When a user deletes a video on DiVine (NIP-09 deletion event on Nostr), how does that propagate to ATProto? When ATProto labelers flag content, how does that feed back to DiVine's moderation system?

### 3. Video Format & Protocol Mapping

Provide a detailed technical mapping between the two protocol representations of the same video.

- **NIP-71 event structure** → **app.bsky.embed.video record**: Field-by-field mapping table
- **NIP-71 event structure** → **app.bsky.feed.post record**: How the containing post record is constructed (text, facets for hashtags/mentions, langs, labels, createdAt)
- **Blossom blob** → **ATProto blob**: Content hash (SHA-256 in Blossom vs CID in ATProto), MIME types, size limits (ATProto video limit is 100MB; DiVine's 6-second loops are well under this)
- **Engagement mapping**: Likes, reposts, replies — if a Bluesky user likes a DiVine video via ATProto, should that propagate back to Nostr? Design the bidirectional engagement sync or justify why it should be one-directional.
- **Profile sync**: Should the user's Nostr profile (kind 0) be synced to their ATProto `app.bsky.actor.profile` record? What fields map (display name, bio, avatar, banner)?

### 4. Feed & Discovery Integration

How DiVine content appears in ATProto discovery mechanisms.

- **Feed generators**: Should DiVine operate an ATProto feed generator (like rsky-feedgen) that surfaces DiVine content? E.g., a "DiVine Trending" feed that Bluesky/Skylight users can subscribe to. How does this integrate with DiVine's existing Gorse recommendation engine?
- **Skylight compatibility**: Skylight uses standard `app.bsky.embed.video` in `app.bsky.feed.post` — confirm that DiVine's ATProto output will render correctly in Skylight's UI. Any special considerations for 6-second looping behavior?
- **Custom lexicon vs standard**: Should DiVine define any custom lexicons (e.g., `video.divine.*`) for metadata that doesn't fit in `app.bsky.embed.video`, or strictly use standard Bluesky lexicons for maximum compatibility? Consider the Lexicon.community standardization effort.
- **RSS/Podcasting 2.0 feeds**: DiVine already builds RSS feed endpoints via funnelcake. How does the ATProto integration interact with this? Could the PDS also serve as a source for RSS generation?

### 5. Moderation & Trust and Safety

How moderation works across both networks.

- **DiVine's existing moderation**: AI-powered classification service with rich content labels. How do these map to ATProto's labeler system? Can DiVine operate as an ATProto labeler service?
- **Incoming ATProto moderation**: When Bluesky's Ozone or other labelers flag DiVine content, how does the PDS handle takedown requests? How do these flow back to the Nostr side?
- **NSFW and content warnings**: Mapping between Nostr content warning tags and ATProto self-labels
- **Account-level moderation**: Handling bans, suspensions, and content removal across both protocols
- **Legal compliance**: DMCA, right to be forgotten, geographic content restrictions — how these work when content exists on both Nostr (where deletion is advisory) and ATProto (where the PDS can enforce deletion)

### 6. Product Roadmap & Phasing

Break the implementation into phases with clear milestones.

- **Phase 1 — MVP**: What's the minimum to get DiVine videos appearing on ATProto? Likely: identity setup, basic event translation, blob handling, repo sync. No engagement sync, no feed generators, no bidirectional anything.
- **Phase 2 — Discovery**: Feed generators, profile sync, better metadata mapping, custom feeds on Skylight
- **Phase 3 — Engagement**: Bidirectional likes/replies (if justified), ATProto labeler integration, advanced moderation sync
- **Phase 4 — Advanced**: Custom lexicons, collaborative features, potential for ATProto-native features that don't exist on Nostr

For each phase, estimate:
- Engineering effort (person-weeks, assuming Rust-proficient team familiar with both protocols)
- Infrastructure costs (additional compute, storage, bandwidth)
- Dependencies and risks
- Success metrics

### 7. Competitive & Strategic Analysis

- **Skylight comparison**: Skylight uses Bluesky's lexicons directly and depends on Bluesky's infrastructure. DiVine would be dual-native (Nostr + ATProto). What are the strategic advantages and disadvantages of each approach?
- **Reelo comparison**: Reelo is building custom ATProto lexicons for video. How does DiVine's approach compare?
- **Bridge vs native**: Why is running a DiVine PDS better than relying on Bridgy Fed / Eclipse for cross-posting? What are the tradeoffs?
- **Network effects**: With ~380K Skylight users and 42M+ Bluesky users, what's the realistic addressable audience for DiVine content on ATProto? How does this compare to DiVine's Nostr-side audience?
- **studio.coop implications**: How does this ATProto integration fit into the broader studio.coop cooperative platform model?

### 8. Open Questions & Risks

Identify and discuss:
- **ATProto spec stability**: How stable are the video-related lexicons? Risk of breaking changes?
- **PDS operational burden**: Based on Blacksky's experience, what are the real-world challenges of running an independent PDS?
- **Blob storage costs**: Video is expensive. What are the cost implications of potentially duplicating video storage across Blossom and ATProto blob stores?
- **Rate limits and quotas**: ATProto relay network rate limits, PDS federation requirements, any restrictions on automated posting
- **Legal entity**: Which entity operates the PDS — Verse Communications, andOtherStuff (the nonprofit), or studio.coop?
- **Nostr event types beyond video**: If this works well for NIP-71 video events, could the same PDS translate other Nostr event types (kind 1 text notes, kind 30023 long-form content) to ATProto records? Should it?

---

## Output Format

Please produce:

1. **Technical Specification Document** — Detailed enough that a Rust engineer familiar with both Nostr and ATProto could begin implementation. Include data models, API flows, sequence diagrams, and configuration examples.
2. **Product Plan** — Phased roadmap with milestones, effort estimates, dependencies, and success metrics.
3. **Architecture Diagram Description** — Describe (in enough detail to render as a diagram) the full system architecture showing: DiVine mobile app → Nostr signing → funnelcake relay → PDS translation service → ATProto relay network → Skylight/Bluesky/etc., with all data flows and storage layers labeled.

## Additional Context

- DiVine is built by a small team; the solution should minimize operational overhead
- The PDS should be "set and forget" as much as possible — not a service that needs constant babysitting
- Verse/andOtherStuff is funded by Jack Dorsey, who has strong views on protocol-level decentralization — the architecture should be philosophically coherent with the idea that Nostr is the primary protocol and ATProto is a distribution channel
- DiVine's 6-second loop format is a deliberate product choice (like Vine) — the ATProto representation should preserve this identity even though ATProto supports up to 3-minute videos
- The Nostr signature verification on the PDS side is a key innovation — document it thoroughly as it could be a pattern other Nostr projects adopt for ATProto interoperability
