# Lossless Divine-to-ATProto Crosspost Reconciliation

**Date:** 2026-07-12
**Status:** Approved design direction; written specification awaiting final review

## Problem

`divine-atbridge` currently behaves like a best-effort mirror rather than an
at-least-once publication system.

Production evidence on 2026-07-12 showed:

- the live ingester accepted three new posts from `rabble.divine.video`;
- production still ran image `0c50ea5`, while proactive expired-session refresh
  existed only in merged revision `e532ea1` and a staging registry image;
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
5. Transient failures never become permanently invisible merely because an
   attempt counter reached a threshold.
6. Permanent failures are explicit, inspectable, and excluded only for a typed
   reason such as invalid signature, unsupported media, deletion, moderation, or
   user opt-out.
7. Service health exposes publication lag and failure state, not just process
   liveness.

## Architecture

The system uses three cooperating paths that all feed the existing durable
`publish_jobs` table.

### 1. Live path

The live path remains low latency. It polls Funnelcake for video events, but it
uses deterministic composite pagination and per-kind checkpoints.

Funnelcake's `/api/videos/events` contract gains backward-compatible fields:

- request: `after=<created_at>` and `after_id=<event_id>` for ascending scans;
- request: existing `before=<created_at>` plus new `before_id=<event_id>` for
  deterministic descending scans;
- ordering: `(created_at, id)` with the direction matching the scan;
- response: existing `next_cursor` plus `next_cursor_id`;
- optional `author=<full hex pubkey>` filter for account archive scans.

The old timestamp-only request and response remain valid for existing clients.
The bridge creates independent offsets such as
`divine-sky-bridge:34235` and `divine-sky-bridge:34236` so one kind cannot move
the other kind's checkpoint.

Each poll begins five minutes before the durable high-water timestamp. This
overlap captures delayed indexing and same-second arrivals whose event ID sorts
below the prior high-water ID. Pages are read oldest-first through a composite
cursor. Every event is offered to the idempotent queue, and the durable
high-water advances only after the full scan succeeds. A crash repeats the
overlap; it cannot create a gap.

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
events_scanned           cumulative count
jobs_enqueued            cumulative count
last_reconciled_at       nullable timestamp
last_error               nullable diagnostic
updated_at               timestamp
```

The archive worker scans each enabled author from the oldest available event to
the captured upper bound, in bounded pages, and commits its composite cursor
after each successfully queued page. It enqueues videos and deletion events in
source order. Published AT records retain the event's original timestamp.

An interrupted import resumes from the stored composite cursor. Completion
means the captured upper bound was reached, not merely that one relay
subscription returned EOSE. There is no event-count cap.

### 3. Scheduled reconciliation

After initial completion, every enabled account is reconciled periodically.
The normal reconciliation scans from at least 24 hours before
`last_reconciled_at` through a newly captured upper bound, relying on event-ID
idempotency to discard repeats. A slower full audit can reset the scan to the
oldest available event without republishing mapped records.

This path repairs:

- events missed by live cursor or source-cache behavior;
- terminal transient jobs created by an older bridge version;
- outages longer than the live overlap window;
- delayed deletion events;
- operator mistakes that temporarily disabled a dependency.

## Durable Job Semantics

Publish execution returns a typed disposition:

- `success`: mapping written and job completed;
- `transient`: retry forever with exponential backoff capped at ten minutes;
- `permanent`: complete as rejected with a structured reason;
- `cancelled`: source deletion, account disablement, or opt-out made publication
  no longer eligible.

Authentication expiry, refresh failures, HTTP `408`, `429`, `5xx`, network
errors, PDS unavailability, and video-service processing timeouts are transient.
Invalid signatures, unsupported kinds, permanently invalid media, and explicit
policy rejection are permanent.

The attempt counter remains diagnostic. It no longer converts transient errors
to terminal state. Failed transient rows remain claimable after `retry_at`.
Pending jobs are not starved by an older failed job in backoff.

Reconciliation must distinguish a published/permanently-rejected job from a
retryable failed job. An existing transient row is revived or left scheduled;
it is never treated as proof that the source event was handled successfully.

An operator command performs narrowly scoped revival by event ID, account, or
typed transient reason. The first production use revives Rabble's three current
events and any other account jobs failed by the expired-session signature error.

## Deletions And Addressable Events

The archive source includes kind `5` events. A deletion cancels an unpublished
job or deletes an existing mapped AT record. Replay remains idempotent.

For replaceable NIP-71 coordinates, current repository behavior remains the
source of truth: each signed event ID is independently mapped unless an existing
translation rule explicitly resolves it as a replacement. This project does not
invent new Nostr replacement semantics.

## Observability And Operations

`/metrics` exposes at least:

- pending, in-progress, transient-failed, and permanently-rejected jobs;
- oldest pending/transient job age;
- successful publishes and failures by typed reason;
- timestamp of last successful publish;
- live-source lag by kind;
- accounts pending/running/failed archive import;
- per-account archive scan progress totals;
- timestamp and outcome of the last reconciliation pass.

Readiness becomes degraded when publication makes no successful progress while
eligible jobs age beyond a configured threshold, or when live-source lag exceeds
its threshold. One malformed permanent event does not take the process out of
service.

Alerts target sustained publish lag, growing queue depth, failed archive scans,
and source reconciliation lag. The runbook includes safe job revival and
per-account audit commands without exposing credentials.

## Rollout

### Immediate remediation

1. Deploy merged image `e532ea1` to staging.
2. Exercise an expired account session and verify first-attempt proactive
   refresh, video upload, AT record creation, and persisted rotated session.
3. Promote the identical image digest to production.
4. Revive Rabble's three current jobs and all jobs with the same expired-session
   transient signature.
5. Verify the three source event IDs have AT mappings and playable Bluesky
   posts with their original timestamps.

### Durable rollout

1. Ship Funnelcake composite/ascending/author pagination with compatibility
   tests.
2. Ship database migration and typed job dispositions.
3. Enable lossless per-kind live scans.
4. Enable resumable full archive imports for one test account, then Rabble.
5. Enable scheduled reconciliation and observability.
6. Migrate existing ready accounts into archive sync without duplicating
   records.
7. Expand to all connected users after queue lag and error rates remain nominal.

Feature flags separate live cursor changes, archive import, and scheduled
reconciliation so each can be rolled back without disabling already working
crossposts.

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
- transient jobs remain retryable beyond the former attempt cap;
- permanent failures do not retry;
- reconciliation revives legacy terminal transient jobs;
- two users with interleaved events both converge;
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
4. A transiently failed job automatically recovers after its dependency is
   restored, even beyond the previous retry cap.
5. Scheduled reconciliation repairs an intentionally omitted live event.
6. Operators can see publish and archive lag and receive an alert before a user
   reports missing posts.
7. Staging and production use the same verified image digest for each promoted
   service revision.
