# Bluesky Crosspost — Launch Blockers (investigated 2026-06-01, updated 2026-07-13)

## Release gate update — 2026-07-13

Staging now publishes records, but the candidate that preceded this update is
not eligible for production promotion. A two-account canary proved that
`video.bsky.app` may return the same `already_exists` blob CID for identical
source bytes while that blob is retrievable only under the first account's DID.
AT blob identity is content-addressed, but availability is repository-scoped.

The next candidate must pass both of these gates:

1. **Per-repository video ownership.** Provision two isolated connected
   accounts and publish identical verified source bytes. Each account must have
   exactly one feed record, and
   `com.atproto.sync.getBlob?did=<that DID>&cid=<video BlobRef CID extracted from the record>` must return the
   bytes for both accounts. A video-service cache hit must trigger an
   authenticated `com.atproto.repo.uploadBlob` to the target account's PDS; its
   foreign cached `BlobRef` must never be placed in that account's record.
2. **Stable create identity.** Every queued event must have a non-null
   `publish_jobs.reserved_rkey` before media upload or PDS publication. Inject a
   crash after `createRecord` commits and before the mapping/job completion
   write. After lease expiry and retry, the event must retain the same AT URI,
   only one feed record may exist, and any ambiguous create failure—including
   rsky's generic duplicate-key HTTP 500—may be accepted only after `getRecord`
   returns the exact prepared record. Different content is
   `REMOTE_RECORD_DIVERGED` and must never be overwritten.
3. **Crash-stable prepared intent.** Before the first `createRecord`, the queue
   row must contain both `reserved_rkey` and the exact `prepared_record`. A retry
   must skip Blossom, transcoding, caption upload, and video upload, and reuse
   that JSON exactly. A different cache outcome must not change the blob CID
   selected by the first attempt.
4. **Writer and lease fencing.** Existing epoch-1 jobs remain quarantined until
   audited; epoch-2 workers only claim epoch-2 jobs, and the database rejects a
   claim without the epoch marker. Long jobs renew their lease. Completion and
   failure writes must match the current lease owner so a stale worker cannot
   overwrite a replacement worker's state. Ownership is a fresh UUID per claim,
   not a pod PID/name that can collide across replicas.

Required canary evidence:

```text
account A: exactly one AT URI; selected blob retrievable for DID A
account B: exactly one AT URI; selected blob retrievable for DID B
retry A after injected lost response: same AT URI; record count unchanged
retry A: fetch count unchanged; upload count unchanged; prepared JSON unchanged
legacy epoch-1 row: not claimed by epoch-2 worker; legacy claim rejected
expired session: refreshed; same URI/blob invariants
repeat reconciliation: zero unexplained differences
staging pod image digest: identical to the proposed production digest
```

Do not repair Rabble's archive or promote the image until every line above is
recorded against the candidate digest.

The B1–B4 material below is retained as the June incident record and is
superseded as current-state guidance. As of this update, production runs an
older atbridge image, the binary applies bridge-owned migrations at startup,
and the deployed configuration enables the video service. The active blockers
are the per-DID blob and stable-create release gates above, followed by the
staged reconciliation rollout in the RFC.

## Historical June investigation (superseded)

Investigation of why Bluesky crossposting is not working / not launchable. Evidence
gathered from the live `dv-platform-prod` cluster (kubectl), the IaC repo, and the
divine-sky source. Ranked by severity. **B1 is the confirmed primary cause.**

## STATUS UPDATE 2026-06-03 (after first IaC pass)

- **B1b (ENVIRONMENT placeholder) FIXED.** The live ExternalSecrets now request the
  correct `*-production` keys (verified: `divine-atbridge-relay-url-production`, no more
  literal `ENVIRONMENT`). The kustomize patch applied.
- **B1a (missing secrets) STILL OPEN — the remaining blocker.** ESO error is now
  `Secret does not exist` on the *correct* key names, i.e. the GCP Secret Manager
  secrets were never created. Confirmed NotFound (not `PERMISSION_DENIED`), so it's
  absent secrets, not IAM. ESO reads from project `dv-platform-prod`. All three
  ExternalSecrets are `SecretSyncedError / READY=False`; all three ArgoCD apps remain
  `OutOfSync / Degraded`; `sky` namespace still has zero pods.

### EXACT secrets to create in GCP Secret Manager (project `dv-platform-prod`)

Every one of these is currently missing. This is the complete checklist (pulled live
from each ExternalSecret's `remoteRef.key` list):

**divine-atbridge (10):**
`divine-atbridge-relay-url-production`, `divine-atbridge-pds-url-production`,
`divine-atbridge-pds-auth-token-production`, `divine-atbridge-blossom-url-production`,
`divine-atbridge-database-url-production`, `divine-atbridge-s3-endpoint-production`,
`divine-atbridge-s3-bucket-production`, `divine-atbridge-plc-directory-url-production`,
`divine-atbridge-handle-domain-production`,
`divine-atbridge-atproto-provisioning-token-production`

**divine-handle-gateway (7):**
`divine-handle-gateway-database-url-production`,
`divine-handle-gateway-keycast-atproto-token-production`,
`divine-handle-gateway-atproto-provisioning-url-production`,
`divine-handle-gateway-atproto-keycast-sync-url-production`,
`divine-handle-gateway-atproto-name-server-sync-url-production`,
`divine-handle-gateway-atproto-name-server-sync-token-production`,
`divine-handle-gateway-atproto-provisioning-token-production`

**rsky-pds (8):**
`rsky-pds-database-url-production`, `rsky-pds-jwt-secret-production`,
`rsky-pds-admin-password-production`, `rsky-pds-repo-signing-key-production`,
`rsky-pds-plc-rotation-key-production`, `rsky-pds-jwt-key-production`,
`rsky-pds-aws-access-key-id-production`, `rsky-pds-aws-secret-access-key-production`

After creating them, ESO refreshes hourly — force it with
`kubectl annotate externalsecret <name> -n sky force-sync=$(date +%s) --overwrite`
(or delete the ExternalSecret to trigger immediate recreation), then ArgoCD should
sync the Deployments and pods should appear.

### STAGING IS THE CONFIG TEMPLATE — but NOT proof of function (verified 2026-06-03)

CORRECTION: an earlier draft said "staging is the proven template / the bridge works."
That overstated it. Staging's CONTROL PLANE is healthy (pods Running, secrets synced),
so it's a valid template for config KEY NAMES/structure. But the DATA PLANE has never
run: staging `account_links`=0 (incl. pending), `publish_jobs`=0, published
`record_mappings`=0, and the staging relay `wss://relay.staging.dvines.org` is
currently DOWN (continuous connect failures). So no crosspost has ever succeeded in
any environment. Prove staging end-to-end first (see plan
`2026-06-03-atproto-production-promotion.md` Chunk F) before treating prod as a copy.
The `dv-platform-staging` cluster runs the full stack at the pod level:
```
$ kubectl --context <staging> get pods -n sky
divine-atbridge-...        1/1 Running (39h)
divine-handle-gateway-...  1/1 Running (2d22h)   (x2)
rsky-pds-...               1/1 Running (2d21h)
$ kubectl --context <staging> get externalsecret -n sky
divine-atbridge-runtime / handle-gateway / rsky-pds  → SecretSynced=True
```
So: the same secret KEY SET (with `-staging` suffix) already exists and works in
project `dv-platform-staging`. Promotion to prod = recreate that key set with
`-production` suffix in `dv-platform-prod` and populate with prod values. There is
NO bridge deployment in POC (`rich-compiler-479518-d2` / `dv-platform-poc`); only
`staging` and `production` overlays exist. Don't use POC as a reference.

### IMAGE PINNING INTERACTION (important for B2)

Staging runs image `…/containers-staging/divine-atbridge:3ba324c` — an OLDER build
that PREDATES the startup self-migration (B2). Staging's schema was therefore applied
out-of-band (hand `psql`). Implication for prod: the self-migration only runs if prod
is pinned to a build that contains the B2 commit (`048c293` or later). If prod ships
an older/`latest` image without it, the bridge will NOT self-migrate and the schema
must be applied manually (or pin prod to a B2-containing build — preferred). This is
the same image-pinning gap tracked in `2026-05-30-atproto-production-rollout.md`
Chunk B.

## B1 — CONFIRMED ROOT CAUSE: the bridge is not running in production

`divine-atbridge` (the service that performs Nostr→Bluesky crossposting) and
`divine-handle-gateway` have **zero pods** in prod. The `sky` namespace contains
only Services + ServiceAccounts; no Deployments are running.

Evidence:
```
$ kubectl get pods -n sky -l app.kubernetes.io/name=divine-atbridge
No resources found in sky namespace.

$ kubectl get applications -n argocd | grep -E 'atbridge|handle-gateway|rsky-pds'
divine-atbridge         OutOfSync   Degraded
divine-handle-gateway   OutOfSync   Degraded
rsky-pds                OutOfSync   Degraded
```

Causal chain (each step verified):
1. ArgoCD `divine-atbridge` SyncError: "one or more synchronization tasks completed
   unsuccessfully (retried 5 times)." The `Deployment` resource is `OutOfSync` and
   never became healthy.
2. The sync fails on the `ExternalSecret`: ArgoCD syncResult message
   `could not get secret data from provider`.
3. The External Secrets Operator cannot read the backing secrets from GCP Secret
   Manager. Live cluster events:
   ```
   externalsecret/divine-atbridge-runtime  UpdateFailed
     error processing spec.data[0] (key: divine-atbridge-relay-url-ENVIRONMENT),
     err: unable to access Secret from SecretManager Client: Secret does not exist
   externalsecret/divine-handle-gateway-runtime  ... divine-handle-gateway-database-url-ENVIRONMENT ... Secret does not exist
   externalsecret/rsky-pds-runtime  ... rsky-pds-database-url-production ... Secret does not exist
   ```
4. Because the `divine-atbridge-runtime` Secret is never created, the Deployment
   (which mounts it via `envFrom`) cannot start → no pods → no crossposting.

Two distinct problems are tangled here:
- **B1a (atbridge/handle-gateway): stale `ENVIRONMENT` placeholder.** The *live*
  ExternalSecret requests `divine-atbridge-relay-url-ENVIRONMENT` (literal
  `ENVIRONMENT`). The production overlay DOES contain a JSON-patch that rewrites the
  10 keys to `...-production`, and `kubectl kustomize .../overlays/production`
  renders correctly. So the desired manifest is right, but it has never been
  successfully applied — the live object is a leftover from an earlier partial sync,
  and ESO keeps failing on the unsubstituted key. ArgoCD is correctly pointed at
  `k8s/applications/divine-atbridge/overlays/production` on `main`.
- **B1b (rsky-pds): correct key, missing secret.** rsky-pds already requests
  `rsky-pds-database-url-production` (correctly substituted) but ESO still reports
  "Secret does not exist" — i.e. the secret was never created in Secret Manager.

**Fix (IaC + Secret Manager, NOT divine-sky code):**
1. Create the backing secrets in GCP Secret Manager for the `dv-platform-prod`
   project: all 10 `divine-atbridge-*-production`, the `divine-handle-gateway-*-production`
   set, and the `rsky-pds-*-production` set. (Use the ESO event list as the
   authoritative checklist of missing keys.)
2. Force an ArgoCD hard refresh/sync of `divine-atbridge`, `divine-handle-gateway`,
   `rsky-pds` so the corrected ExternalSecret (with `...-production` keys) replaces
   the stale `ENVIRONMENT` object. If the stale ExternalSecret blocks reconciliation,
   delete it and let ArgoCD recreate from the rendered overlay.
3. Verify: `kubectl get pods -n sky` shows running atbridge/handle-gateway pods and
   the ExternalSecrets go `SecretSynced=True`.

> Could not directly enumerate Secret Manager: the local `gcloud` is pointed at a
> different project (`rich-compiler-479518-d2`, not `dv-platform-prod`) — see the
> standing note about verifying gcloud/kubectl context after auth. The ESO
> in-cluster error is authoritative regardless (it uses the cluster workload-identity
> binding to the correct project).

## B2 — Schema migrations are never auto-applied to the bridge DB

There is **no migration automation** for the bridge database:
- No `db-migrate` Job in `divine-atbridge` or `divine-handle-gateway` IaC
  (keycast and divine-brain have one for their own DBs; the bridge does not).
- No embedded migration runner in the Rust code (no `diesel_migrations` /
  `sqlx::migrate` / dependency). `Dockerfile.atbridge` COPYs `migrations/` but
  `CMD ["divine-atbridge"]` never applies them.

So whoever bootstrapped the bridge DB applied `migrations/*/up.sql` by hand. After
PR #7 (oldest-first scheduler) the bridge REQUIRES the `004_publish_job_scheduler`
columns (`publish_jobs.nostr_pubkey`, `event_created_at`, `job_source`, lease cols;
`account_links.publish_backfill_state`). Reproduced locally: against a DB with only
`001` applied, the bridge's live-claim query fails with
`column "nostr_pubkey" does not exist`.

Once B1 is fixed and pods start, confirm the prod bridge DB has 004 + 005 applied
(`\d publish_jobs` should show `job_source`/`lease_owner`; `\d account_links` should
show `publish_backfill_state` and `crosspost_enabled` default TRUE). If not, apply
them. **Latent footgun:** two migration dirs share the `004_` prefix
(`004_provisioning_keys`, `004_publish_job_scheduler`) — harmless under a glob
applier (they sort deterministically and both run) but ambiguous under any
version-tracking runner. Worth renaming `004_publish_job_scheduler` → `005_` (and
bumping the existing `005_crosspost_default_true` → `006_`) IF a tracking runner is
ever adopted; do NOT rename under a glob/manual applier without `IF NOT EXISTS`
guards, since the columns may already be applied in prod.

## B3 — Video routing likely disabled → crossposts may publish but not play

`VIDEO_SERVICE_ENABLED` is **not set** in the atbridge deployment env (any overlay),
so `config.rs` defaults it to `false`. Per prior findings
(`memory: rsky-pds-video-upload-blocker`), uploading video blobs directly to the PDS
(bypassing `video.bsky.app`) results in "Video not found" on playback. So even after
B1/B2, video crossposts may post but be unplayable on Bluesky. Set
`VIDEO_SERVICE_ENABLED=true` (+ `VIDEO_SERVICE_URL` if non-default) in the atbridge
deployment before launch, and verify a real video plays end-to-end.

## B4 — (from earlier audit) PDS entryway env unset

`PDS_OAUTH_AUTHORIZATION_SERVER` / `PDS_ENTRYWAY_DID` are unset in rsky-pds IaC, so
`/.well-known/oauth-protected-resource` 404s and entryway token-trust is inert. This
blocks the third-party ATProto *login* path, not crossposting directly, but is on the
same launch checklist. See `2026-05-30-atproto-production-rollout.md` Chunk B.

## Launch order
B1 (start the bridge) → B2 (confirm schema) → B3 (enable video routing) → verify one
real crosspost end-to-end → then B4 for the login surface.
```
```
