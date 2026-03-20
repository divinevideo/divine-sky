# Staging ATProto End-To-End Remediation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the staging ATProto flow fully operational by fixing the `rsky-pds` DID-document-resolution blocker, deploying the current `divine-sky` callback path, publishing ATProto handle state through `divine-name-server`, serving `/.well-known/atproto-did` from `divine-router`, and wiring the real user-facing keycast control plane into staging.

**Architecture:** Work the critical path in order: `rsky-pds` must successfully finish provisioning first; only then can `divine-handle-gateway` mark links `ready` and push state to keycast and the name-server. The name-server becomes the public read-model publisher into Fastly KV, and `divine-router` stays read-only at the edge, serving `/.well-known/atproto-did` only for `active + ready` users. Keep feedgen and labeler out of scope except for final regression checks; they are not on the account-provisioning path.

**Tech Stack:** Rust (`axum`, `reqwest`, Fastly Compute), TypeScript (`hono`, Cloudflare Workers, Vitest), Kubernetes/Kustomize/ArgoCD, GCP Secret Manager, Fastly KV, `rsky-pds`, Bash.

---

## Scope And Boundaries

- `rsky-pds` is the immediate blocker. Until `createAccount` succeeds end-to-end, `divine-router` and `divine-name-server` cannot make staging “fully work”.
- `divine-router` and `divine-name-server` are separate edge/serverless deployments, not `divine-sky` GKE services. Their rollout path is independent of ArgoCD for `sky`.
- `keycast` is required for the real user-facing enable/status/disable loop. If it is not updated and deployed, staging may work only through internal service calls, not through the real product flow.
- `divine-feedgen` and `divine-labeler` are not required for DID creation, DID resolution, or `/.well-known/atproto-did`. They get regression-checked at the end only.

## Success Criteria

- `POST /xrpc/com.atproto.server.createAccount` succeeds on staging `rsky-pds` for a fresh canary handle and PLC DID.
- Staging `divine-handle-gateway` marks the link `ready` and successfully syncs keycast + name-server after provisioning.
- `divine-name-server` stores and republishes `atproto_did` and `atproto_state` into Fastly KV, and its hourly reconciliation preserves those fields.
- `divine-router` returns the bare DID from `https://username.divine.video/.well-known/atproto-did` only for `active + ready` usernames, and `404` for `pending`, `failed`, or `disabled`.
- Keycast `/api/user/atproto/enable`, `/status`, `/disable`, and `/internal/atproto/state` are live in staging and use the real `divine-sky` control-plane URL and service token.
- The full smoke flow passes: claim username, opt in, provisioning reaches `ready`, `/.well-known/atproto-did` resolves, mirrored publish succeeds, disable removes public DID resolution.

## Chunk 1: Unblock PDS Provisioning

### Task 1: Add A Reproducible Staging PDS Canary Smoke Flow

**Files:**
- Create: `scripts/staging-pds-did-smoke.sh`
- Create: `docs/runbooks/staging-pds-did-resolution.md`
- Modify: `docs/runbooks/divine-sky-staging-status.md`

- [ ] **Step 1: Write the failing staging smoke script**

Create a Bash smoke script that requires:

```bash
PDS_URL
PDS_ADMIN_PASSWORD
CANARY_HANDLE
CANARY_DID
```

and runs:

```bash
curl -fsS "$PDS_URL/xrpc/_health"
curl -i -X POST "$PDS_URL/xrpc/com.atproto.server.createAccount" \
  -H "Authorization: Bearer $PDS_ADMIN_PASSWORD" \
  -H "Content-Type: application/json" \
  -d "{\"did\":\"$CANARY_DID\",\"handle\":\"$CANARY_HANDLE\"}"
curl -i "$PDS_URL/xrpc/com.atproto.repo.describeRepo?repo=$CANARY_DID" \
  -H "Authorization: Bearer $PDS_ADMIN_PASSWORD"
```

The script must print response status and body for each step and exit non-zero on the first failure.

- [ ] **Step 2: Run the smoke script against staging and verify it reproduces the current failure**

Run:

```bash
PDS_URL=https://pds.staging.dvines.org \
PDS_ADMIN_PASSWORD=... \
CANARY_HANDLE=atproto-canary-$(date +%s).staging.dvines.org \
CANARY_DID=did:plc:... \
bash scripts/staging-pds-did-smoke.sh
```

Expected: FAIL during or immediately after `createAccount`, with evidence that the PDS is attempting DID-document resolution incorrectly.

- [ ] **Step 3: Write the runbook with a captured failing example**

Document:
- required env vars
- exact commands
- the expected failing response before the fix
- the expected success response after the fix
- where to look next if the failure changes shape

- [ ] **Step 4: Re-run the smoke flow once after documenting it**

Run: `bash scripts/staging-pds-did-smoke.sh`

Expected: same failure as Step 2, now captured in a committed runbook.

- [ ] **Step 5: Commit**

```bash
git add scripts/staging-pds-did-smoke.sh docs/runbooks/staging-pds-did-resolution.md docs/runbooks/divine-sky-staging-status.md
git commit -m "docs: add staging pds did-resolution smoke flow"
```

### Task 2: Patch Or Pin A Working `rsky-pds` Image For Staging

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/base/deployment.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/staging/kustomization.yaml`
- External: forked `rsky-pds` repository source file(s) for DID-document lookup, identified from the smoke output in Task 1

- [ ] **Step 1: Use the smoke output to isolate the exact `rsky-pds` DID-resolution code path**

From the failing response and logs, identify the handler or client in the forked `rsky-pds` repo that resolves `did:plc:...` documents. Record the exact file path in the fork before changing code.

- [ ] **Step 2: Patch the fork and build a staging image tag**

Build and publish a non-`latest` image tag, for example:

```bash
did-doc-fix-<short-sha>
```

The patch must make the smoke flow from Task 1 pass without manual post-processing of the DID string.

- [ ] **Step 3: Stop using `latest` for staging and pin the patched image tag**

Update:
- `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/staging/kustomization.yaml`
- `../divine-iac-coreconfig/k8s/applications/rsky-pds/base/deployment.yaml`

Make these changes:

```yaml
images:
  - name: rsky-pds
    newName: us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds
    newTag: did-doc-fix-<short-sha>
```

Also remove the shell `command` override from the base deployment if the patched image no longer needs it; keep the explicit env-driven port and probes.

- [ ] **Step 4: Render the staging manifests before syncing**

Run:

```bash
cd ../divine-iac-coreconfig
kustomize build k8s/applications/rsky-pds/overlays/staging >/tmp/rsky-pds-staging.yaml
```

Expected: exit code `0`.

- [ ] **Step 5: Sync staging and rerun the canary smoke**

Run the normal ArgoCD/coreconfig deployment flow for `rsky-pds`, then:

```bash
cd /Users/rabble/code/divine/divine-sky
bash scripts/staging-pds-did-smoke.sh
```

Expected: PASS. `createAccount` succeeds and `describeRepo` returns the canary DID and handle.

- [ ] **Step 6: Commit the deployment pin**

```bash
cd ../divine-iac-coreconfig
git add k8s/applications/rsky-pds/base/deployment.yaml k8s/applications/rsky-pds/overlays/staging/kustomization.yaml
git commit -m "fix: pin staging rsky-pds image with did resolution patch"
```

## Chunk 2: Deploy The Current `divine-sky` Callback Path

### Task 3: Verify And Deploy The `divine-handle-gateway` / `divine-atbridge` Images Used By Staging

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/staging/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml`
- Test: `crates/divine-handle-gateway/tests/control_plane.rs`
- Test: `crates/divine-atbridge/tests/provision_api.rs`
- Test: `crates/divine-atbridge/tests/provisioning_lifecycle.rs`

- [ ] **Step 1: Run the focused `divine-sky` control-plane tests locally**

Run:

```bash
cargo test -p divine-handle-gateway --test control_plane -- --nocapture
cargo test -p divine-atbridge --test provision_api -- --nocapture
cargo test -p divine-atbridge --test provisioning_lifecycle -- --nocapture
```

Expected: PASS. These tests prove the staging images should contain the callback logic for pending, ready, failed, and disabled transitions.

- [ ] **Step 2: Build and publish immutable staging images**

Build and push new staging image tags for:
- `divine-handle-gateway`
- `divine-atbridge`

Do not reuse `latest`; produce immutable tags tied to the source commit.

- [ ] **Step 3: Replace `latest` in both staging overlays**

Update:
- `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/staging/kustomization.yaml`
- `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml`

with:

```yaml
newTag: <immutable-sha-tag>
```

- [ ] **Step 4: Render both overlays before syncing**

Run:

```bash
cd ../divine-iac-coreconfig
kustomize build k8s/applications/divine-handle-gateway/overlays/staging >/tmp/divine-handle-gateway-staging.yaml
kustomize build k8s/applications/divine-atbridge/overlays/staging >/tmp/divine-atbridge-staging.yaml
```

Expected: both commands exit `0`.

- [ ] **Step 5: Sync staging and verify the callback path**

After the deployment:
- opt a canary user in through keycast or the internal control-plane entrypoint
- confirm the `account_links` row reaches `ready`
- confirm the handle-gateway logs show success for keycast sync and name-server sync, not just local DB mutation

- [ ] **Step 6: Commit the staging image bumps**

```bash
cd ../divine-iac-coreconfig
git add k8s/applications/divine-handle-gateway/overlays/staging/kustomization.yaml k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml
git commit -m "chore: pin staging divine-sky control-plane images"
```

## Chunk 3: Publish ATProto Handle State Through `divine-name-server`

### Task 4: Land And Deploy The Internal ATProto Sync Endpoint

**Files:**
- Modify: `../divine-name-server/src/index.ts`
- Create: `../divine-name-server/src/routes/internal-atproto.ts`
- Create: `../divine-name-server/src/routes/internal-atproto.test.ts`
- Modify: `../divine-name-server/migrations/0008_reserve_atproto_service_subdomains.sql`
- Modify: `../divine-name-server/src/utils/subdomain.ts`

- [ ] **Step 1: Write failing tests for the internal sync route**

Add tests for:
- valid bearer token + valid payload updates `atproto_did` and `atproto_state`
- missing token returns `401`
- unknown username returns `404`
- invalid DID or invalid state returns `400`

Minimal happy-path assertion:

```ts
expect(response.status).toBe(200)
expect(json.atproto_state).toBe('ready')
expect(syncUsernameToFastly).toHaveBeenCalledTimes(1)
```

- [ ] **Step 2: Run the focused Vitest file and verify the missing behavior**

Run:

```bash
cd ../divine-name-server
npm run test:once -- src/routes/internal-atproto.test.ts
```

Expected: FAIL if the route is missing, not mounted, or not validating auth/payload correctly.

- [ ] **Step 3: Implement and mount the route**

Implement:

```ts
POST /api/internal/username/set-atproto
```

Rules:
- require `Authorization: Bearer <ATPROTO_SYNC_TOKEN>`
- look up the canonical username
- update `atproto_did` and `atproto_state` in D1
- push the updated read model to Fastly KV

Also reserve service subdomains required for ATProto and edge routing:

```sql
('pds', 'system', 'ATProto PDS host', unixepoch()),
('feed', 'system', 'ATProto feed generator host', unixepoch()),
('labeler', 'system', 'ATProto labeler host', unixepoch())
```

and ensure the subdomain helper excludes those labels from user-profile routing.

- [ ] **Step 4: Re-run the focused tests**

Run:

```bash
cd ../divine-name-server
npm run test:once -- src/routes/internal-atproto.test.ts src/utils/validation.test.ts
```

Expected: PASS.

- [ ] **Step 5: Apply the D1 migration remotely and deploy the worker**

Run:

```bash
cd ../divine-name-server
npx wrangler d1 migrations apply divine-name-server-db --remote
npx wrangler deploy
```

Expected: migration applies cleanly and the worker deploy succeeds.

- [ ] **Step 6: Commit**

```bash
cd ../divine-name-server
git add src/index.ts src/routes/internal-atproto.ts src/routes/internal-atproto.test.ts migrations/0008_reserve_atproto_service_subdomains.sql src/utils/subdomain.ts
git commit -m "feat: add atproto sync endpoint to divine-name-server"
```

### Task 5: Preserve ATProto Fields During Hourly Fastly Reconciliation

**Files:**
- Modify: `../divine-name-server/src/index.ts`
- Modify: `../divine-name-server/src/utils/fastly-sync.ts`
- Modify: `../divine-name-server/tests/atproto-sync.test.ts`
- Create: `../divine-name-server/tests/atproto-cron-sync.test.ts`

- [ ] **Step 1: Write the failing cron reconciliation test**

Add a test that loads an active username with:

```ts
atproto_did: 'did:plc:abc123',
atproto_state: 'ready'
```

and asserts the scheduled job sends those fields into `bulkSyncToFastly`.

- [ ] **Step 2: Run the focused tests and confirm the current omission**

Run:

```bash
cd ../divine-name-server
npm run test:once -- tests/atproto-sync.test.ts tests/atproto-cron-sync.test.ts
```

Expected: FAIL because the hourly reconciliation currently emits only `pubkey`, `relays`, and `status`.

- [ ] **Step 3: Make Fastly publication preserve ATProto fields and fail loudly when staging is misconfigured**

Change:
- the scheduled reconciliation in `src/index.ts` to include `atproto_did` and `atproto_state`
- `syncUsernameToFastly()` so missing `FASTLY_API_TOKEN` / `FASTLY_STORE_ID` is not silently treated as success during staging/prod deploys

Minimal scheduled payload shape:

```ts
data: {
  pubkey: u.pubkey!,
  relays: ...,
  status: 'active' as const,
  atproto_did: u.atproto_did,
  atproto_state: u.atproto_state,
}
```

- [ ] **Step 4: Re-run the focused tests**

Run:

```bash
cd ../divine-name-server
npm run test:once -- tests/atproto-sync.test.ts tests/atproto-cron-sync.test.ts
```

Expected: PASS.

- [ ] **Step 5: Deploy and verify the Fastly KV entry survives the next cron tick**

Run the worker deploy, then:
- perform one internal sync to publish a canary `ready` DID
- inspect the Fastly KV entry immediately
- inspect it again after the next hourly cron or after invoking the scheduled handler locally

Expected: the KV record still contains `atproto_did` and `atproto_state = ready`.

- [ ] **Step 6: Commit**

```bash
cd ../divine-name-server
git add src/index.ts src/utils/fastly-sync.ts tests/atproto-sync.test.ts tests/atproto-cron-sync.test.ts
git commit -m "fix: preserve atproto fields during fastly reconciliation"
```

## Chunk 4: Serve Public DID Resolution Through `divine-router`

### Task 6: Deploy The Read-Only `/.well-known/atproto-did` Edge Handler

**Files:**
- Modify: `../divine-router/src/main.rs`
- Modify: `../divine-router/README.md`

- [ ] **Step 1: Run the existing router tests that cover ATProto readiness gating**

Run:

```bash
cd ../divine-router
cargo test atproto_did_returns_ok_when_active_ready_and_did_present -- --nocapture
cargo test atproto_did_returns_not_found_when_state_is_pending -- --nocapture
cargo test atproto_did_returns_not_found_for_inactive_user -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Update the README to document the new KV payload**

Document that Fastly KV now stores:

```json
{
  "pubkey": "...",
  "relays": ["..."],
  "status": "active",
  "atproto_did": "did:plc:abc123",
  "atproto_state": "ready"
}
```

- [ ] **Step 3: Publish the Fastly service**

Run:

```bash
cd ../divine-router
fastly compute publish --non-interactive && fastly purge --all
```

Expected: successful publish and purge.

- [ ] **Step 4: Verify public DID resolution against a canary user**

Run:

```bash
curl -i https://<canary-username>.divine.video/.well-known/atproto-did
```

Expected:
- `200` with bare DID when `atproto_state = ready`
- `404` when the same user is moved to `pending`, `failed`, or `disabled`

- [ ] **Step 5: Commit**

```bash
cd ../divine-router
git add src/main.rs README.md
git commit -m "feat: serve atproto did resolution from fastly kv"
```

## Chunk 5: Bring The Real Keycast Control Plane Live In Staging

### Task 7: Wire Keycast ATProto Runtime Env And Secrets Through Coreconfig

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/deployment.yaml`
- Modify: `../divine-iac-coreconfig/k8s/external-secrets/base/keycast-secrets.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`

- [ ] **Step 1: Add the missing ATProto env vars and secrets to the keycast deployment contract**

Extend the base deployment with:

```yaml
- name: DIVINE_SKY_ATPROTO_CONTROL_PLANE_URL
  valueFrom:
    secretKeyRef:
      name: keycast-atproto-runtime
      key: DIVINE_SKY_ATPROTO_CONTROL_PLANE_URL
- name: KEYCAST_ATPROTO_TOKEN
  valueFrom:
    secretKeyRef:
      name: keycast-atproto-runtime
      key: KEYCAST_ATPROTO_TOKEN
```

and add a new `ExternalSecret` for:

```yaml
keycast-divine-sky-atproto-control-plane-url-ENVIRONMENT
keycast-atproto-token-ENVIRONMENT
```

- [ ] **Step 2: Patch the staging overlay to use the staging secret names**

Update:

```yaml
keycast-divine-sky-atproto-control-plane-url-staging
keycast-atproto-token-staging
```

in `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`.

- [ ] **Step 3: Render the staging keycast manifests**

Run:

```bash
cd ../divine-iac-coreconfig
kustomize build k8s/applications/keycast/overlays/staging >/tmp/keycast-staging.yaml
```

Expected: exit code `0`.

- [ ] **Step 4: Commit**

```bash
cd ../divine-iac-coreconfig
git add k8s/applications/keycast/base/deployment.yaml k8s/external-secrets/base/keycast-secrets.yaml k8s/applications/keycast/overlays/staging/kustomization.yaml
git commit -m "feat: wire keycast atproto staging runtime config"
```

### Task 8: Deploy A Keycast Image That Includes The ATProto Endpoints

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`
- Test: `../keycast/api/tests/atproto_opt_in_state_test.rs`
- Test: `../keycast/api/tests/atproto_http_test.rs`
- Test: `../keycast/api/tests/update_profile_divine_name_test.rs`

- [ ] **Step 1: Run the focused keycast ATProto test suite**

Run:

```bash
cd ../keycast
cargo test -p keycast_api --test atproto_opt_in_state_test -- --nocapture
cargo test -p keycast_api --test atproto_http_test -- --nocapture
cargo test -p keycast_api update_profile_claims_name_without_enabling_atproto -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Build and publish an immutable staging keycast image**

Produce a concrete image tag from the tested source revision and push it to the staging registry.

- [ ] **Step 3: Replace the staging image tag in coreconfig**

Update:

```yaml
images:
  - name: keycast
    newTag: <immutable-sha-tag>
```

in `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`.

- [ ] **Step 4: Sync staging and verify the keycast endpoints**

Verify:

```bash
POST /api/user/atproto/enable
GET  /api/user/atproto/status
POST /api/user/atproto/disable
POST /internal/atproto/state
```

Expected:
- `enable` returns `202` with `state = pending`
- internal sync can move the same user to `ready`
- `disable` moves state to `disabled`

- [ ] **Step 5: Commit**

```bash
cd ../divine-iac-coreconfig
git add k8s/applications/keycast/overlays/staging/kustomization.yaml
git commit -m "chore: pin staging keycast image with atproto control plane"
```

## Chunk 6: Execute The End-To-End Staging Smoke And Lock In Rollback

### Task 9: Update The Staging Smoke And Launch Docs, Then Run A Canary

**Files:**
- Modify: `docs/runbooks/atproto-opt-in-smoke-test.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Modify: `docs/runbooks/divine-sky-staging-status.md`

- [ ] **Step 1: Update the smoke test with explicit cross-repo checkpoints**

Add concrete checks for:
- `rsky-pds` canary account creation
- keycast `pending -> ready -> disabled`
- name-server D1 row contents
- Fastly KV record contents
- router `/.well-known/atproto-did`
- mirrored post existence after the user reaches `ready`

- [ ] **Step 2: Update the launch checklist with rollback gates**

Document:
- how to roll back the `rsky-pds` image tag
- how to revert the router deploy
- how to disable the keycast opt-in path without deleting existing AT repos
- how to confirm Fastly KV no longer advertises a DID after disable

- [ ] **Step 3: Run the canary flow end-to-end**

Run, in order:

```bash
curl -fsS https://pds.staging.dvines.org/xrpc/_health
curl -fsS https://login.staging.dvines.org/api/user/atproto/status
curl -i https://<canary>.divine.video/.well-known/atproto-did
```

Then:
- claim a new canary username
- enable ATProto
- wait for `ready`
- verify `/.well-known/atproto-did`
- publish one mirrored test post
- disable ATProto
- verify `/.well-known/atproto-did` returns `404`

- [ ] **Step 4: Record the final staging status**

Update `docs/runbooks/divine-sky-staging-status.md` with:
- exact image tags now deployed
- exact canary usernames/DIDs used
- which repos were deployed and when
- remaining known issues, if any

- [ ] **Step 5: Commit**

```bash
git add docs/runbooks/atproto-opt-in-smoke-test.md docs/runbooks/launch-checklist.md docs/runbooks/divine-sky-staging-status.md
git commit -m "docs: record staging atproto remediation rollout"
```

## Recommended Execution Order

1. Chunk 1 first. Do not spend time on router or keycast rollout until the PDS smoke passes.
2. Chunk 2 second. Once PDS succeeds, make sure staging `divine-sky` is actually running the image that contains the callback logic.
3. Chunks 3 and 4 next. The public read-model path (`divine-name-server` -> Fastly KV -> `divine-router`) is the next dependent boundary.
4. Chunk 5 after that. Bring the real user-facing keycast flow online only after the service-to-service path is proven.
5. Chunk 6 last. Run the canary and write down the exact deployed state before widening traffic.

Plan complete and saved to `docs/superpowers/plans/2026-03-21-staging-atproto-end-to-end-remediation.md`. Ready to execute?
