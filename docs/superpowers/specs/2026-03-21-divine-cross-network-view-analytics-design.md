# DiVine Cross-Network View Analytics Design

**Date:** 2026-03-21
**Status:** Approved

## Purpose

Add creator-visible view analytics for DiVine short-form video across DiVine apps, mirrored ATProto posts, and partner playback surfaces without pretending third-party reads are exact. The system should keep creator-facing numbers growing, document how they are calculated, and preserve enough raw evidence to change formulas later without losing history.

## Current Constraints

- DiVine already treats Nostr as the write path of record and ATProto as a derived distribution path.
- The bridge already persists Nostr-to-AT mappings and media lineage through `record_mappings` and `asset_manifest`.
- The active media path fetches from Blossom, verifies the source hash, uploads to the PDS, and publishes `app.bsky.feed.post` plus `app.bsky.embed.video`.
- ATProto and Bluesky do not currently provide a protocol-level, cross-client, canonical video view counter that DiVine can simply subscribe to.
- Current product direction already treats AT engagement as analytics-only rather than projecting likes, reposts, or replies back into Nostr.

## Goals

- Show creators two stable public numbers: `Verified Plays` and `Estimated Reach`.
- Keep three internal counters: `verified_plays`, `cdn_deliveries`, and `reported_external_views`.
- Count only eligible short-form video with canonical measured duration `<= 6300 ms`.
- Support optimistic counting while keeping the methodology explicit and defensible.
- Attribute playback to apps when possible, with confidence levels instead of fake precision.
- Keep the mirroring path and analytics path operationally separate.

## Non-Goals

- Exact unique-human counting across all third-party AT clients.
- Minting synthetic Nostr like, repost, or reply events from AT analytics.
- Letting imported or remote accounts masquerade as native `username.divine.video` identities.
- Supporting long-form video in the same pipeline before the short-form gate is stable.

## Core Decisions

### 1. Public numbers are derived from three internal counters

Internally, DiVine stores:

- `verified_plays`
- `cdn_deliveries`
- `reported_external_views`

Creators see:

- `Verified Plays`
- `Estimated Reach`

`Estimated Reach` is the deduped union of the three internal sources with published caveats.

### 2. The view story is intentionally optimistic but documented

Most creators want the number to move. The system should therefore use light qualification and light dedupe rather than the strictest possible interpretation. The product must still publish a methodology note that explains:

- what counts as a verified play
- what counts as a delivery
- what counts as an external report
- that `Estimated Reach` is an estimate, not a literal count of unique people

### 3. Only videos `<= 6.3` seconds are eligible

This is a hard product invariant for now.

- Every candidate video must be probed from actual media bytes.
- The authoritative field is `canonical_duration_ms`.
- Signed event metadata may be stored as `declared_duration_ms`, but it does not determine eligibility.
- Anything over `6300 ms` is skipped before PDS upload and excluded from analytics.

### 4. The duration gate belongs in the bridge publish path

The current bridge already fetches source media before upload. That is the right place to measure duration and enforce the short-form contract. Analytics should inherit the bridge eligibility result rather than re-deciding it later.

### 5. The analytics pipeline is separate from the publish path

Mirroring must not depend on CDN log latency, external report delivery, or rollup freshness. The analytics system is a separate read-model pipeline fed by:

- first-party DiVine play events
- DiVine Blossom or CDN delivery logs
- external voluntary view reports

### 6. An explicit record-to-asset join is required

Today, `record_mappings` and `asset_manifest` are both persisted, but there is no durable explicit join from a mirrored post to the canonical source asset. The analytics system needs a dedicated join record so a CDN delivery can be resolved back to the mirrored Nostr and AT post pair without guessing.

### 7. App attribution is confidence-scored

App attribution should be tracked with confidence, not all-or-nothing trust.

- `trusted_first_party`
- `trusted_partner`
- `inferred`
- `unknown`

Low-confidence observations can still count toward `Estimated Reach` if they pass eligibility and dedupe rules.

### 8. Imported account naming remains explicit

Imported or bridge-created identities must use an explicit alias convention rather than claiming the native DiVine namespace. The active assumption is:

- native DiVine-hosted identities: `username.divine.video`
- imported or bridge-created NIP-05 aliases: `username-domain.bluesky.name`

Analytics keys must be based on canonical DIDs, pubkeys, record URIs, and source hashes, never on alias text.

## Architecture

### Bridge path

`divine-atbridge` remains the source-of-truth publish worker.

For each eligible NIP-71 video:

1. verify the Nostr signature
2. resolve the linked account and opt-in state
3. fetch bytes from Blossom
4. measure duration from the fetched media
5. reject media whose canonical duration exceeds `6300 ms`
6. upload the verified eligible blob to the PDS
7. publish the AT record
8. persist record mapping, media facts, and explicit record-to-asset linkage

### Analytics path

A dedicated analytics service ingests three raw evidence streams:

1. first-party DiVine playback events
2. CDN or Blossom delivery batches
3. external partner or future protocol view reports

It stores raw normalized observations, then recomputes rollups for creator-facing numbers.

### Read model

The read model produces:

- per-video totals
- per-creator totals
- time-bucketed trends
- app-attribution breakdowns
- public numbers: `Verified Plays` and `Estimated Reach`

The read model is fully recomputable from raw observation tables.

## Data Model

### Extend `asset_manifest`

Store media facts required by both publishing and analytics:

- `declared_duration_ms`
- `measured_duration_ms`
- `canonical_duration_ms`
- `probe_status`
- `probe_error`
- `eligibility_state`
- `eligible_for_mirroring`

Because `asset_manifest` is keyed by `source_sha256`, these facts stay stable across reposts or replays of the same source asset.

### Add `record_asset_links`

This new table joins a published record to its canonical media asset:

- `nostr_event_id`
- `at_uri`
- `source_sha256`
- `blossom_url`
- `at_blob_cid`
- `asset_role`
- `created_at`

For this feature, the primary role is `primary_video`.

### Add `view_observations`

Immutable raw evidence rows with fields such as:

- `id`
- `video_id`
- `nostr_event_id`
- `at_uri`
- `source_sha256`
- `observation_type`
- `observed_at`
- `viewer_fingerprint`
- `network`
- `app_id`
- `confidence`
- `source`
- `payload_json`
- `accepted`
- `rejection_reason`

### Add `video_view_rollups`

Bucketed aggregates keyed by canonical video plus time window:

- `video_id`
- `bucket_start`
- `verified_plays`
- `cdn_deliveries`
- `reported_external_views`
- `estimated_reach`
- `updated_at`

### Add `app_registry`

Registry rows for known first-party and partner reporters:

- `app_id`
- `network`
- `trust_level`
- `secret_ref` or verification key reference
- `display_name`
- `status`

## Counting Rules

### Verified Plays

Incremented only by first-party DiVine play events on eligible videos.

The first shipped contract should store both:

- `play_started`
- `play_qualified`

Only `play_qualified` increments `verified_plays`.

### CDN Deliveries

Incremented from DiVine-controlled media delivery evidence after light dedupe:

- one counted delivery per `video_id + viewer_fingerprint + app classification + time window`
- retries, preloads, and obvious duplicate edge fetches within the window collapse to one

### Reported External Views

Incremented from accepted external reports after:

- authentication or signature verification
- replay protection
- dedupe in the same viewer and time window

### Estimated Reach

Computed as a union:

- all qualified first-party plays
- plus deduped CDN deliveries not already matched to a first-party play in the same viewer bucket
- plus deduped external reports not already matched to either source

This should bias toward growth, but not so aggressively that obvious retry storms inflate numbers.

## Trust Model

### Trusted first-party

Events from DiVine-controlled apps and websites with stable app identifiers.

### Trusted partner

Events from approved partner apps using a DiVine-issued signing secret or key.

### Inferred

Events or deliveries where the app identity is guessed from:

- user agent
- referrer
- request path
- CDN metadata

### Unknown

Accepted observations that cannot be attributed with useful confidence.

## Failure Policy

### Media fetch failure

- skip publishing
- store failure state for operator visibility
- do not create analytics eligibility

### Media probe failure

- mark `probe_status = failed`
- store the error
- skip mirroring and analytics eligibility

### Unsupported duration

- mark `eligibility_state = skipped_unsupported_duration`
- do not upload to the PDS
- do not include in analytics rollups

### Unresolvable delivery evidence

- keep the raw observation for debugging
- do not count it until it can be joined to an eligible video

### Rejected external reports

- store the raw observation and rejection reason
- do not count it

## Testing

Add focused tests for:

- sub-6.3-second video mirrors successfully
- exactly 6.3-second video mirrors successfully
- longer video is skipped before upload
- actual media probe overrides declared metadata
- record-to-asset linkage is persisted for each published short video
- first-party plays increment only `verified_plays`
- CDN duplicate fetches collapse correctly
- external reports dedupe correctly
- `Estimated Reach` suppresses overlaps across evidence sources
- low-confidence observations count while still surfacing weak attribution

## Rollout

### Phase 1

- persist media facts and raw observations
- no creator-facing counters yet

### Phase 2

- backfill rollups
- compare formulas on internal dashboards

### Phase 3

- expose `Verified Plays` and `Estimated Reach`
- publish a methodology page

### Phase 4

- add signed partner reporting
- explore a future AT or Nostr view-event proposal as an additional ingest source

## References

- ATProto custom feed and interaction surfaces: <https://docs.bsky.app/docs/starter-templates/custom-feeds>
- ATProto blob lifecycle guidance: <https://atproto.com/guides/blob-lifecycle>
- ATProto video handling guidance: <https://atproto.com/guides/video-handling>
- Bluesky and ATProto feed lexicons: <https://raw.githubusercontent.com/bluesky-social/atproto/main/lexicons/app/bsky/feed/defs.json>
- Feed interaction lexicon: <https://raw.githubusercontent.com/bluesky-social/atproto/main/lexicons/app/bsky/feed/sendInteractions.json>
- Mostr aliasing examples: <https://soapbox.pub/blog/mostr-fediverse-nostr-bridge/> and <https://soapbox.pub/blog/follow-threads/>
