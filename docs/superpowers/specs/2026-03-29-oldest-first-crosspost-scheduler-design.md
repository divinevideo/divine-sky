# Oldest-First Crosspost Scheduler Design

## Goal

When a Divine user becomes eligible for ATProto crossposting, the historical NIP-71 publish backlog for that user should be enqueued and published oldest first. New live posts for that same user may still publish immediately and are allowed to overtake unfinished backlog work.

## Problem

`divine-atbridge` currently publishes directly from the relay-consumer path. That creates three problems for migrated users:

- there is no durable queue between relay ingest and PDS writes
- there is no per-user backlog replay phase when an account first becomes publishable
- the relay cursor currently advances only after publish work completes, so the runtime couples ingest durability to downstream publish latency

The repo already has a `publish_jobs` table, but it is effectively a stub. It is not rich enough to drive a real scheduler or express oldest-first backlog ordering.

## Constraints

- historical backlog order matters within the migrated backlog
- new live posts should not wait behind backlog posts
- duplicate relay delivery and repeated backfill seeding must stay safe
- existing `record_mappings` remain the source of truth for what has actually published
- delete events must not allow a queued-but-never-published post to leak through later
- this feature should stay inside `divine-sky`; it should not assume upstream control-plane changes

## Approaches Considered

### 1. Strict per-user gate

Block all new posts for a user until their backlog is fully drained.

Pros:

- strongest chronological guarantee

Cons:

- violates the product decision that new posts may publish immediately
- increases perceived latency for newly active users

### 2. Minimal dual-path backlog replay

Add a best-effort replay worker for migrated users while keeping live publishing mostly unchanged.

Pros:

- smaller change
- faster to ship

Cons:

- keeps queueing and publish durability half-implemented
- duplicates logic between live and replay paths
- makes deletion and retries harder to reason about

### 3. Durable scheduler for live and backlog publish tasks

Turn `publish_jobs` into the real scheduler, enqueue live relay events durably, and add a backlog planner that seeds per-user historical jobs oldest first.

Pros:

- one durable surface for retries, ordering, and observability
- clean separation between ingest and expensive publish work
- makes backlog replay idempotent and restart-safe

Cons:

- larger refactor
- requires queue semantics, planner state, and worker orchestration

### Recommendation

Use option 3. The user explicitly chose the durable scheduler path, and it is the only approach that cleanly supports oldest-first backlog replay without blocking live publishing.

## Architecture

### 1. Expand `publish_jobs` into the scheduler

Add the metadata needed to drive ordering and worker ownership:

- `nostr_pubkey`
- `event_created_at`
- `event_payload`
- `job_source` with `live` and `backfill`
- `lease_owner`
- `lease_expires_at`
- `completed_at`

Keep `nostr_event_id` as the primary key so duplicate enqueue attempts collapse onto one durable job per source event.
Persisting `event_payload` in the queue is required. The worker cannot safely reconstruct the original Nostr event from `nostr_event_id` alone after relay ingest has moved on.

### 2. Add durable account backfill state

Track whether a publishable account has had its historical backlog seeded:

- `publish_backfill_state`
- `publish_backfill_started_at`
- `publish_backfill_completed_at`
- `publish_backfill_error`

These fields live on `account_links` because backlog seeding is fundamentally tied to account lifecycle state.

Existing lifecycle queries and test fixtures that materialize `AccountLinkLifecycleRow` must be updated in lockstep with these columns so `bridge_opt_in_gate`, provisioning tests, and raw-SQL lifecycle helpers keep compiling and reading the right shape.

### 3. Split ingest from publish execution

The relay ingest path should:

- validate the event enough to know whether it is enqueueable
- look up account eligibility
- persist or upsert a `publish_jobs` row
- advance the relay cursor only after enqueue succeeds

The publish workers should:

- claim jobs using a lease
- run one live lane that only claims `live` jobs
- run one backlog lane that only claims `backfill` jobs
- process `backfill` jobs oldest first by `event_created_at`
- run the heavy pipeline work that fetches blobs, uploads assets, writes PDS records, and stores mappings

### 4. Add a per-user backlog planner

The planner scans `account_links` for rows that are both:

- `crosspost_enabled = true`
- `provisioning_state = 'ready'`
- `publish_backfill_state IN ('not_started', 'failed')`

For each eligible user, it opens an author-filtered relay subscription for historical publish and delete kinds, reads until `EOSE`, sorts the stored history by `(created_at, id)`, and then replays that ordered history into the scheduler:

- publish events become `backfill` jobs
- delete events cancel queued historical jobs that have not published yet

When the historical scan completes, it marks the account backlog state complete.

Repeated planner runs are safe because enqueue is idempotent on `nostr_event_id`.

## Ordering Semantics

The scheduler guarantees:

- `backfill` jobs for a user are published oldest first by source event timestamp
- `live` jobs publish through a separate live lane and may overtake queued `backfill` jobs

The scheduler does not guarantee a perfectly chronological merged timeline between backlog and live traffic. A live post created after migration can appear on ATProto before an older backlog post that is still draining. That trade-off is intentional.

## Delete Handling

Delete handling has two branches:

- if the target event already has a `record_mapping`, use the existing ATProto delete path
- if the target event only has a queued publish job, mark that job `skipped` or canceled so the create never publishes later
- if the target event has no queued row yet, create a tombstone `publish_jobs` row in `skipped` state so later live or backlog enqueue attempts cannot resurrect it
- if the delete is observed during historical backlog replay, apply the same cancellation or tombstone rule before the backlog worker ever sees that create

The system should not emit an ATProto delete for a post that never created an ATProto record.

## Failure Handling

- Relay cursor durability moves from "publish succeeded" to "enqueue persisted."
- Claimed jobs whose lease expires return to the queue for retry.
- Backfill planner crashes are safe because re-seeding is idempotent.
- Tombstone rows created by deletes must win over later enqueue attempts for the same `nostr_event_id`.
- Worker failures increment `attempt`, preserve the last error, and leave the job retryable.
- Account backlog state moves to `failed` only when the planner itself fails, not when a single publish job fails.

## Testing Strategy

Add coverage for:

- queue ordering and lease/claim behavior
- queue payload durability and tombstone semantics
- relay cursor advancement after enqueue, not after publish
- backfill planner seeding a user oldest first
- live and backlog lanes progressing without starvation
- delete events canceling queued jobs before publish
- lifecycle-row fixtures and DB reset helpers after the schema expansion
- end-to-end runtime behavior with live and backfill work interleaving safely

## Non-Goals

- perfect merged chronology between live and backlog traffic
- moving profile sync onto the scheduler in this slice
- distributed multi-instance work partitioning beyond lease-based safety
- changing upstream user-consent or provisioning APIs outside this repo
