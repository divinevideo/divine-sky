# Lossless Divine-to-ATProto Crosspost Reconciliation RFC

**Date:** 2026-07-12
**Status:** Approved product direction; design-review revision 2

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
        -> deterministic AT record
```

`publish_jobs` is a rebuildable execution queue. It is never authoritative
evidence that an event was published, rejected, deleted, or superseded.

## Product Contract

When a Divine user connects ATProto distribution:

- connection completes immediately and archive import begins automatically;
- the account exposes importing, caught-up, or attention-required state to
  authorized support tooling;
- the user's complete retained Divine video archive imports asynchronously;
- AT feed records use the original Nostr `created_at` as the record's
  `createdAt`; AT repository commit and AppView index times remain later and are
  not backdated;
- every future eligible post eventually publishes without manual replay;
- deleted, superseded, disabled, opted-out, or moderation-blocked posts do not
  remain published;
- disabling or disconnecting crosspost deletes existing bridge-created AT
  records, clears bridge credentials, and prevents resurrection;
- recovery continues across bridge, database, PDS, media-service, and source
  API outages.

Archive completeness is bounded by the canonical source's retention contract.
The new bridge source retains all verified relevant signed events indefinitely;
if that contract ever changes, the product and SLO language must change first.

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

Archive discovery resolves the entire finite snapshot, including deletions and
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
and an account `eligibility_generation`.

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
2. Every eligible effective event has exactly one deterministic AT record or a
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
`event_id FixedString(64)`, verified signed payload, ingestion time, and
verification version. It uses a SharedMergeTree-compatible MergeTree family
engine and:

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
snapshots are neither the source nor a backfill input. The migration is
additive/idempotent, uses one logical object per DDL statement, does not use
`ON CLUSTER` or multi-table rename, and uses `SET alter_sync = 2` for metadata
ALTERs and `SET mutations_sync = 2` for any bounded materialization. This is
consistent with the repository's ClickHouse Cloud migration requirements.

### Versioned pagination protocol

Endpoint:

```text
GET /internal/v1/bridge/source-events
```

It requires service identity, an authorized bridge audience, a full lowercase
64-character author pubkey, `kinds` drawn from `{34235,34236,5}`, and `limit`
within `1..=500`. It returns complete signed events only after source
verification.

The first request omits `cursor`. The server atomically determines the greatest
available `(created_at,event_id)` for the author/kinds and returns it inside an
opaque, signed/versioned `v1` cursor token. That tuple defines a finite snapshot.
The token binds author, kinds, ascending direction, after tuple, upper tuple,
page limit ceiling, and expiry. Parallel timestamp/ID fields are not accepted;
malformed, mixed-author, expired, or partially supplied state is rejected.

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

For an initial scan, `after` is negative infinity. The response contains:

```text
events             at most requested_limit events
next_cursor        opaque v1 token, absent only at snapshot completion
snapshot_upper     diagnostic tuple, constant for the scan
has_more           true iff the extra row exists within the upper bound
```

`next_cursor.after` is the last returned tuple. An empty page with `has_more`
false terminates. Returning an empty page with `has_more` true is a protocol
error and retryable. Legacy public timestamp cursors remain unchanged and are
never mixed with the internal protocol.

The raw source is updated continuously rather than through the five-minute
public snapshot refresh/cache path, so it can support the five-minute live SLO.
API and ClickHouse integration tests prove the tuple predicates, token binding,
same-second behavior, late arrivals, and finite upper bound.

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

An existing event ID with different bytes/pubkey is a security error and cannot
overwrite the stored fact. All `e` and syntactically valid `a` tags are expanded
into target rows. A deletion is effective only when its verified signer owns the
target event/coordinate. Unknown targets remain durable and are resolved when
the target later arrives.

Resolved state is separate from immutable facts:

```text
translation_states
  account_id
  nostr_event_id
  state
  reason_code
  eligibility_policy_version
  eligibility_generation
  last_evaluated_at
  PK (account_id, nostr_event_id)
```

`record_mappings` has `UNIQUE(account_id, nostr_event_id)` and `UNIQUE(at_uri)`.
`publish_jobs` has `UNIQUE(account_id, nostr_event_id)`, references the state it
executes, and can be regenerated from eligible states without a mapping.

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
last_published_created_at / last_published_event_id
last_reconciled_at
lifetime_scan/publish counters
last safe reason code and bounded diagnostic
eligibility_generation
```

For each source page, one PostgreSQL transaction:

1. locks the scan row and verifies lease/generation;
2. inserts immutable `source_events` and every deletion target;
3. recomputes affected coordinate, tombstone, and eligibility states;
4. inserts/cancels derived jobs idempotently;
5. records shadow differences when in shadow mode;
6. updates the scan's after tuple and per-run counters;
7. commits.

No source mutation, job, or cursor becomes visible alone. A crash before commit
replays the page; a crash after commit resumes after it. Reaching the captured
upper tuple promotes the account high-water in a separate compare-and-swap that
requires the same scan ID, upper tuple, completed status, and eligibility
generation. It then starts publication convergence for archive scans.

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

New bridge-created video records use `com.atproto.repo.putRecord` with a
deterministic rkey derived from the complete Nostr event ID. The encoding is a
versioned, ATProto-valid, collision-resistant textual representation of all 32
event-ID bytes; the implementation must not truncate the ID. The repository is
the connected account's verified DID, never a value from event content.

Therefore `(account DID, collection, deterministic rkey)` is the remote
idempotency key. Retrying after a timeout or crash overwrites the same logical
record with identical translated content. A crash after PDS commit but before
the local mapping commit recovers by reading that deterministic record,
verifying its embedded/source event identity, and inserting the missing mapping.

Existing mappings with PDS-generated TID rkeys remain valid and are not moved.
Before deterministic publication, the worker first checks for an existing local
mapping. Migration/reconciliation never creates a deterministic replacement for
an already mapped legacy event.

### Publication/deletion race protocol

Every account eligibility change increments `eligibility_generation`. A worker
claims work at generation `G`, then immediately before media upload and again
before `putRecord` locks/re-reads account, source resolution, and tombstone state.
If the current generation/state differs, it cancels without publication.

The remote side effect cannot share a PostgreSQL transaction. After `putRecord`,
mapping finalization performs a compare-and-swap against generation `G`. If it
fails, it durably schedules deletion of the deterministic AT record. Deletion
work is idempotent and remains pending until both mapping and deterministic
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

Authentication/session refresh uses compare-and-swap session versions so two
workers cannot overwrite a newer rotation. Sessions are envelope-encrypted at
rest with GCP KMS through workload identity, never logged, and removed on
disconnect/account deletion. PDS account/session/DID are derived only from the
verified account mapping.

Media retrieval uses an allowlisted Divine media service or an equivalent
SSRF-safe fetcher: HTTPS only; allowed ports/origins; DNS resolution and every
redirect revalidated; loopback, link-local, RFC1918, metadata, and other private
ranges blocked; strict redirect, byte, duration, decompression, MIME, and
dimension limits; content hash verification where supplied. Fetch failures that
may recover are retryable; immutable policy violations are rejected.

## Operator and Support Interfaces

Detailed account status and repair are not public metrics. An authenticated
admin API behind the existing operator identity/access layer provides:

- per-account discovery/convergence state, safe cursors, last scanned and last
  published event IDs, reason codes, and lag;
- dry-run reconciliation and legacy-migration reports;
- bounded revival by account/event and allowlisted retryable reason;
- deep-scan requests with maximum accounts/pages/events and explicit expiry.

Mutations default to dry-run and require actor identity, operation ID, reason,
limits, and confirmation. Every request and result is written to an immutable
audit table. Controls cannot change event ownership, revive rejected/cancelled
work without re-evaluation, accept arbitrary payloads/DIDs, or cross accounts.

Provisioning preserves existing response fields and may add an optional
`archive_sync` status link. The detailed endpoint is operator/support-only in
this release; a user-facing progress surface is a later compatible product
decision.

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

Run summaries retain counts and a deterministic digest. Detailed rows have a
bounded operational retention; aggregate/audit results persist. Repair promotion
requires two consecutive full runs over the same finite snapshot with identical
digests, zero unexplained false positives in sampled accounts, and signed
operator approval. Promotion thresholds are configuration with reviewed
defaults, not hidden code constants.

## Observability, Health, and SLOs

Process liveness and Kubernetes readiness remain limited to whether the process
can safely serve/consume work and reach mandatory dependencies. Publication SLO
degradation does not mark every pod unready and trigger an eviction loop.

A separate workload-health endpoint and aggregate bounded-cardinality metrics
expose:

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

- 95% of eligible live events have their deterministic AT record within five
  minutes of canonical source ingestion;
- 99% within thirty minutes;
- zero eligible source events permanently lost.

Archive SLO/progress is separate. Alerts cover sustained eligible-unmapped age,
queue growth, failed/expired scans, source lag, KMS/session failures, and
reconciliation divergence. Alert rules live with the service deployment in
`divine-iac-coreconfig`; divine-sky owns metric contract fixtures, and IaC CI
tests rule parsing and expected-series evaluation. Runbooks name the owning
team, severity, and safe repair operation.

## Backward-Compatible Rollout and Migration

### Immediate incident repair

1. Deploy the already merged proactive session-refresh image to staging.
2. Exercise an expired session and verify refresh, media upload, record creation,
   rotated-session persistence, and playback.
3. Promote the identical digest to production.
4. Re-evaluate and revive Rabble's missing events and all jobs with the same
   typed retryable authentication failure.
5. Verify source IDs, AT mappings, AppView records, and video playback.

### Expand/migrate/contract sequence

1. **Funnelcake expand:** add the append-only source table and authenticated v1
   API without changing public video reads. Backfill/verification is a bounded,
   resumable job. Old Funnelcake binaries remain compatible.
2. **Bridge schema expand:** startup migration creates nullable/additive tables,
   columns, indexes, and constraints only. Old bridge binaries can continue
   writing the legacy queue.
3. **Dual-read/write:** new binaries materialize source/state for all new work
   while preserving legacy queue behavior. Feature flags keep deterministic
   writes, scans, and repair disabled.
4. **Explicit data migration job:** a separately invoked command acquires a
   migration lease, reads a stable key range in bounded transactions, checkpoints
   by queue primary key, and writes classification results idempotently. It never
   runs during pod startup.
5. **Shadow:** run complete source/state comparisons without repair, validate
   digests, pagination, SLO metrics, and capacity.
6. **Enable by slice:** deterministic AT writes, lossless live discovery, one
   test archive, Rabble archive/repair, scheduled reconciliation, then cohorts.
7. **Validate constraints:** after all writers are compatible and backfill is
   complete, validate foreign keys/checks/NOT NULL constraints without coupling
   rollout availability to data volume.
8. **Contract:** remove legacy cursor/job interpretation only after a full deep
   reconciliation and rollback window.

Old and new binaries may overlap only in explicitly dual-compatible phases.
Rollback disables new claimers/scans but retains additive state. It never drops
source facts or deterministic mappings. Funnelcake migration follows the same
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
| same-second composite paging, finite upper bound, token binding | Funnelcake ClickHouse integration test against real test table plus API test |
| page atomicity and scan CAS | isolated PostgreSQL integration fixtures with crash injection after every write boundary |
| >100 archive, >500 outage catch-up, two interleaved users | cross-service integration tests |
| PDS commit then bridge crash | mock-PDS fault test; restart proves one deterministic record and recovered mapping |
| deletion before/after/racing publish; no resurrection | PostgreSQL + mock-PDS concurrency tests |
| retry beyond old cap, typed rejection, fair scheduling | Rust scheduler tests with deterministic clock |
| session CAS/encryption/redaction | unit/integration tests with fake KMS and log assertions |
| SSRF redirects/DNS rebinding/size/decompression/time limits | media-fetcher security tests |
| legacy classification/resume/partial failure | isolated PostgreSQL migration-command tests |
| shadow digest and promotion threshold | reconciliation integration tests |
| metric contract and SLO alert expressions | divine-sky metric fixtures plus IaC rule tests |
| expired session, multi-page archive, restart, dependency outage | staging acceptance |
| mappings, AppView records, playback, lag | bounded production audit |

The workspace adds `.coverage-thresholds.json` and a CI coverage command using
the repository's selected Rust coverage tool. Thresholds start at the measured
baseline and may not decrease; all newly introduced reconciliation/state modules
require at least 85% line coverage and branch coverage where the tool supports
it. Coverage supplements, rather than replaces, the fault and integration tests.
PostgreSQL tests create isolated per-test schemas/databases; ClickHouse tests use
isolated table names and deterministic cleanup.

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
- account disconnect racing publication ends with no AT record or stored
  session;
- readiness remains stable while workload health/alerts correctly degrade.

## Implementation Slices

Each slice is independently deployable and reversible:

1. immediate auth deployment and production repair;
2. Funnelcake append-only source read model and v1 pagination API;
3. bridge additive authoritative schema and shared eligibility/reason registry;
4. deterministic AT rkeys, `putRecord`, and crash recovery;
5. transactional live/archive discovery and bounded scheduling;
6. durable multi-target deletion and generation-fenced reconciliation;
7. shadow, incremental, and deep reconciliation plus operator API;
8. observability, KMS/session hardening, media safety, and SLO alerts;
9. resumable legacy migration and cohort rollout;
10. production convergence audit and legacy contract retirement.

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
3. Same-second, late, outage, restart, and cursor-token tests prove lossless
   finite discovery.
4. The PDS-commit/local-crash test proves exactly one deterministic AT record.
5. Retryable work recovers after dependency restoration beyond the old cap.
6. Every `e`/`a` deletion race test ends without a lasting/resurrected record.
7. Deep reconciliation repairs an intentionally omitted event and the next run
   has an identical zero-difference digest.
8. Operators can audit and safely repair account state, and SLO alerts fire in
   tests before a user report is required.
9. Staging and production use identical verified image digests per promoted
   service revision.
