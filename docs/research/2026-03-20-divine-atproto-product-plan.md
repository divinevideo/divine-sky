# DiVine ATProto Integration Product Plan

> Status: supporting research document. The canonical source of truth is `docs/plans/2026-03-20-divine-atproto-unified-plan.md`.

## Product Thesis

DiVine should treat ATProto as a distribution network, not as its source of truth. The product win is reach: creators keep the Nostr-native DiVine workflow, while their clips become legible to Bluesky, Skylight, Flashes, and related clients with minimal extra ceremony.

All effort and infrastructure figures below are planning estimates derived from current public ATProto and `rsky` materials, not vendor quotes or published Blacksky operating numbers.

## Strategic Position

### Why a native PDS is better than a bridge

Running a DiVine PDS is heavier than relying on a third-party bridge, but it gives DiVine control over:

- identity and handle policy
- moderation and takedown flow
- replay and recovery from relay outages
- media lifecycle and storage economics
- future product features like custom feeds, provenance records, or creator analytics

Third-party bridges are good for experimentation. They are weak foundations for a video product with moderation, licensing, and recovery requirements.

### Competitive read

- Bluesky and Skylight apps that only target `app.bsky.*` gain instant compatibility but inherit Bluesky's product envelope.
- Reelo, rebranded to Spark in TechCrunch's January 28, 2025 reporting, demonstrates the opposite strategy: own backend, own lexicon, and more product freedom, but weaker default interoperability.
- DiVine's advantage is dual-nativity: keep Nostr semantics and distribution independence while exporting a standard ATProto representation for reach.

### Network effects

- Bluesky's FAQ says the network had over 42 million users as of February 2026.
- TechCrunch reported on January 26, 2026 that Skylight had passed 380,000 users.

Those are account and app-scale numbers, not guaranteed active audience. The realistic near-term target is not "all of Bluesky." It is:

- DiVine creators getting meaningful incremental impressions off standard AT discovery
- a subset of video-heavy AT clients adopting DiVine feeds
- DiVine clips becoming linkable and followable anywhere ATProto records are understood

## Phased Roadmap

### Phase 1: MVP distribution

Scope:

- account linking and `did:plc` provisioning
- bridge consumer for NIP-71, NIP-92, NIP-09, and kind `0`
- blob fetch plus AT upload
- standard `app.bsky.feed.post` with `app.bsky.embed.video`
- basic delete propagation
- one-way profile sync

Effort:

- 8 to 12 person-weeks for a Rust-capable team familiar with Nostr and ATProto

Infrastructure:

- $1,000 to $3,000 per month to start, excluding heavy video egress spikes
- biggest cost drivers are object storage, bandwidth, and moderation labor

Success metrics:

- 95 percent or better mirrored-publish success within 2 minutes
- under 0.5 percent replay or duplicate-write errors
- at least 25 percent of active DiVine creators opt in within the first launch cohort

Dependencies and risks:

- stable account-linking UX
- blob processing reliability
- moderation policy alignment across protocols

### Phase 2: discovery and ranking

Scope:

- DiVine-operated custom feed generator
- Gorse-backed ranking for AT feed skeletons
- richer caption and hashtag handling
- analytics for AT reach and watch-through

Effort:

- 4 to 6 person-weeks

Infrastructure:

- additional $500 to $1,500 per month for feed and indexing services

Success metrics:

- at least 10,000 feed subscriptions across DiVine-owned AT feeds
- measurable watch-through lift versus generic Bluesky distribution

Dependencies and risks:

- AppView/feed discoverability
- ranking quality on short-loop content

### Phase 3: moderation and engagement intelligence

Scope:

- DiVine labeler service
- moderation queue for inbound AT labels and reports
- analytics-only ingest of AT likes, reposts, and replies
- creator dashboards showing AT reach and engagement

Effort:

- 6 to 10 person-weeks

Infrastructure:

- additional $1,000 to $2,000 per month, mostly moderation and indexing

Success metrics:

- human review SLA under 24 hours for cross-network moderation cases
- creator dashboard usage by at least 40 percent of mirrored accounts

Dependencies and risks:

- abuse tooling
- label taxonomy drift between DiVine and Bluesky clients

### Phase 4: advanced protocol product

Scope:

- optional `video.divine.*` lexicons for provenance or richer clip metadata
- experimental loop-aware presentation features
- selective bidirectional interaction models if user-signed delegation becomes viable
- reuse of the bridge pattern for other Nostr kinds such as text notes or long-form posts

Effort:

- 8 to 12 person-weeks per feature cluster

Infrastructure:

- variable; largely driven by media transforms and indexing volume

Success metrics:

- partner clients or DiVine-owned AT experiences actually consuming the extended metadata
- reduced storage cost per mirrored clip if shared object storage lands

Dependencies and risks:

- standards maturity
- ecosystem appetite for custom lexicons

## Moderation and Legal Operations

Recommendation:

- Verse Communications runs day-to-day operations and on-call ownership.
- Governance policy and contributor accountability can sit under the broader studio.coop or andOtherStuff umbrella if that better matches the project's public values.

Legal realities:

- AT-side deletes are enforceable on DiVine infrastructure.
- Nostr-side deletes are advisory once relayed.
- DMCA, regional restrictions, and right-to-be-forgotten requests need a unified case-management system even if final enforcement differs by protocol.

## Cost Model

The dominant costs are not CPU. They are:

- duplicated or derived video storage
- egress from object storage and CDN
- moderation review
- feed and analytics indexing at scale

Cost minimization tactics:

- keep ATProto mirroring opt-in at first
- dedupe source blobs in shared storage where feasible
- use standard `app.bsky.*` records before funding custom-protocol work

## Top Risks

1. `rsky` is promising but still publicly marked as pre-1.0 and subject to change.
2. Video preprocessing and PDS-hosted blobs can become the cost center quickly.
3. Handle and PDS hostname choices are sticky; a sloppy launch creates migration debt.
4. Cross-network moderation will fail noisily if the case-management path is underbuilt.
5. Bidirectional engagement sync is tempting and likely premature.

## Recommended Execution Order

1. Build and test the one-way publish bridge.
2. Prove reliability and cost on a small creator cohort.
3. Add discovery surfaces and feed ranking.
4. Add moderation and analytics depth.
5. Only then consider public custom lexicons or bidirectional interactions.

## Sources

- Bluesky FAQ: https://bsky.social/about/faq
- TechCrunch on Skylight growth, January 26 2026: https://techcrunch.com/2026/01/26/tiktok-alternative-skylight-soars-to-380k-users-after-tiktok-u-s-deal-finalized/
- TechCrunch on Reelo/Spark, January 28 2025: https://techcrunch.com/2025/01/28/reelo-stands-out-among-the-apps-building-a-tiktok-for-bluesky/
- `rsky` repository README: https://github.com/blacksky-algorithms/rsky
- ATProto production guidance: https://atproto.com/guides/going-to-production
