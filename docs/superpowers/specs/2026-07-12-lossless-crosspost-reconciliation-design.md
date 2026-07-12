# Lossless Divine-to-ATProto Crosspost Reconciliation

**Date:** 2026-07-12
**Status:** Approved design direction; written specification awaiting final review

## Problem

`divine-atbridge` currently behaves like a best-effort mirror rather than an
at-least-once publication system.

Production evidence on 2026-07-12 showed:

- the live ingester accepted three new posts from `rabble.divine.video`;
- production had not yet deployed the merged proactive expired-session refresh;
- two Rabble publish jobs repeatedly failed at `getServiceAuth` with an expired
  account JWT reported by rsky as `400 BadJwt`;
- the public Bluesky feed remained at the previous day's newest post;
- `/health/ready` and `/metrics` remained green because publish failures do not
  affect readiness and the watchdog counts only expired leases and failed
  initial backfills;
- the REST ingest cursor stores Unix seconds and filters with
  `event.created_at > cursor`, which can permanently skip a second event in the
  same second;
- REST ingest fetches at most five 100-event pages and can advance its cursor
  beyond older events it never fetched;
- a job terminalized after the retry cap remains in `publish_jobs`, and the
  idempotency check treats every existing job as processed, so later replay
  cannot repair it;
- initial author replay is intentionally capped at 100 events and never performs
  scheduled reconciliation after completion.

These are independent failure modes. Deploying session refresh repairs today's
authentication incident but does not make publication lossless.

## Core Principle

The bridge is a reconciliation system, not a streaming system. Streams and
pollers provide latency; reconciliation provides correctness. Every canonical
source event remains discoverable until it is successfully translated or
explicitly rejected for a durable reason.

The canonical Nostr event, its durable translation state, and any resulting AT
record are authoritative. `publish_jobs` is a rebuildable execution queue, not
the source of truth.

## Product Decision

When a Divine user connects ATProto distribution:

- connection completes immediately;
- their complete eligible Divine video archive imports asynchronously;
- historical ATProto posts retain the original Nostr `created_at` timestamp;
- deleted posts do not remain published;
- every future eligible post is eventually published without requiring a human
  to notice or replay it;
- recovery continues across bridge, database, PDS, video-service, and source API
  outages.

"Complete archive" means the user's signed kind `34235` and `34236` video
events available from the canonical Divine source, reconciled with their signed
kind `5` deletions. Kind `0` remains profile synchronization, not a feed post.
Moderation, disabled-account, and explicit crosspost opt-out rules still apply.

## Reliability Invariants

1. A valid eligible Nostr event is either mapped to an AT URI or has a durable,
   non-terminal retry scheduled.
2. Advancing a source checkpoint never makes an unqueued event unreachable.
3. Replaying a page, job, or entire account is safe because Nostr event IDs are
   the idempotency keys.
4. Restarts may duplicate reads but must not duplicate ATProto records.
5. Retryable failures never become permanently invisible merely because an
   attempt counter reached a threshold.
6. Rejected events are explicit, inspectable, and excluded only for a typed
   reason such as invalid signature, unsupported media, deletion, moderation, or
   user opt-out.
7. Service health exposes publication lag and failure state, not just process
   liveness.

## Architecture

The system uses three cooperating discovery paths. Each records canonical event
and translation state before deriving work in `publish_jobs`.

### Authoritative state and derived work

`source_events` materializes the source facts needed for deterministic recovery:

```text
nostr_event_id           primary key
nostr_pubkey             account foreign key
kind                     source event kind
event_created_at         original Nostr timestamp
event_payload            complete signed Nostr event
translation_state        discovered | eligible | translated | rejected |
                         cancelled | needs_review
translation_reason       nullable typed reason
discovered_at            first discovery timestamp
updated_at               timestamp
```

`record_mappings` remains the AT publication result. Its existing primary key on
`nostr_event_id` prevents multiple mappings for one source event, and its existing
unique index on `at_uri` prevents one AT record from being claimed twice.

`publish_jobs.nostr_event_id` remains a primary key and references the source
event being executed. The queue can be deleted and regenerated from eligible
`source_events` that lack a published mapping or intentional rejection. Queue
state alone never proves that an event was translated.

### 1. Live path

The live path remains low latency. It polls Funnelcake for video events, but it
uses deterministic composite pagination and per-kind checkpoints.

Funnelcake's `/api/videos/events` contract gains backward-compatible fields:

- request: `after=<created_at>` and `after_id=<event_id>` for ascending scans;
- request: existing `before=<created_at>` plus new `before_id=<event_id>` for
  deterministic descending scans;
- ascending ordering is exactly `ORDER BY created_at ASC, id ASC`;
- descending ordering is exactly `ORDER BY created_at DESC, id DESC`;
- event IDs are compared as their canonical lowercase 64-character hex strings;
- response: existing `next_cursor` plus `next_cursor_id`;
- optional `author=<full hex pubkey>` filter for account archive scans.

The old timestamp-only request and response remain valid for existing clients.
The bridge creates independent offsets such as
`divine-sky-bridge:34235` and `divine-sky-bridge:34236` so one kind cannot move
the other kind's checkpoint.

Each poll begins one configurable overlap window before the durable high-water
timestamp. The default is five minutes, chosen to exceed normal Funnelcake
indexing and cache delay while keeping repeated reads inexpensive. Operators may
increase it without a migration. The overlap captures delayed indexing and
same-second arrivals whose event ID sorts below the prior high-water ID. Pages
are read oldest-first through a composite cursor. Every event is recorded and
offered to the idempotent queue, and the durable high-water advances only after
the full scan succeeds. A crash repeats the overlap; it cannot create a gap.

There is no silent `max_pages` completion. A configurable work budget may yield
and persist an in-progress scan cursor, but it must not advance the durable
high-water until it reaches the scan's captured upper bound.

### 2. Full archive import

Provisioning leaves the account ready immediately and creates archive state in
`account_archive_sync`:

```text
nostr_pubkey             primary key / account foreign key
status                   pending | running | reconciling | completed | failed
scan_created_at          nullable current composite cursor timestamp
scan_event_id            nullable current composite cursor event ID
scan_upper_created_at    immutable upper bound captured when a scan starts
scan_upper_event_id      immutable upper bound captured when a scan starts
last_successful_event_id last event whose translation reached a durable outcome
events_scanned           cumulative count
jobs_enqueued            cumulative count
last_reconciled_at       nullable timestamp
last_error               nullable diagnostic
updated_at               timestamp
```

The archive worker captures `scan_upper_created_at` and `scan_upper_event_id`
before reading its first page. This defines a finite snapshot. It scans each
enabled author from the oldest available event to that bound, in bounded pages,
and commits its composite cursor after each page whose source events and derived
jobs are durably recorded. It processes videos and deletion events in source
order. Published AT records retain the event's original timestamp.

An interrupted import resumes from the stored composite cursor. Completion
means the captured upper bound was reached, not merely that one relay
subscription returned EOSE. There is no event-count cap.

### 3. Scheduled reconciliation

After initial completion, every enabled account is reconciled in two modes:

- **Incremental reconciliation:** scan from a configurable window before
  `last_reconciled_at` through a newly captured upper bound. The default window
  is 24 hours. Event-ID idempotency discards repeats.
- **Deep reconciliation:** scan the entire available history and compare every
  source event against source, translation, mapping, deletion, and queue state.
  It runs on a slower cadence and can also be requested for one account.

Together these satisfy the stronger invariant that every source event is
eventually compared against bridge state, not merely assumed handled because a
stream cursor passed it.

This path repairs:

- events missed by live cursor or source-cache behavior;
- terminal retryable jobs created by an older bridge version;
- outages longer than the live overlap window;
- delayed deletion events;
- operator mistakes that temporarily disabled a dependency.

## Durable Job Semantics

Publish execution returns a typed disposition:

- `success`: mapping written and job completed;
- `retryable`: retry indefinitely while the event and account remain eligible,
  with exponential backoff capped at ten minutes;
- `rejected`: complete intentionally with a structured durable reason;
- `cancelled`: source deletion, account disablement, or opt-out made publication
  no longer eligible.

Authentication expiry, refresh failures, HTTP `408`, `429`, `5xx`, network
errors, PDS unavailability, and video-service processing timeouts are retryable.
Invalid signatures, unsupported kinds, invalid media, and explicit policy
rejection are rejected.

Eligibility is re-evaluated before every retry and during reconciliation. If the
account no longer exists, is disabled, opts out, or the event has been deleted,
the job becomes cancelled rather than retrying indefinitely. The attempt counter
remains diagnostic. It no longer converts retryable errors to terminal state.
Failed retryable rows remain claimable after `retry_at`. Pending jobs are not
starved by an older failed job in backoff.

Reconciliation must distinguish a published/rejected job from a retryable failed
job. An existing retryable row is revived or left scheduled;
it is never treated as proof that the source event was handled successfully.

An operator command performs narrowly scoped revival by event ID, account, or
typed retryable reason. The first production use revives Rabble's three current
events and any other account jobs failed by the expired-session signature error.

## Deletions And Addressable Events

The archive source includes kind `5` events. A deletion cancels an unpublished
job or deletes an existing mapped AT record. Replay remains idempotent.

Deletion state is authoritative over queued work. Immediately before any
publication side effect, a worker re-reads source eligibility and cancellation
state. In the `publish -> delete -> publish retry` race, the retry observes the
tombstone and cannot resurrect the post. If publication and deletion are already
concurrent, the durable deletion remains pending until it observes and removes
any mapping created by the racing publish.

For replaceable NIP-71 coordinates, current repository behavior remains the
source of truth: each signed event ID is independently mapped unless an existing
translation rule explicitly resolves it as a replacement. This project does not
invent new Nostr replacement semantics.

## Observability And Operations

`/metrics` exposes at least:

- pending, in-progress, retryable-failed, and rejected jobs;
- oldest pending/retryable job age;
- successful publishes and failures by typed reason;
- timestamp of last successful publish;
- live-source lag by kind;
- accounts pending/running/failed archive import;
- per-account archive scan progress totals;
- timestamp and outcome of the last reconciliation pass.

Readiness becomes degraded when publication makes no successful progress while
eligible jobs age beyond a configured threshold, or when live-source lag exceeds
its threshold. One malformed rejected event does not take the process out of
service.

Alerts target sustained publish lag, growing queue depth, failed archive scans,
and source reconciliation lag. The runbook includes safe job revival and
per-account audit commands without exposing credentials.

The initial live-publication SLO is:

- at least 95% of eligible live events published within five minutes;
- at least 99% published within thirty minutes;
- no eligible source event permanently lost.

Alerts derive from these targets instead of process uptime alone. Archive-import
progress is tracked separately because a large historical import is expected to
take longer than live publication.

## Rollout

### Immediate remediation

1. Deploy the merged proactive session-refresh image to staging.
2. Exercise an expired account session and verify first-attempt proactive
   refresh, video upload, AT record creation, and persisted rotated session.
3. Promote the identical image digest to production.
4. Revive Rabble's three current jobs and all jobs with the same expired-session
   retryable signature.
5. Verify the three source event IDs have AT mappings and playable Bluesky
   posts with their original timestamps.

### Durable rollout

1. Ship Funnelcake composite/ascending/author pagination with compatibility
   tests.
2. Ship database migration and typed job dispositions.
3. Enable lossless per-kind live scans.
4. Run reconciliation in **shadow mode**: record and report differences without
   enqueuing repair work.
5. Compare shadow results to canonical source events and existing mappings, then
   enable repair for one test account.
6. Enable resumable full archive import and repair for Rabble.
7. Enable scheduled reconciliation and observability.
8. Migrate existing ready accounts into archive sync without duplicating
   records.
9. Expand to all connected users after queue lag, shadow differences, and error
   rates remain nominal.

Feature flags separate live cursor changes, archive import, and scheduled
reconciliation so each can be rolled back without disabling already working
crossposts.

### Existing-state migration

Migration classifies existing rows explicitly:

- completed job with a mapping: materialize translated source state and leave
  the completed queue row unchanged;
- pending or in-progress job: materialize eligible source state and preserve it
  as runnable work, reclaiming only expired leases;
- non-terminal failed job: classify as retryable and preserve its next retry;
- terminal failed job with a known authentication, network, PDS, rate-limit,
  server, or video-timeout error: convert to retryable and clear terminal state;
- known invalid-signature, unsupported-kind, invalid-media, moderation, deletion,
  disablement, or opt-out outcome: materialize rejected or cancelled state and
  do not requeue;
- ambiguous legacy failure: mark `needs_review`, expose it in shadow
  reconciliation, and do not silently discard or republish it.

The migration is idempotent and can be dry-run to report counts per class before
changing queue state.

## Testing

Automated coverage must prove:

- more than 100 historical events import completely across pages;
- events sharing one timestamp are neither skipped nor duplicated;
- an event arriving late with the same timestamp and a lower event ID is found
  by overlap reconciliation;
- a scan interrupted after any page resumes at the correct composite cursor;
- an outage producing more than 500 events catches up without advancing past
  unfetched pages;
- both video kinds maintain independent live checkpoints;
- original timestamps survive translation and AT publication;
- deletion-before-publication cancels work and deletion-after-publication
  deletes the record;
- expired sessions refresh before `getServiceAuth` and repo writes;
- retryable jobs remain runnable beyond the former attempt cap;
- rejected failures do not retry;
- reconciliation revives legacy terminal retryable jobs;
- two users with interleaved events both converge;
- given page `A, B, C`, a crash after enqueueing `B` but before committing the
  page cursor causes restart to ignore duplicate `A` and `B` and process `C`;
- `publish -> delete -> publish retry` cannot resurrect the deleted record;
- health degrades on sustained publish lag and recovers after progress;
- a complete second reconciliation produces no duplicate AT records.

Staging verification uses at least two accounts, including one expired session,
one multi-page archive, same-second fixtures, a forced worker restart, and a
temporary dependency outage. Production verification compares canonical source
event IDs to `record_mappings`, then checks public AppView records and video
playback.

## Non-Goals

- Two-way Bluesky-to-Divine synchronization.
- Importing another person's restored Vine archive into a newly connected
  account without a verified ownership mapping.
- Changing Divine's Nostr-native authoring model.
- Treating direct ATProto edits as authoritative over signed Divine events.
- Replacing moderation or account opt-out policy.

## Acceptance Criteria

The work is complete when:

1. Rabble's three currently missing posts publish and play on Bluesky.
2. A connected user with more than 100 eligible historical posts receives the
   full backdated archive.
3. Same-second, late, outage, and restart tests demonstrate at-least-once source
   delivery and exactly-once AT record creation by event ID.
4. A retryable failed job automatically recovers after its dependency is
   restored, even beyond the previous retry cap.
5. Scheduled reconciliation repairs an intentionally omitted live event.
6. Operators can see publish and archive lag and receive an alert before a user
   reports missing posts.
7. Staging and production use the same verified image digest for each promoted
   service revision.
