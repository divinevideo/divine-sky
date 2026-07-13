# Lossless Divine-to-ATProto Crosspost Reconciliation RFC

**Date:** 2026-07-12
**Status:** Execution authorized by product owner after design-gate escalation;
remaining gate findings are mandatory implementation prerequisites below

## Problem

`divine-atbridge` currently behaves like a best-effort mirror. Production
accepted new Divine posts while ATProto publication repeatedly failed, yet the
bridge remained healthy and replay could not revive terminalized jobs. Separate
cursor, page-limit, retry, authentication, archive-limit, and observability
failures can each make a post disappear permanently.

Deploying proactive session refresh repairs the current authentication incident.
It does not establish the product guarantee.

## Core Principle

The bridge is a reconciliation system, not a streaming system. Streams provide
latency; reconciliation provides correctness. Every canonical source fact
remains discoverable until the currently effective event is translated or an
explicit, durable policy outcome makes it ineligible.

The authoritative chain is:

```text
verified Nostr source facts
        -> resolved source/eligibility state
        -> durably reserved AT record identity
```

`publish_jobs` is a rebuildable execution queue. It is never authoritative
evidence that an event was published, rejected, deleted, or superseded.

## Product Contract

When a Divine user connects ATProto distribution:

- connection completes immediately and archive import begins automatically;
- the account exposes importing, caught-up, or attention-required state to
  authorized support tooling;
- every eligible, currently effective video in the user's retained Divine
  archive imports asynchronously;
- AT feed records use the original Nostr `created_at` as the record's
  `createdAt`; AT repository commit and AppView index times remain later and are
  not backdated;
- every future eligible post eventually publishes without manual replay;
- deleted, superseded, disabled, opted-out, or moderation-blocked posts do not
  remain published;
- disabling or disconnecting crosspost fences publication immediately, then
  deletes existing bridge-created AT records and clears bridge credentials
  through the disconnect state machine below;
- recovery continues across bridge, database, PDS, media-service, and source
  API outages.

Archive completeness is bounded by the canonical source's retention contract
and the supported-media envelope. Funnelcake's existing `nostr.events_local`
table is the pre-deployment historical source: it stores one raw signed row per
event ID without a TTL. The new bridge table is an optimized read model backfilled
from that raw table, not a replacement source of truth. Live relay ingestion
dual-writes through the same verified-event path after cutover.

The retained boundary begins with the earliest row present in `events_local`;
Divine does not claim recovery for events never ingested there. The source
backfill re-verifies signatures/IDs and compares per-author/kind/day counts plus
deterministic ID digests between `events_local` and the new table.
Unverifiable rows are quarantined by ID and reported; any count/digest gap marks
the source build incomplete and blocks archive rollout/caught-up claims. The
bridge-source retention contract is indefinite except for lawful erasure, which
uses the explicit purge protocol below. If source retention changes, product and
SLO language must change first.

The initial supported-media envelope is a valid kind `34235`/`34236` event with
one canonical media reference on a configured Divine Blossom HTTPS origin, a
valid lowercase SHA-256 `x` tag matching fetched bytes, and a source MIME in the
reviewed transcoder allowlist (`video/mp4`, `video/quicktime`, or `video/webm`).
The target video service transcodes accepted input to its supported AT format.
Other otherwise-valid videos are typed `UNSUPPORTED_MEDIA`, visible as archive
exceptions, and become reconsiderable when `eligibility_policy_version` changes.

### Effective archive semantics

The archive mirrors the currently effective video for each NIP-33 coordinate,
not every historical edit as a separate Bluesky post:

- non-addressable events are keyed by Nostr event ID;
- addressable kind `34235`/`34236` events are grouped by
  `(pubkey, kind, d-tag)`;
- the greatest `(created_at, event_id)` is the effective revision unless a valid
  deletion targets the event or coordinate;
- older revisions remain durable source facts with state `superseded`; they are
  not published;
- all valid kind `5` `e` and `a` targets are processed, not only the first tag.

Archive discovery resolves the entire finite bounded traversal, including deletions and
supersession, before publication convergence begins. Historical content that was
already deleted therefore does not briefly appear during import.

## Authoritative Eligibility Predicate

Every discovery, retry, reconciliation, migration, metric, and SLO calculation
uses the same versioned `EligibilityDecision` function. A source event is
`eligible` exactly when all of these are true:

1. its ID is the SHA-256 of the canonical Nostr serialization and its Schnorr
   signature verifies;
2. its full pubkey equals the connected account's verified Nostr pubkey;
3. its kind is supported and its payload, tags, timestamps, and media references
   pass bounded protocol validation;
4. the account exists, is provisioned, enabled, and has crosspost opted in;
5. it is the currently effective revision for its addressable coordinate;
6. no valid same-author deletion targets its event ID or coordinate;
7. moderation does not block publication;
8. its media passes immutable origin, type, size, hash, and safety policy.

The result stores `eligibility_policy_version`, a stable machine reason code,
the account lifecycle epoch, and a per-event/coordinate desired-state version.

Outcomes are:

- `eligible`: publication must converge;
- `retryable`: eligibility or publication could not yet be determined or
  completed;
- `rejected`: invalid or intentionally unsupported under a durable policy;
- `cancelled`: a once-actionable event became ineligible through deletion,
  supersession, account disablement, opt-out, or moderation;
- `needs_review`: legacy or unknown state cannot be safely classified.

Rejected events are reconsidered only when their stored policy version is older
than the current translator policy. Unknown internal/upstream failures default
to retryable; ambiguous legacy data defaults to `needs_review`, never silent
rejection.

Stable initial reason codes are:

```text
retryable: AUTH_EXPIRED, AUTH_REFRESH_FAILED, NETWORK, TIMEOUT, RATE_LIMITED,
           UPSTREAM_5XX, PDS_UNAVAILABLE, MEDIA_PROCESSING, UNKNOWN_RETRYABLE
rejected:  INVALID_EVENT_ID, INVALID_SIGNATURE, INVALID_PAYLOAD,
           UNSUPPORTED_KIND, UNSUPPORTED_MEDIA, INVALID_MEDIA,
           POLICY_REJECTED
cancelled: DELETED, SUPERSEDED, ACCOUNT_DISABLED, OPTED_OUT,
           MODERATION_BLOCKED
review:    LEGACY_AMBIGUOUS
```

Diagnostics are bounded, sanitized fields separate from reason codes. They may
not contain JWTs, authorization headers, signed service-auth URLs, full payloads,
or user media URLs.

## Reliability Invariants

1. Every verified source fact remains queryable from the canonical append-only
   source and is eventually compared with bridge state.
2. Every eligible effective event has exactly one durably reserved AT record or a
   durable non-terminal retry.
3. A source checkpoint advances only in the same PostgreSQL transaction that
   persists every source fact, deletion edge, resolution change, and derived
   work in that page.
4. Restarts may duplicate discovery and execution but cannot duplicate AT
   records.
5. A deletion, supersession, disablement, opt-out, or moderation change cannot
   be followed by a lasting resurrected record.
6. Attempt counts are diagnostic and never make retryable work invisible.
7. Queue loss is recoverable from authoritative source and mapping state.
8. Health and SLOs measure eligible-event convergence, not only process uptime.

## Canonical Funnelcake Source

The existing public `/api/videos/events` endpoint is a filtered product view and
is not used for correctness. Funnelcake adds an authenticated, bridge-only API
over a dedicated append-only read model of verified events.

### ClickHouse workload and read model

The workload is mixed OLAP with narrow per-author history scans: equality on
author, a small kind set, then a composite time/ID range. It also needs exact
event-ID lookup for audits. Ingestion is append-only and late arrival is normal;
deletion and replacement are separate facts, not ClickHouse mutations.

The additive `nostr.bridge_source_events_v1` table contains typed columns for
`pubkey FixedString(64)`, `kind UInt32`, `created_at DateTime`,
`event_id FixedString(64)`, verified signed payload, `source_indexed_at`, a
monotonic per-copy `ingest_version`, and verification version. It uses
`ReplacingMergeTree(ingest_version)` and:

```sql
ORDER BY (pubkey, kind, created_at, event_id)
```

No user-based partitioning is allowed. Start without partitioning unless an
explicit indefinite-retention lifecycle plan justifies bounded monthly
partitions. Known scalar fields use native non-nullable types; low-cardinality
categorical fields use `LowCardinality` only after cardinality validation.

This is query-driven and additive because ClickHouse `ORDER BY` is effectively
immutable. It follows `schema-pk-plan-before-creation`,
`schema-pk-prioritize-filters`, `schema-pk-filter-on-orderby`,
`schema-types-native-types`, and `schema-partition-low-cardinality`. Although the
general cardinality rule favors low-cardinality prefixes, author must lead here
because every correctness scan supplies exactly one author and that predicate
provides the dominant pruning. This is a workload-specific derived decision,
validated with `EXPLAIN indexes = 1` and representative production-shaped data.

Funnelcake ingestion appends every verified kind `34235`, `34236`, and `5`
event before public moderation/deletion filtering. Existing deduplicated video
snapshots are neither the source nor a backfill input. Exact event-ID audits use
the existing `relay_events_by_id` model; the new table receives a benchmarked
event-ID bloom index only if `EXPLAIN indexes = 1` demonstrates a need.

Cutover deliberately tolerates duplicate copies without losing writes:

1. create the target table;
2. create its materialized view from `events_local` for new relevant rows;
3. capture a finite backfill upper `indexed_at`/ID tuple;
4. copy raw rows through that upper in resumable batches;
5. verify per-author/kind/day ID counts and digests while the view remains live;
6. mark the source API ready only after verification succeeds.

`bridge_source_builds` durably stores build ID, captured raw upper tuple, last
raw ID checkpoint, rows copied/quarantined, source/target counts and digests,
verification version, completion time, and ready flag. Each copy batch and its
checkpoint are one acknowledged operation; replay is idempotent by event ID.

The API deduplicates every narrowed author/kind page by event ID using the latest
`ingest_version` (`argMax` or an equivalent tested query). MV/backfill overlap
and replay therefore return one response row even before background merges.
Conflicting bytes for one ID fail verification and quarantine the ID. Live
inserts are batched, or use `async_insert=1` with
`wait_for_async_insert=1`; unacknowledged insertion is forbidden.

The migration is additive/idempotent, uses one logical object per DDL statement,
does not use `ON CLUSTER`, `OPTIMIZE ... FINAL`, or multi-table rename, and uses
`SET alter_sync = 2` for metadata ALTERs and `SET mutations_sync = 2` for any
bounded materialization. This is consistent with the repository's ClickHouse
Cloud migration requirements.

Lawful vanish/erasure is the retention exception. Before payload purge,
Funnelcake sends a durable purge command containing affected IDs/coordinates to
the bridge; the bridge fences them, converges AT deletion, and acknowledges.
The bridge acknowledgement requires AT deletion status plus scrubbing PostgreSQL
source payload/prepared records, media caches/artifacts, operational exports, and
encrypted data keys. Backups use bounded expiry or cryptographic erasure; only a
minimal access-controlled anti-replay digest/tombstone remains. Funnelcake then
removes payload rows from `events_local`, `bridge_source_events_v1`, and every
derived raw/read model using its bounded purge workflow. If remote deletion
cannot complete, the purge is attention-required and follows the disconnect
hard-deadline policy. Adding the new table to `vanish.rs` and testing the full
bridge/Funnelcake/PDS cascade is a release gate.

### Versioned pagination protocol

Endpoint:

```text
GET /internal/v1/bridge/source-events
```

It requires service identity, an authorized bridge audience, a full lowercase
64-character author pubkey, `kinds` drawn from `{34235,34236,5}`, and `limit`
within `1..=500`. It returns complete signed events only after source
verification.

The first request omits `cursor`. The server determines the greatest available
`(created_at,event_id)` for the author/kinds and returns it inside an opaque,
signed/versioned `v1` cursor token. That tuple defines a finite **bounded
traversal**, not a ClickHouse MVCC snapshot. A row arriving later behind the page
cursor may be absent from this traversal; overlap and deep traversal must find
it before zero-loss/caught-up evidence is accepted.

The token binds a random scan ID, author, kinds, ascending direction, after
tuple, upper tuple, page-limit ceiling, issued time, expiry, and signing-key ID.
Parallel timestamp/ID fields are not accepted. Verification keys are retained
for at least token TTL plus renewal grace plus clock skew (minimum 55 days under
the defaults); signatures are checked in constant time. Earlier key retirement
returns `SOURCE_RESTART_REQUIRED`, not a renewable-expiry response.

Every page executes the exact ascending predicate:

```text
(created_at > after_created_at OR
 (created_at = after_created_at AND event_id > after_event_id))
AND
(created_at < upper_created_at OR
 (created_at = upper_created_at AND event_id <= upper_event_id))
ORDER BY created_at ASC, event_id ASC
LIMIT requested_limit + 1
```

For an initial scan, `after` is negative infinity. Continuation repeats author,
kinds, `limit`, and the returned cursor. `limit` may decrease but cannot exceed
the token ceiling. The response contains:

```text
events             at most requested_limit events
next_cursor        opaque v1 token, absent only at traversal completion
snapshot_upper     diagnostic tuple, constant for the scan
has_more           true iff the extra row exists within the upper bound
```

`next_cursor.after` is the last returned tuple. For an empty archive, `events`
is empty, `snapshot_upper` and `next_cursor` are absent, and `has_more=false`.
An empty non-initial page with `has_more=false` terminates. Empty plus
`has_more=true` is `SOURCE_PROTOCOL_ERROR` and retryable.

Tokens normally expire after 24 hours and are persisted with the bridge scan.
`POST /internal/v1/bridge/source-scans/{scan_id}/renew` accepts a valid expired
token within a 30-day renewal grace plus the last persisted token. It reissues
only the same author/kinds/direction/upper tuple and the signed token's after
tuple; callers cannot supply raw tuples. Beyond grace, the traversal restarts
idempotently and must still meet the archive completion bound.

Invalid service token is `401 SOURCE_UNAUTHENTICATED`; unauthorized identity is
`403 SOURCE_FORBIDDEN`; malformed/binding mismatch is
`400 SOURCE_CURSOR_INVALID`; normal expiry is `409 SOURCE_CURSOR_EXPIRED` with
`retryable=true`; rate limit is `429` with `Retry-After`; source failure is
`503`, retryable. JSON errors are
`{code,message,retryable,request_id}` with safe messages. Legacy public timestamp
cursors remain unchanged and are never mixed with the internal protocol.

The raw source is updated continuously rather than through the five-minute
public snapshot refresh/cache path, so it can support the five-minute live SLO.
API and ClickHouse integration tests prove the tuple predicates, token binding,
same-second behavior, late arrivals, cursor renewal, and finite upper bound.

## Divine-Sky Durable State

### Source facts and deletion edges

PostgreSQL stores immutable verified facts:

```text
source_events
  nostr_event_id PK
  nostr_pubkey
  kind
  event_created_at
  event_payload
  verification_version
  discovered_at

source_event_targets
  deletion_event_id FK -> source_events
  target_type        event | coordinate
  target_event_id    empty unless event target
  target_kind        0 unless coordinate target
  target_d_tag       empty unless coordinate target
  target_pubkey
  ownership_valid
  PK (deletion_event_id, target_type, target identity)
```

Complete payload columns are readable only by the atbridge workload DB role;
support APIs never return them. Database/ClickHouse backups containing payloads
remain inside the production trust boundary with the same encrypted retention
and lawful-erasure process. Logs, traces, metrics, and audit samples contain
full event IDs but no payload/media URL/session ciphertext.

An existing event ID with different bytes/pubkey is a security error and cannot
overwrite the stored fact. All `e` and syntactically valid `a` tags are expanded
into target rows. A deletion is effective only when its verified signer owns the
target event/coordinate. Unknown targets remain durable and are resolved when
the target later arrives.

An `e` target cancels that exact event regardless of arrival order. An `a`
target cancels same-author coordinate revisions with
`revision.created_at <= deletion.created_at`; a later signed revision recreates
the coordinate. For multiple coordinate deletions, each revision is cancelled
when any owned tombstone is at or after its timestamp. At equal timestamps the
deletion wins, independent of event-ID ordering. Late arrival recomputes the
coordinate from all revisions and tombstones, so observation order cannot change
the result. Every syntactically valid target is retained even when its kind is
not yet translatable, allowing later policy versions to resolve it.

Resolved state is separate from immutable facts:

```text
translation_states
  account_id
  nostr_event_id
  state
  reason_code
  eligibility_policy_version
  desired_state_version
  last_evaluated_at
  PK (account_id, nostr_event_id)
```

Funnelcake adds an authoritative append-only moderation log with a transactional
monotonic version allocator. Every current mutation path dual-writes before
acknowledgement. The log covers event, coordinate, pubkey ban/suspension,
privacy, tag policy, NSFW hard block, quarantine, and vanished-author scopes;
configuration distinguishes hard crosspost blocks from transferable labels.
Bootstrap snapshots all effective tables/sets into a finite version and proves
subject/action count and digest equality before ready.

`GET /internal/v1/bridge/moderation-decisions` uses the source workload identity
and exposes apply/clear decisions by version, action ID, scope, subject, reason,
and effective time. It reuses the full source cursor renewal/error contract with
`(moderation_version,action_id)` ordering. Same-version ties use action ID and
hard-block uncertainty fails closed. Divine-sky persists `moderation_states` plus a durable
checkpoint, increments every affected event/coordinate `desired_state_version`,
and deep reconciliation compares its checkpoint with Funnelcake. Missed
notifications therefore self-repair; block schedules deletion and unblock
re-evaluates eligibility under the current policy.

`record_mappings` has `UNIQUE(account_id, nostr_event_id)` and `UNIQUE(at_uri)`.
`publish_jobs` has `UNIQUE(account_id, nostr_event_id)`, references the state it
executes, and stores desired operation `publish | delete`, claimed
`desired_state_version`, and an optional blocking archive scan ID. It can be
regenerated from eligible states without a mapping. Converting a completed
publish into deletion increments the desired-state version and atomically
upserts the row as runnable `delete` work.

### Scan state and atomic page commits

Each account has separate discovery scans for `live`, `archive`,
`reconcile_incremental`, and `reconcile_deep`; live scans are also separated by
kind. A `source_scans` row contains scan ID/type, account, kind set, immutable
upper tuple, resumable after tuple, lease owner/expiry, attempt, next retry,
per-run counters, and status.

`account_sync_state` contains:

```text
discovery_state       pending | scanning | caught_up | attention_required
convergence_state     pending | publishing | caught_up | attention_required
last_scanned_created_at / last_scanned_event_id
last_published_created_at / last_published_event_id (diagnostic, not a watermark)
last_reconciled_at
lifetime_scan/publish counters
last safe reason code and bounded diagnostic
account_lifecycle_epoch
```

State transitions are derived from persisted counts, never from the greatest
published tuple:

- discovery is `pending` before a scan exists, `scanning` while a valid scan is
  incomplete, and `caught_up` only after its upper-bound CAS plus a subsequent
  overlap pass finds no behind-cursor arrivals;
- convergence is `pending` before archive release, `publishing` while any
  snapshot-scoped eligible item lacks a mapping or any publish/delete work is
  retryable, and `caught_up` only when eligible-unmapped, runnable publish, and
  runnable delete counts are all zero;
- either dimension becomes `attention_required` for `needs_review`, source
  digest gaps, an expired scan beyond renewal grace, cleanup-incomplete state,
  or retryable work older than the configured operator threshold (default 30
  minutes live, 24 hours archive); a successful scan/repair plus zero remaining
  triggering rows clears it;
- rejected and cancelled rows are durable completed outcomes and do not prevent
  caught-up. Rejected rows produce `caught_up_with_exceptions` in the combined
  support status; cancelled rows do not;
- combined status priority is `disconnecting/cleanup_incomplete`, then
  `attention_required`, then `importing` if either dimension is not caught up,
  then `caught_up_with_exceptions`, else `caught_up`;
- an empty archive reaches both caught-up states with zero totals.

Support progress includes snapshot/scan ID and discovered, effective, eligible,
mapped, retryable, rejected, cancelled, needs-review, deletion-pending, and
remaining counts, separated into current-run and lifetime totals.

For each source page, one PostgreSQL transaction:

1. locks the scan row and verifies lease/account lifecycle epoch;
2. inserts immutable `source_events` and every deletion target;
3. recomputes affected coordinate, tombstone, moderation, and eligibility
   states, incrementing only affected desired-state versions;
4. inserts/cancels derived jobs idempotently; every publish claim path checks the
   account archive-epoch gate, while compensating deletes remain claimable;
5. records shadow differences when in shadow mode;
6. updates the scan's after tuple and per-run counters;
7. commits.

No source mutation, job, or cursor becomes visible alone. A crash before commit
replays the page; a crash after commit resumes after it. Reaching the captured
upper tuple promotes the account high-water in a separate compare-and-swap that
requires the same scan ID, upper tuple, completed status, and account lifecycle
epoch. Archive completion does not open the account gate. It first runs a
settlement overlap traversal after the indexing-delay window, then a full deep
proof traversal for initial import, each including the required moderation
checkpoint. Only a CAS over lifecycle epoch, archive epoch, source build,
moderation upper version, and zero unresolved behind-cursor facts opens
publication for every path. Late deletion therefore cannot race archive publish.

Fault injection covers a crash after every numbered write and before/after both
transaction commits. In the page `A,B,C` case, a crash after deriving work for
`B` but before page commit rolls everything back; restart safely replays `A,B,C`.

### Discovery modes

- **Live:** continuously scans each video kind from a configurable overlap
  before its high-water. Default overlap is five minutes, chosen to exceed
  expected indexing delay. A work budget may yield only by persisting the scan
  cursor; it cannot promote high-water early.
- **Archive:** first scans all video and deletion facts oldest-first to a finite
  upper tuple, resolves current state, then starts bounded publication. It has no
  event-count or page-count cap.
- **Incremental reconciliation:** defaults to a configurable 24-hour overlap and
  compares source, state, mappings, tombstones, and jobs.
- **Deep reconciliation:** scans the full retained history on a slower cadence
  and may be requested for one account.

Scans use leases with heartbeats and expiry. Global and per-account concurrency,
per-cycle page budgets, fair account scheduling, jittered retry, Retry-After,
and upstream circuit breakers prevent a large archive or outage from starving
live publication. Capacity expansion is gated by a production-shaped benchmark
and configurable budgets, not an unverified account-count assumption.

## Exactly-Once AT Record Identity

The official [`app.bsky.feed.post` Lexicon](https://github.com/bluesky-social/atproto/blob/main/lexicons/app/bsky/feed/post.json)
declares `key: "tid"`; generic record-key syntax alone is insufficient. A full
Nostr event ID cannot be its rkey. Exactly-once identity therefore uses a durable
publication intent, not a truncated/rehashed event ID.

Before any media upload or PDS record call, one PostgreSQL transaction inserts:

```text
publication_intents
  account_id
  lifecycle_epoch
  nostr_event_id
  repo_did
  collection          app.bsky.feed.post
  rkey                standards-compliant TID allocated once
  source_fingerprint
  prepared_record     nullable canonical DAG-JSON after media resolution
  expected_record_hash nullable until prepared_record is persisted
  desired_state_version
  status              reserved | media_ready | prepared | written | mapped |
                      delete_pending | deleted
  UNIQUE (account_id, lifecycle_epoch, nostr_event_id)
  UNIQUE (repo_did, collection, rkey)
```

The standard TID generator runs under the uniqueness constraint and the intent
commits before the remote side effect. Original Nostr time remains in record
`createdAt`; the TID represents bridge publication time. Media job ID, resulting
blob/CID, and caption outcomes are persisted idempotently in `media_ready`.
Only then does a CAS persist the exact canonical record plus its hash and advance
to `prepared`; `written` is impossible without both. Queue deletion cannot erase
the intent or prepared artifact.

The compatibility release must prove the deployed rsky PDS atomically supports
`com.atproto.repo.createRecord` with a caller-supplied TID rkey. The first remote
write uses that create-only operation, not a `getRecord`/`putRecord` TOCTOU
sequence. `RecordAlreadyExists` and a lost response recover with `getRecord` at
the reserved URI and equality against the persisted exact record/hash. Absence
retries the same supplied-rkey create. Different content is
`REMOTE_RECORD_DIVERGED` / `needs_review` and is not overwritten or deleted.
If rsky fails this contract test, intent publication remains disabled until an
atomic create-only or equivalent `swapRecord` CAS is implemented and tested.

Existing TID mappings remain valid and are materialized as already-mapped
intents. Reconciliation never allocates a second intent for a mapped event.

Unfenced legacy auto-rkey `createRecord` and supplied-TID intent writers are never
active concurrently.
Accounts carry `publication_mode = legacy | draining | intent_v1`. During
dual-read/write rollout every account remains legacy. First deploy a compatibility
release whose legacy claim SQL enforces a DB-backed global/account writer epoch,
verify zero unfenced replicas remain, then set `draining`, stop claims, drain or
expire every old lease/side effect, and atomically set `intent_v1`. Rollback may
use only a compatible binary preserving
reserved intents; rolling back to unfenced `createRecord` is forbidden. A
mixed-version fault test pauses after PDS commit and proves a legacy claimer
cannot create a second TID record.

### Publication/deletion race protocol

Account lifecycle changes increment `account_lifecycle_epoch`; deletion,
supersession, moderation, or policy resolution increments only the affected
event/coordinate `desired_state_version`. A worker claims both values, then
immediately before media upload and again before `putRecord` locks/re-reads
account, source resolution, moderation, and tombstone state. If either differs
or state/policy is not currently eligible, it cancels. Unrelated posts do not
invalidate each other.

The remote side effect cannot share a PostgreSQL transaction. After `putRecord`,
mapping finalization atomically requires the claimed lifecycle epoch,
desired-state version, current policy version, `state=eligible`, and absence of
an effective tombstone/moderation block. If any predicate fails, it durably
schedules deletion of the reserved AT record even without a local mapping.
Deletion
work is idempotent and remains pending until both mapping and reserved
record absence are confirmed. Thus a publication that races deletion may appear
briefly due to external consistency, but cannot survive reconciliation or be
resurrected by retry.

## Typed Execution and Retry

Execution dispositions are `success`, `retryable`, `rejected`, and `cancelled`.
Retryable work retries indefinitely only while the shared eligibility predicate
continues to permit it. Backoff is exponential with full jitter, honors
`Retry-After`, caps at a configurable ten-minute default, and participates in
per-PDS circuit breaking and per-account fairness. Attempt count never changes
the disposition.

Opt-in/provision/disable/disconnect accept identity only from a Keycast-signed,
short-lived lifecycle assertion whose claims bind full Nostr pubkey, account ID,
operation, nonce, audience, and expiry. Gateway and atbridge verify issuer/JWKS,
signature, claims, and nonce, and derive identity from claims rather than request
bodies. Provisioning verifies the returned PDS session DID equals the account
mapping. Caller-selected identities and replayed assertions fail closed.

Per-repo writes require the verified per-account session; shared-token fallback
is removed. Create/refresh response DID and token subject must match the account;
absence is retryable and mismatch is `needs_review`. Authentication/session
refresh CAS compares account ID, DID, session version,
active-session tombstone, and account lifecycle epoch. Disconnect tombstones and
increments the lifecycle epoch in one transaction before cleanup, so an
in-flight refresh cannot recreate credentials. Sessions are envelope-encrypted
at rest with GCP KMS through workload identity and never logged.

Disconnect states are `active -> disconnecting -> disconnected` or
`cleanup_incomplete`. Entering `disconnecting` immediately fences new work,
cancels scans, tombstones refresh, and queues deletion of every bridge-created
record. A cleanup session may remain for at most seven days encrypted under a
distinct KMS key and exposed only to a separate DB role/view, workload identity,
delete-only worker image, and client implementing only `getRecord`/`deleteRecord`;
publication pods cannot decrypt it. Successful deletion destroys it and reports
`disconnected`. Revoked
credentials or PDS outage keep retrying and report `disconnecting`; after seven
days the account becomes `cleanup_incomplete` for operator/PDS-admin recovery.
A hard 30-day deadline destroys remaining credential ciphertext even if remote
cleanup is impossible, retaining only safe record URIs and audit state. The
product reports that remote cleanup could not be confirmed rather than claiming
success. Reconnect is rejected while cleanup is pending; after completion it
creates a new lifecycle epoch. A changed DID always cleans the old DID before a
new archive can publish.

Production media retrieval is restricted to configured Divine Blossom HTTPS
origins and the exact `x`-tag object path. It permits no URL credentials, query
override, arbitrary port, or noncanonical hash. Every DNS resolution and at most
three same-origin redirects are revalidated; the client connects to the validated
pinned IP and verifies the peer address. Loopback, link-local, RFC1918, metadata, and
other private ranges are blocked. Defaults are a 2,048-byte URL, 256-KiB event,
256 tags, 16 values/tag, 8-KiB tag value, five-second connect timeout, 120-second
total timeout, and byte/decompression ceiling of the lesser of 100 MiB and the
target PDS-advertised limit. MIME/dimensions are validated and SHA-256 must
match. Recoverable fetch failures are retryable; immutable violations rejected.

## Operator and Support Interfaces

The source API uses short-lived Google-signed GKE Workload Identity OIDC tokens.
Funnelcake validates Google JWKS signature, exact issuer, exact per-environment
audience `divine-funnelcake-bridge-source`, `exp`/`nbf` with bounded skew, and an
allowlist containing only the environment's divine-atbridge service-account
subject. Tokens live at most five minutes. Missing/unverifiable claims fail
closed; the current shared provisioning bearer is never accepted.

The operator API is exposed only through the existing Cloudflare Access-protected
internal hostname. The service independently validates the Access JWT against
the configured team issuer/JWKS, exact application audience, `exp`/`nbf`, and
derives actor from immutable `sub` plus email. A bridge DB role binding grants
`support_viewer`, `repair_operator`, or `migration_admin`; absent/disabled roles
fail closed. Per-actor mutation quotas, unique operation IDs, and Access plus
application audit prevent replay. Forwarded identity headers without a valid JWT
are ignored.

Versioned JSON endpoints are:

```text
GET  /internal/v1/accounts/{account_id}/sync
GET  /internal/v1/operations/{operation_id}
POST /internal/v1/reconciliation/operations
POST /internal/v1/revival/operations
POST /internal/v1/migration/operations
POST /internal/v1/disconnect/operations
```

Account sync returns the combined state, scan/snapshot IDs, safe cursors, reason
codes, lag, and all progress totals. Collection results use opaque pagination.
Mutations require `Idempotency-Key`, `{dry_run:true, reason, scope, limits,
expires_at}` and role-specific maximum accounts/pages/events. The server echoes
normalized scope and a confirmation digest. Dry-run returns `200`. Execution
requires a second request with the same key and `confirm_digest`; it returns
`202 {operation_id,status_url}`. Polling returns queued/running/succeeded/
partially_failed/failed/cancelled plus safe per-class counts. Errors use
`{code,message,retryable,request_id,operation_id?}` and stable HTTP semantics.

Every request, normalized scope, actor, confirmation, state transition, and
result is written to an append-only audit table. Controls cannot change event
ownership, revive rejected/cancelled work without eligibility re-evaluation,
accept arbitrary payloads/DIDs, or cross accounts.

Provisioning preserves existing fields and adds optional absolute
`archive_sync` pointing to the authorized status resource. The detailed surface
is operator/support-only in this release; a user-facing client is later.

## Shadow Reconciliation

Shadow mode performs discovery and comparison without creating/cancelling
publication or deletion work. Differences are stored, not only counted:

```text
shadow_differences
  run_id, account_id, nostr_event_id, difference_type
  expected_state, observed_state, source_digest, state_digest
  first_seen_at, last_seen_at
  UNIQUE(run_id, account_id, nostr_event_id, difference_type)
```

Run summaries retain counts, source upper bound, and a deterministic digest.
Detailed rows have bounded operational retention; aggregate/audit results
persist. Because ClickHouse traversal is not MVCC, repair promotion requires the
source bootstrap to be verified, then two consecutive deep traversals separated
by more than the configured indexing-delay settlement window. Each captures a
new upper bound; promotion requires no source changes/late arrivals between
runs, identical difference digests, zero unexplained false positives in sampled
accounts, and signed operator approval. Thresholds are reviewed configuration.

## Observability, Health, and SLOs

Process liveness and Kubernetes readiness remain limited to whether the process
can safely serve/consume work and reach mandatory dependencies. Publication SLO
degradation does not mark every pod unready and trigger an eviction loop.

Existing `/metrics` remains the backward-compatible JSON watchdog response
`{expired_leases,failed_backfills}` for one deprecation release. New Prometheus
OpenMetrics are served as `text/plain; version=0.0.4` at `/metrics/prometheus`.
`GET /health/workload` returns JSON
`{status: healthy|degraded, reasons:[code], oldest_eligible_age_seconds,
source_lag_seconds, checked_at}` with `200` when healthy and `503` when degraded.
It is explicitly not wired to Kubernetes `/health/ready`; workload degradation
must not evict all workers. Aggregate bounded-cardinality metrics expose:

- jobs by disposition and stable reason code;
- oldest eligible-unmapped age and eligible queue depth;
- source and publication lag by discovery mode/kind;
- publishes, failures, deletions, and reconciliation differences;
- account counts by discovery/convergence state;
- last successful publish and reconciliation timestamps;
- archive scan/convergence totals without account IDs as public metric labels.

Existing metric fields remain available through a deprecation window. Authorized
account detail comes from the support API, avoiding unbounded Prometheus labels.

Initial live SLOs are:

- 95% of eligible live events have their reserved AT record within five
  minutes of canonical source ingestion;
- 99% within thirty minutes;
- zero eligible source events permanently lost.

Archive progress is segmented by effective archive size. Initial cohort rollout
targets are p95 caught-up/caught-up-with-exceptions within 15 minutes for up to
100 events, two hours for 101-1,000, and 24 hours above 1,000, with 100% of
completed accounts showing zero unexplained eligible-unmapped records after a
deep traversal. Missing-source proof or any unexplained eligible-unmapped row
halts cohort expansion; intentionally rejected rows are reported exceptions.

Alerts cover sustained eligible-unmapped age, queue growth, failed/expired
scans, source lag, KMS/session failures, and reconciliation divergence. Alert
rules live with the service deployment in
`divine-iac-coreconfig`; divine-sky owns metric contract fixtures, and IaC CI
tests rule parsing and expected-series evaluation. Runbooks name the owning
team, severity, and safe repair operation.

## Backward-Compatible Rollout and Migration

### Immediate incident repair

1. Deploy the already merged proactive session-refresh image to staging.
2. Exercise an expired session and verify refresh, media upload, record creation,
   rotated-session persistence, and playback.
3. Promote the identical digest to production.
4. Run the slice-1 `divine-atbridge repair-legacy-badjwt` command, which works on
   the current schema. It is dry-run by default; requires explicit full event IDs
   or an exact escaped `BadJwt: Signature tag didn't verify` error predicate,
   account scope,
   maximum rows, operation ID, and `--confirm` digest; selects only terminal
   failed publish jobs, re-verifies event/account enabled state, stores bounded
   before-images in an additive `operator_actions` audit row, and atomically
   resets matching jobs to immediately claimable. `--rollback-operation-id`
   restores only rows not subsequently claimed/changed. Tests cover selection,
   bounds, revalidation, rollback, and secret-free output.
5. Verify source IDs, AT mappings, AppView records, and video playback.

### Expand/migrate/contract sequence

1. **Funnelcake expand:** add the append-only source table and authenticated v1
   API without changing public video reads. Backfill from `events_local` is a
   finite, checkpointed job with the count/digest proof above. The source API
   cannot advertise ready and no archive cohort starts until proof succeeds.
   Old Funnelcake binaries remain compatible.
2. **Bridge schema expand:** startup migration creates nullable/additive tables,
   columns, indexes, and constraints only. Old bridge binaries can continue
   writing the legacy queue.
3. **Dual-read/write:** new binaries materialize source/state for all new work
   while preserving legacy queue behavior. Feature flags keep intent-based
   writes, scans, and repair disabled.
4. **Session compatibility fence:** deploy a release whose refresh/store query
   requires session version, lifecycle epoch, active tombstone, and writer epoch;
   verify zero unconditional plaintext writers/pods remain before backfill.
5. **Session encryption expand:** add ciphertext, encrypted data-key, KMS key
   version, session version, lifecycle epoch, and tombstone columns. New binaries
   dual-read plaintext/ciphertext but write encrypted form plus legacy plaintext
   only while legacy pods remain. Drain/fence old writers; then a resumable KMS
   job encrypts every active row, verifies decrypt/account/DID binding, rotates a
   sample, and checkpoints. Null plaintext only after 100% verification; validate
   constraints after the rollback window, then drop plaintext columns in the
   contract release. Before plaintext nulling, rollback may restore the old
   binary; afterward only encryption-aware rollback is allowed. Key rotation
   writes a new version by CAS and retains the prior KMS decrypt version until
   all rows and backups pass verification.
6. **Explicit data migration job:** a separately invoked command acquires a
   migration lease, reads a stable key range in bounded transactions, checkpoints
   by queue primary key, and writes classification results idempotently. It never
   runs during pod startup.
7. **Shadow:** run complete source/state comparisons without repair, validate
   digests, pagination, SLO metrics, and capacity.
8. **Enable by slice:** intent-based AT writes, lossless live discovery, one
   test archive, Rabble archive/repair, scheduled reconciliation, then cohorts.
9. **Validate constraints:** after all writers are compatible and backfill is
   complete, validate foreign keys/checks/NOT NULL constraints without coupling
   rollout availability to data volume.
10. **Contract:** remove legacy cursor/job interpretation only after a full deep
   reconciliation and rollback window.

Old and new binaries may overlap only in explicitly dual-compatible phases.
Rollback disables new claimers/scans but retains additive state. It never drops
source facts, publication intents, or mappings. Funnelcake migration follows the same
additive/read-switch/retire pattern; no atomic multi-table swap is assumed.

### Legacy classification job

The operator command supports `--dry-run`, maximum rows, batch size, resume
token, and operation ID. It reports counts and stable samples without payloads.
Each batch commits its classifications and checkpoint together. A crash resumes
from the last committed key; reruns are idempotent.

Versioned, explicit predicates classify:

- completed plus mapping -> translated;
- pending/in-progress -> eligible, preserving runnable work and only reclaiming
  expired leases;
- non-terminal known retryable -> retryable with existing next retry;
- terminal known auth/network/PDS/rate-limit/server/media-processing code ->
  retryable;
- known validation/policy code -> rejected;
- known deletion/disable/opt-out/moderation code -> cancelled;
- free-form or unknown legacy error -> `LEGACY_AMBIGUOUS` / `needs_review`.

Free-form substring matching is permitted only through a reviewed versioned
mapping registry with a `needs_review` fallback. Before any migrated row is
executed, current signature, ownership, account generation, deletion,
moderation, and policy eligibility are re-evaluated.

Dry-run and mutation use the same classifier and stable snapshot. Partial
failure leaves additive rows and checkpoint intact. Rollback stops the job and
disables new claimers; it does not attempt lossy reverse classification.

## Verification Matrix

Every invariant has an executable proof owner:

| Invariant / failure | Proof |
|---|---|
| signature, ID, author/DID binding, all `e`/`a` targets | Rust unit/property tests |
| raw-source seed count/digest proof, MV/backfill overlap dedupe | Funnelcake ClickHouse integration tests |
| same-second composite paging, finite upper bound, token binding | Funnelcake ClickHouse integration test against real test table plus API test |
| page atomicity and scan CAS | isolated PostgreSQL integration fixtures with crash injection after every write boundary |
| >100 archive, >500 outage catch-up, two interleaved users | cross-service integration tests |
| PDS commit then bridge crash | mock-PDS fault test; restart proves one reserved record and recovered mapping |
| deletion before/after/racing publish; no resurrection | PostgreSQL + mock-PDS concurrency tests |
| retry beyond old cap, typed rejection, fair scheduling | Rust scheduler tests with deterministic clock |
| session CAS/encryption/redaction | unit/integration tests with fake KMS and log assertions |
| plaintext encryption backfill, refresh/disconnect race, key rotation | isolated PostgreSQL + fake-KMS fault tests |
| SSRF redirects/DNS rebinding/size/decompression/time limits | media-fetcher security tests |
| legacy classification/resume/partial failure | isolated PostgreSQL migration-command tests |
| shadow digest and promotion threshold | reconciliation integration tests |
| metric contract and SLO alert expressions | divine-sky metric fixtures plus IaC rule tests |
| expired session, multi-page archive, restart, dependency outage | staging acceptance |
| mappings, AppView records, playback, lag | bounded production audit |

The workspace standardizes on `cargo-llvm-cov` and CI runs
`cargo llvm-cov --workspace --all-features --lcov --output-path
target/llvm-cov/lcov.info`, followed by `scripts/check-coverage-thresholds`.
`.coverage-thresholds.json` is versioned as
`{"version":1,"global":{"lines":N,"functions":N,"regions":N},"modules":{...}}`.
The first implementation prerequisite creates the checker, threshold file, and
CI wiring. The active repository AGENTS policy requires 100% coverage, so every
changed/new Rust module must report 100% lines/functions/regions; generated or
provably unreachable-code exclusions require an inline justification and
explicit reviewed JSON entry. Coverage supplements fault/integration
tests. PostgreSQL tests create isolated per-test schemas/databases; ClickHouse
tests use isolated table names and deterministic cleanup.

Additional required cases:

- original Nostr time is stored in AT `createdAt` while commit time is current;
- same-timestamp late arrival with a lower event ID is found by overlap/deep
  reconciliation;
- a work budget yields without promoting high-water;
- unsupported media becomes typed rejected and is reconsidered only after a
  policy-version change;
- archive discovery sees deletion before publication convergence;
- a complete second reconciliation creates no new AT records or work;
- mixed-version deployment and malformed/expired cursor tokens fail safely;
- an archive survives an outage beyond cursor TTL through bound renewal;
- delete-before-observation, delete-then-new-revision, equal-time deletion, and
  out-of-order coordinate facts resolve identically;
- moderation block/unblock, missed moderation notification, and block racing
  `putRecord` converge without a lasting record;
- legacy/intent writer draining plus PDS/local crash creates one record;
- account disconnect racing publication ends with no AT record or stored
  session;
- disconnect/reconnect, disconnect with revoked credentials, cleanup deadline,
  and reconnect to a changed DID follow the declared product states;
- readiness remains stable while workload health/alerts correctly degrade.

## Implementation Slices

Each slice is independently deployable and reversible:

1. immediate auth deployment and production repair;
2. Funnelcake append-only source read model and v1 pagination API;
3. bridge additive authoritative schema and shared eligibility/reason registry;
4. KMS/session lifecycle fencing, media safety, and authenticated operator/source
   boundaries;
5. durable publication intents, writer drain fence, `putRecord`, and crash recovery;
6. transactional live/archive discovery and bounded scheduling;
7. durable multi-target deletion, moderation, and version-fenced reconciliation;
8. shadow/deep reconciliation, operator API, observability, and SLO alerts;
9. resumable legacy migration and cohort rollout;
10. production convergence audit and legacy contract retirement.

The implementation plan assigns every slice a repository owner, feature flag,
schema/API prerequisite, failing tests, promotion evidence, and rollback action.
No archive cohort can publish before slices 2-7 meet their exit gates.

## Non-Goals

- two-way Bluesky-to-Divine synchronization;
- importing another person's archive without verified ownership;
- publishing every superseded NIP-33 edit as a separate feed post;
- making direct ATProto edits authoritative over signed Divine facts;
- exposing detailed per-account state in public metrics;
- replacing moderation policy.

## Acceptance Criteria

The work is complete when:

1. Rabble's currently missing posts publish and play, and their mappings survive
   a full reconciliation with no duplicates.
2. A connected user with more than 100 retained eligible posts receives the
   complete effective backdated archive without deleted/superseded posts.
3. The historical source build has a durable verified upper/checkpoint plus
   matching raw/target counts and ID digests; gaps block rollout.
4. Same-second, late, outage, restart, and cursor-token tests prove lossless
   finite discovery.
5. The PDS-commit/local-crash test proves exactly one reserved AT record.
6. Retryable work recovers after dependency restoration beyond the old cap.
7. Every `e`/`a` deletion and moderation race ends without a lasting record.
8. Deep reconciliation repairs an intentionally omitted event and the next run
   has an identical zero-difference digest.
9. Account/archive/disconnect states satisfy their count-based predicates and
   the archive cohort thresholds.
10. Operators can audit and safely repair account state, and SLO alerts fire in
   tests before a user report is required.
11. Staging and production use identical verified image digests per promoted
   service revision.
