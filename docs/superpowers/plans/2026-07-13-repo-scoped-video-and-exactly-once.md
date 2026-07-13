# Repo-Scoped Video and Exactly-Once Publication Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure cached videos remain retrievable for every target DID and every eligible Nostr event creates at most one AT feed record across retries and crashes.

**Architecture:** Treat video-service `already_exists` as a processing-cache result, not proof of target-repository blob ownership. Fall back to an authenticated direct PDS upload for that DID. Before the first create, persist both a durable TID and the exact prepared AT record on the queue row; every retry reuses both and skips media work. Epoch-fenced claims prevent an overlapping legacy auto-rkey writer from consuming new work, while renewable owner-fenced leases prevent stale workers from changing queue state. Ambiguous creates are recovered only when the record at the reserved URI exactly matches the persisted prepared record.

**Tech Stack:** Rust 2021, Diesel/PostgreSQL, ATProto XRPC, rsky PDS, Bluesky video service, mockito, Kubernetes staging canaries.

**Implementation status (2026-07-13):** Tasks 1–6 are implemented locally and
under verification. The original rkey-only design was rejected in adversarial
review because a fresh optimized upload and cached retry could select different
blob CIDs. The implementation now persists and reuses the exact prepared JSON,
fences writer generations and lease owners, and includes a DB-backed
commit/lost-response/retry test that proves media work occurs once. Task 7's
live staging canary remains mandatory. No production promotion has occurred.

---

## File map

- Modify `crates/divine-atbridge/src/video_service.rs`: distinguish fresh and cached video outcomes and route cached results through the target PDS.
- Create `crates/divine-atbridge/src/tid.rs`: generate standards-compatible monotonic AT TIDs.
- Modify `crates/divine-atbridge/src/lib.rs`: export the TID module.
- Create `migrations/008_publish_job_reserved_rkey/up.sql` and `down.sql`: persist the rkey, exact prepared record, and writer epoch; install the claim fence without a table-scanning index.
- Modify `crates/divine-bridge-db/src/migrations.rs`, `schema.rs`, `models.rs`, and `queries.rs`: atomically reserve/read prepared intent, fence writer generations, renew leases, and owner-fence finalization.
- Modify `crates/divine-atbridge/src/pipeline.rs`: carry durable prepared intent through queued execution, skip media work on retry, and use create-only publication.
- Modify `crates/divine-atbridge/src/publisher.rs`: send caller-supplied `createRecord.rkey` and recover matching existing records.
- Modify `crates/divine-atbridge/src/runtime.rs`: reserve the TID after claim, heartbeat long leases, and finalize only as the current owner.
- Modify `crates/divine-atbridge/tests/post_record_contract.rs`, `publish_path_integration.rs`, and `runtime_resilience.rs`: prove repo ownership and exactly-once crash recovery.
- Modify `docs/runbooks/bluesky-crosspost-launch-blockers.md`: add per-DID blob and duplicate-record release gates.

## Task 1: Make cached video fallback repository-safe — complete

**Files:**
- Modify: `crates/divine-atbridge/src/video_service.rs`

- [ ] **Step 1: Write the failing immediate-cache test**

Replace the cached-blob happy-path assertion with
`cached_video_is_uploaded_directly_to_target_pds`. Mock a `409 already_exists`
response containing `bafkreiotherrepo`, then require exactly one authenticated
`POST /xrpc/com.atproto.repo.uploadBlob` containing the original bytes and return
`bafkreitargetrepo`. Assert the uploader returns `bafkreitargetrepo`, never the
cross-repository CID.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge video_service::tests::cached_video_is_uploaded_directly_to_target_pds -- --exact
```

Expected: FAIL because the current implementation returns
`bafkreiotherrepo` and never calls the target PDS.

- [ ] **Step 3: Represent cache provenance explicitly**

Change `VideoUploadOutcome` to distinguish `FreshJob(String)`,
`CachedJob(String)`, and `CachedBlob(BlobRef)`. A non-success
`already_exists` response is cached even when it contains a blob. A successful
new response remains fresh.

- [ ] **Step 4: Implement the direct-PDS fallback**

For `CachedBlob`, call:

```rust
self.pds_client
    .upload_blob_for_did(data, mime_type, user_did)
    .await
    .context("failed to upload cached video source to target PDS")
```

Log the target DID and safe outcome only. Do not log service tokens, source URLs,
or response bodies.

- [ ] **Step 5: Run GREEN**

```bash
cargo test -p divine-atbridge video_service::tests::cached_video_is_uploaded_directly_to_target_pds -- --exact
```

Expected: PASS; the PDS upload mock is called once and the returned CID is owned
by the target repo.

## Task 2: Cover cached job-status fallback without regressing fresh jobs — complete

**Files:**
- Modify: `crates/divine-atbridge/src/video_service.rs`

- [ ] **Step 1: Write the failing cached-job test**

Update `upload_409_already_exists_with_job_id_resolves_via_job_status` so the
status endpoint returns `bafkreicachedvideo`, the target PDS upload returns
`bafkreitargetvideo`, and the final assertion requires
`bafkreitargetvideo`.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge upload_409_already_exists_with_job_id_resolves_via_job_status -- --exact
```

Expected: FAIL because the cached job result is currently returned directly.

- [ ] **Step 3: Implement cached-job fallback**

Poll cached jobs for diagnostics/completion, but after a blob is returned upload
the verified input bytes directly to the target PDS and use only that PDS
response. Fresh jobs continue returning the optimized blob uploaded by the
video service.

- [ ] **Step 4: Prove the fresh path does not double-upload**

Add `fresh_video_job_does_not_call_direct_pds_upload`. A successful new job and
completed status must return the optimized CID while a zero-call PDS upload mock
remains unmatched.

- [ ] **Step 5: Run GREEN and commit**

```bash
cargo test -p divine-atbridge video_service::tests -- --nocapture
git add crates/divine-atbridge/src/video_service.rs
git commit -m "fix(atbridge): scope cached videos to target repos"
```

Expected: all video-service tests PASS.

## Task 3: Persist one TID per queued source event — complete

**Files:**
- Create: `crates/divine-atbridge/src/tid.rs`
- Modify: `crates/divine-atbridge/src/lib.rs`
- Create: `migrations/008_publish_job_reserved_rkey/up.sql`
- Create: `migrations/008_publish_job_reserved_rkey/down.sql`
- Modify: `crates/divine-bridge-db/src/migrations.rs`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`

- [ ] **Step 1: Write failing TID and migration tests**

Add unit tests proving `next_tid()` returns 13 lowercase sortable-base32
characters and is strictly monotonic for 10,000 calls. Add a DB test proving
`reserve_publish_job_rkey(event_id, candidate)` returns the first candidate on
every later call. The Nostr event ID primary key supplies queue uniqueness.
Rkeys are repository-scoped, so reservation uses an advisory lock plus a
same-linked-account collision check instead of a queue-wide unique index.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge tid::tests -- --nocapture
cargo test -p divine-bridge-db reserve_publish_job_rkey -- --nocapture
```

Expected: FAIL because the module, column, and query do not exist.

- [ ] **Step 3: Add the additive migration**

`up.sql`:

```sql
ALTER TABLE publish_jobs
  ADD COLUMN IF NOT EXISTS reserved_rkey TEXT,
  ADD COLUMN IF NOT EXISTS prepared_record JSONB,
  ADD COLUMN IF NOT EXISTS writer_epoch INTEGER NOT NULL DEFAULT 1;
```

`down.sql` drops the trigger/function and additive columns. Existing epoch-1
jobs remain quarantined until explicitly audited.

- [ ] **Step 4: Implement the official TID shape**

Use the ATProto sortable alphabet `234567abcdefghijklmnopqrstuvwxyz`. Encode a
monotonic microsecond timestamp to 11 characters and a process clock ID to two
characters. Use an atomic last timestamp and a process-random clock ID; database
the event-keyed reservation is the retry identity fence.

- [ ] **Step 5: Implement first-writer-wins reservation**

Add:

```rust
pub fn reserve_publish_job_rkey(
    conn: &mut PgConnection,
    nostr_event_id: &str,
    candidate: &str,
) -> Result<String>
```

It performs one parameterized update using
`reserved_rkey = COALESCE(reserved_rkey, $2)` and returns the stored value.
Prepared JSON uses the same first-writer-wins operation before any create.

- [ ] **Step 6: Run GREEN and commit**

```bash
cargo test -p divine-atbridge tid::tests -- --nocapture
cargo test -p divine-bridge-db reserve_publish_job_rkey -- --nocapture
git add migrations/008_publish_job_reserved_rkey crates/divine-bridge-db crates/divine-atbridge/src/{tid.rs,lib.rs}
git commit -m "feat(atbridge): reserve stable publish TIDs"
```

## Task 4: Publish queued jobs create-only at their reserved URI — complete

**Files:**
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Modify: `crates/divine-atbridge/src/publisher.rs`
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/tests/post_record_contract.rs`
- Modify: `crates/divine-atbridge/tests/publish_path_integration.rs`

- [ ] **Step 1: Write the failing request-contract test**

Change the queued publish contract to require:

```json
{
  "repo": "did:plc:integration",
  "collection": "app.bsky.feed.post",
  "rkey": "3lp5l4phu2k2w",
  "validate": true,
  "record": {"$type": "app.bsky.feed.post"}
}
```

Assert exactly one create request and no `putRecord` request.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge --test post_record_contract queued_video_posts_use_reserved_tid -- --exact
```

Expected: FAIL because queued creates omit `rkey`.

- [ ] **Step 3: Carry the reservation through execution**

Add `reserved_rkey: Option<String>` to `PublishJobEnvelope`. Queue construction
uses `None`; `publish_job_envelope` copies the persisted value. Immediately
after claiming a job, runtime reserves `next_tid()` when null, reloads the job,
and only then invokes media or PDS code.

- [ ] **Step 4: Add create-only publisher support**

Add `create_record_at_rkey_with_meta(did, collection, rkey, record)` to
`PdsPublisher` and `PdsClient`. It calls `com.atproto.repo.createRecord` with the
caller-supplied TID. Queued video execution requires `Some(rkey)`; direct unit
pipeline calls may retain the legacy auto-rkey helper only outside runtime.

- [ ] **Step 5: Run GREEN and commit**

```bash
cargo test -p divine-atbridge --test post_record_contract -- --nocapture
cargo test -p divine-atbridge --test publish_path_integration -- --nocapture
git add crates/divine-atbridge/src/{pipeline.rs,publisher.rs,runtime.rs} crates/divine-atbridge/tests/{post_record_contract.rs,publish_path_integration.rs}
git commit -m "fix(atbridge): create posts at reserved TIDs"
```

## Task 5: Recover lost responses without duplicate records — complete

**Files:**
- Modify: `crates/divine-atbridge/src/publisher.rs`
- Modify: `crates/divine-atbridge/tests/runtime_resilience.rs`

- [ ] **Step 1: Write the failing lost-response test**

Model attempt one as a successful PDS create followed by local mapping failure.
Attempt two receives an ambiguous create failure for the same rkey (including
rsky's generic HTTP 500 for an existing key). Mock
`com.atproto.repo.getRecord` at the exact repo/collection/rkey and return the
same canonical record. Assert the final mapping uses that URI and the PDS
observed only one successful create side effect.

- [ ] **Step 2: Run RED**

```bash
cargo test -p divine-atbridge --test runtime_resilience pds_commit_then_mapping_failure_recovers_same_record -- --exact
```

Expected: FAIL because the retry currently allocates a remote rkey or treats the
conflict as failure.

- [ ] **Step 3: Implement exact-record recovery**

After any ambiguous create failure, call `getRecord` for the reserved URI.
Normalize rsky's response wrapper, compare the returned record to the exact
prepared JSON value, and return its URI/CID only on equality. Missing records
preserve the original retryable error; different records fail with
`REMOTE_RECORD_DIVERGED` and are never overwritten.

- [ ] **Step 4: Add divergence and replay tests**

Add `existing_reserved_record_with_different_content_is_not_overwritten` and
`successful_job_replay_creates_no_second_record`.

- [ ] **Step 5: Run GREEN and commit**

```bash
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-atbridge publisher -- --nocapture
git add crates/divine-atbridge/src/publisher.rs crates/divine-atbridge/tests/runtime_resilience.rs
git commit -m "fix(atbridge): recover reserved record writes"
```

## Task 6: Persist prepared intent and fence concurrent writers — complete

**Files:**
- Modify: `migrations/008_publish_job_reserved_rkey/up.sql`
- Modify: `crates/divine-bridge-db/src/{models.rs,queries.rs,schema.rs}`
- Modify: `crates/divine-atbridge/src/{pipeline.rs,runtime.rs}`
- Modify: `crates/divine-atbridge/tests/{publish_queue_scheduler.rs,runtime_resilience.rs}`

- [x] Persist the exact prepared AT record before the first create. A retry
  loads it from the queue and performs no Blossom, transcode, caption, or upload
  work.
- [x] Keep the schema default at writer epoch 1. New enqueue transactions
  promote their safe rows to epoch 2. New claim SQL selects epoch 2 and
  supplies a transaction-local marker checked by a database trigger, so an old
  binary cannot claim new work and old work cannot be silently upgraded.
- [x] During overlap, adopt an epoch-1 row only when it is provably pristine:
  pending, attempt zero, no lease, reservation, prepared record, or completion.
  This lets the new ingester recover an old ingester's harmless insert without
  adopting any job that might already have a remote side effect.
- [x] Quarantine pre-release epoch-1 rows for explicit shadow audit instead of
  risking a second create for an unknown legacy side effect.
- [x] Promote only the existing bounded BadJwt repair class to epoch 2 when an
  operator confirms repair; those failures occurred before create, so they do
  not carry an ambiguous remote side effect.
- [x] Extend the lease to ten minutes, heartbeat it every minute, and require
  a fresh UUID claim token for renewal, completion, and failure transitions.
  The token is unique even when multiple Kubernetes replicas share lane and PID.
- [x] Add DB tests for prepared-record first-writer semantics, writer fencing,
  and lease ownership. Add a runtime test for PDS commit, lost response, retry,
  unchanged prepared JSON/rkey, one media fetch/upload, and final mapping.
- [x] Fail closed if any queued/legacy relay video reaches execution without a
  durable reserved rkey; the unreserved helper can no longer create a post.
- [x] Replace the queue-wide unique rkey index with an advisory-lock-protected
  same-account collision check, avoiding a table-scanning startup index while
  respecting that AT rkeys are scoped to a repository.

## Task 7: Verify the complete release gate — in progress

**Files:**
- Modify: `docs/runbooks/bluesky-crosspost-launch-blockers.md`

- [ ] **Step 1: Update the runbook**

Document a staging canary that provisions two isolated connected accounts,
publishes identical verified video bytes, verifies one feed record per event,
and calls `sync.getBlob` with each target DID/CID. Add the crash-after-create
fault test and repeat reconciliation zero-diff check.

- [ ] **Step 2: Run focused and workspace verification**

```bash
cargo fmt --all -- --check
cargo test -p divine-atbridge video_service::tests -- --nocapture
cargo test -p divine-atbridge --test post_record_contract -- --nocapture
cargo test -p divine-atbridge --test runtime_resilience -- --nocapture
cargo test -p divine-bridge-db
bash scripts/test-workspace.sh
```

Expected: all commands PASS.

- [ ] **Step 3: Rebuild and stage**

Build one immutable image from the verified commit, deploy it to staging through
the IaC `newTag` change, and record its digest. Never rebuild for production.

- [ ] **Step 4: Run the two-account staging canary**

Require:

```text
account A: exactly one AT URI, blob retrievable for DID A
account B: exactly one AT URI, blob retrievable for DID B
retry A: same AT URI, no additional record
expired session: refreshed, same invariants
repair preview after completion: zero matching rows
```

- [ ] **Step 5: Promote the identical digest**

Copy the tested manifest to the production registry, merge a separate production
IaC PR, verify the pod image ID equals the staging digest, then run bounded Rabble
repair and AppView/HLS playback checks.
