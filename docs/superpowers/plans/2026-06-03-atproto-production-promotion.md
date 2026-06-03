# ATProto Production Promotion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get ATProto integration (Nostr→Bluesky crossposting + the PDS/handle path) running in production by promoting the already-working staging deployment to the `dv-platform-prod` cluster.

**Architecture:** Production has the bridge manifests but is `OutOfSync`/`Degraded` with **zero pods** because (a) its GCP Secret Manager secrets were never created, (b) several IaC overlay placeholders/patches are incomplete, and (c) images are unpinned. Staging is the authoritative template for every config VALUE.

> ⚠️ **CORRECTION (2026-06-03): "staging works" is NOT established — do not assume promotion alone is sufficient.** Staging pods are `1/1 Running` and ExternalSecrets `SecretSynced=True` (control plane healthy), BUT the staging bridge has **never crossposted**: its DB is empty (`account_links`=0 incl. pending, `publish_jobs`=0, published `record_mappings`=0) and the **staging relay `wss://relay.staging.dvines.org` is currently DOWN** (continuous `failed to connect to relay`, verified 06:18 2026-06-03). So the end-to-end Nostr→Bluesky path has never run in any environment. This plan is therefore "**stand up prod config AND prove the path end-to-end for the first time**," not "copy a known-good deployment." Chunk D's verification is a genuine first-light test, and **Chunk F (below) must establish a working crosspost in staging first** — otherwise prod will hit the same relay/opt-in gaps with no template to copy from. Use staging only as the template for config KEY NAMES and structure, not as proof of function.

**Tech Stack:** GKE, ArgoCD, Kustomize, External Secrets Operator (ESO) → GCP Secret Manager, Rust (divine-atbridge/divine-bridge-db), `kubectl`, `gcloud`.

**Two repos:**
- `divine-iac-coreconfig` (sibling repo) — all the deploy/secret/manifest fixes. **Most of this plan.**
- `divine-sky` (this repo) — already done: the bridge self-migrates on startup (commit `048c293`). Only verification tasks here.

**Cluster contexts (use `--context`):**
- prod: `connectgateway_dv-platform-prod_us-central1_gke-production-membership`
- staging: `connectgateway_dv-platform-staging_us-central1_gke-staging-membership`

**Projects:** prod secrets live in `dv-platform-prod`; staging in `dv-platform-staging`. POC (`rich-compiler-479518-d2` / `dv-platform-poc`) has **no bridge** — never use as reference. Always pass explicit `gcloud --project=`; the local default drifts to the POC project.

---

## Evidence Baseline (verified 2026-06-03, do not re-litigate)

| Service | Staging | Production |
|---|---|---|
| pods | `1/1 Running` | **0** |
| ExternalSecret | `SecretSynced=True` | `SecretSyncedError` |
| image | pinned SHA (e.g. atbridge `3ba324c`) | **`latest`** |

Confirmed gaps (each is a task below):
1. **0 of the bridge `*-production` secrets exist** in `dv-platform-prod` (ESO: `Secret does not exist`, NotFound not IAM; 54 secrets visible incl. keycast's 8, so access is real). 25 keys total.
2. **rsky-pds prod overlay patches only 3 of 8 ExternalSecret keys** — keys 3–7 (`repo-signing-key`, `plc-rotation-key`, `jwt-key`, both `aws-*`) keep the base literal `…-ENVIRONMENT`, which will never resolve. Real IaC bug.
3. **divine-labeler prod overlay has `did:plc:PRODUCTION_DID_PLACEHOLDER`** — an unsubstituted placeholder DID.
4. **All 5 prod images are `newTag: latest`.** The atbridge `latest` may predate the startup self-migration (B2, divine-sky `048c293`); if so the prod schema won't auto-apply.
5. **`VIDEO_SERVICE_ENABLED` is unset** on atbridge → videos publish but don't play.
6. **rsky-pds entryway env unset** (`PDS_OAUTH_AUTHORIZATION_SERVER`, `PDS_ENTRYWAY_DID`) → blocks third-party ATProto *login* (not crossposting). Lower priority.

7. **No crosspost has ever succeeded** in staging (relay down + zero opted-in accounts). The e2e path is unproven anywhere.

**Recommended order:** **F (prove staging e2e) → 1 → 2 → 3 → 4 → 5 → D (deploy/verify prod) → E (login).**
Chunk F comes first: fixing the staging relay + doing one real staging crosspost gives a known-good reference and de-risks prod. Crossposting in prod needs F,1,2,4,5 + the three pods healthy. Login (the full "ATProto integration") additionally needs E.

> **Authoritative doc:** This plan supersedes the prod-secrets / image-pinning / entryway chunks (B, Chunk D-token-trust) of `2026-05-30-atproto-production-rollout.md` with live-verified specifics. Treat THIS file as the execution doc for the prod move; keep the 05-30 plan for the broader divine-sky scheduler/code context only.

---

## Chunk F: Prove the End-to-End Crosspost Path in Staging FIRST

Rationale: the e2e path has never run. Establish it in staging — cheaper to debug, and it becomes the known-good reference prod copies. Do this before touching prod.

### Task F1: Fix the staging relay

**Symptom:** `divine-atbridge` logs continuous `failed to connect to relay wss://relay.staging.dvines.org`. No relay → no Nostr events → nothing to crosspost.

- [ ] **Step 1: Determine whether the relay is down or the URL is wrong**

```bash
SC=connectgateway_dv-platform-staging_us-central1_gke-staging-membership
# Is there a relay workload in the cluster?
kubectl --context $SC get pods -A | grep -iE 'relay|strfry|nostr'
# Does the hostname resolve / serve?
curl -sS -I https://relay.staging.dvines.org 2>&1 | head -3
```
Expected to reveal one of: relay pod crashed/absent, DNS/ingress broken, or the bridge's `RELAY_URL` secret points at a dead host.

- [ ] **Step 2: Fix the identified cause** — restart/redeploy the relay if it's a workload, fix DNS/ingress, or correct `divine-atbridge-relay-url-staging` in `dv-platform-staging` Secret Manager if the URL is wrong.

- [ ] **Step 3: Confirm the bridge connects**

```bash
kubectl --context $SC logs deploy/divine-atbridge -n sky --tail=30 | grep -iE 'connecting bridge runtime|connected|failed to connect'
```
Expected: a stable `connecting bridge runtime` with no immediate `failed to connect` follow-up.

### Task F2: Provision a staging test account and verify one crosspost

- [ ] **Step 1: Opt in a test account** via the staging handle-gateway / keycast flow (claim `<test>.divine.video`, enable ATProto). Confirm the row lands:

```bash
DB=<staging bridge DATABASE_URL from dv-platform-staging SM>
kubectl --context $SC run pg-$$ -n sky --rm -i --restart=Never --image=postgres:16 --quiet --command -- \
  psql "$DB" -tAc "SELECT nostr_pubkey, provisioning_state, crosspost_enabled FROM account_links;"
```
Expected: one row, `provisioning_state` progressing `pending`→`ready`, `crosspost_enabled=t`.

- [ ] **Step 2: Post a Nostr video** as that account, then confirm it crossposts:

```bash
kubectl --context $SC run pg-$$ -n sky --rm -i --restart=Never --image=postgres:16 --quiet --command -- \
  psql "$DB" -tAc "SELECT state, job_source, count(*) FROM publish_jobs GROUP BY 1,2; \
                   SELECT status, count(*) FROM record_mappings GROUP BY 1;"
```
Expected: a `publish_jobs` row reaching `state='published'`, a `record_mappings` row `status='published'`. Then confirm it actually appears AND plays on Bluesky (validates video routing). **This is the first proof the integration works end to end.**

- [ ] **Step 3: Record the staging config that made it work** as the prod template (relay URL shape, video-service setting, any env the bridge needed). Carry these into the prod tasks below.

---

## Chunk A: Create the Production Secrets

### Task A1: Enumerate the exact key set from staging (source of truth)

**Why:** staging works, so its secret key set is correct. We mirror it to prod with `-production` suffix.

- [ ] **Step 1: List the staging secret keys that actually exist and sync**

```bash
gcloud secrets list --project=dv-platform-staging --format="value(name)" \
  | grep -E 'atbridge|handle-gateway|rsky-pds' | sort
```
Expected: 25 names, all `-staging` suffixed (10 atbridge, 7 handle-gateway, 8 rsky-pds).

- [ ] **Step 2: Cross-check against what the prod ExternalSecrets request**

```bash
for es in divine-atbridge-runtime divine-handle-gateway-runtime rsky-pds-runtime; do
  echo "--- $es ---"
  kubectl --context connectgateway_dv-platform-prod_us-central1_gke-production-membership \
    get externalsecret $es -n sky -o jsonpath='{range .spec.data[*]}{.remoteRef.key}{"\n"}{end}'
done
```
Expected: the `-production` versions of the same 25 keys — **except** rsky-pds keys 3–7 will show `…-ENVIRONMENT` (that's the bug fixed in Task B1; create the `-production` secrets here regardless).

The 25 prod keys to create:
```
divine-atbridge-{relay-url,pds-url,pds-auth-token,blossom-url,database-url,s3-endpoint,s3-bucket,plc-directory-url,handle-domain,atproto-provisioning-token}-production
divine-handle-gateway-{database-url,keycast-atproto-token,atproto-provisioning-url,atproto-keycast-sync-url,atproto-name-server-sync-url,atproto-name-server-sync-token,atproto-provisioning-token}-production
rsky-pds-{database-url,jwt-secret,admin-password,repo-signing-key,plc-rotation-key,jwt-key,aws-access-key-id,aws-secret-access-key}-production
```

### Task A2: Create each prod secret with its PRODUCTION value

**CRITICAL:** Do **not** copy staging values. URLs, DB credentials, S3 buckets, signing keys, and the PDS hostname all differ between staging (`*.staging.dvines.org`, `divine-pds-blobs-staging`) and prod (`pds.divine.video`, prod buckets). Signing/rotation keys MUST be freshly generated for prod, never shared with staging. Source prod values from the prod infra owner / existing prod resources.

- [ ] **Step 1: Create each secret (repeat for all 25)**

```bash
# Example — substitute the real production value for each:
printf '%s' "<PROD_VALUE>" | gcloud secrets create divine-atbridge-relay-url-production \
  --project=dv-platform-prod --replication-policy=automatic --data-file=-
```
For a secret that already exists but needs a value, add a version instead:
```bash
printf '%s' "<PROD_VALUE>" | gcloud secrets versions add <name> --project=dv-platform-prod --data-file=-
```

Production value sources (confirm each with the infra owner):
- `*-database-url-production` → the prod Postgres connection strings (atbridge, handle-gateway, rsky-pds may share or differ — confirm).
- `divine-atbridge-pds-url-production` → `https://pds.divine.video`
- `divine-atbridge-handle-domain-production` → `divine.video`
- `divine-atbridge-blossom-url`, `s3-endpoint`, `s3-bucket` → prod Blossom + prod blob bucket (NOT `*-staging`).
- `rsky-pds-repo-signing-key`, `-plc-rotation-key`, `-jwt-key`, `-jwt-secret` → **freshly generated for prod — BUT first confirm the prod PDS is greenfield.** Zero pods ≠ empty database. If the prod PDS Postgres already has any repos/identities (created under an earlier key), minting a NEW `plc-rotation-key`/`repo-signing-key` orphans them irrecoverably. Check before generating:
  ```bash
  # against the prod rsky-pds DB (URL from dv-platform-prod SM, once created):
  #   SELECT count(*) FROM actor;   -- or the rsky-pds repo/identity table
  ```
  If non-zero, do NOT mint new keys — recover the existing ones with the infra owner.
- `rsky-pds-aws-access-key-id`, `-aws-secret-access-key` → prod blob-store credentials.
- `*-atproto-provisioning-token`, `keycast-atproto-token`, `*-sync-token` → prod shared secrets (must match the values keycast/name-server use in prod).

- [ ] **Step 2: Verify all 25 now exist**

```bash
gcloud secrets list --project=dv-platform-prod --format="value(name)" \
  | grep -E 'atbridge|handle-gateway|rsky-pds' | sort | wc -l
```
Expected: `25`.

---

## Chunk B: Fix the Production Overlay Bugs (divine-iac-coreconfig)

### Task B1: Patch the 5 unpatched rsky-pds ExternalSecret keys

**File:** `divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml`

The prod overlay's ExternalSecret patch only replaces `data/0`, `data/1`, `data/2`. Keys 3–7 keep the base `…-ENVIRONMENT` literal and will fail to resolve forever.

- [ ] **Step 1: Confirm the bug**

```bash
cd ../divine-iac-coreconfig
kubectl kustomize k8s/applications/rsky-pds/overlays/production | \
  grep -A1 remoteRef | grep key:
```
Expected (the bug): keys 0–2 say `-production`, keys 3–7 say `-ENVIRONMENT`.

- [ ] **Step 2: Add the 5 missing replace ops to the ExternalSecret patch**

In the `target: kind: ExternalSecret name: rsky-pds-runtime` patch, after the existing `data/2` op, append:
```yaml
      - op: replace
        path: /spec/data/3/remoteRef/key
        value: rsky-pds-repo-signing-key-production
      - op: replace
        path: /spec/data/4/remoteRef/key
        value: rsky-pds-plc-rotation-key-production
      - op: replace
        path: /spec/data/5/remoteRef/key
        value: rsky-pds-jwt-key-production
      - op: replace
        path: /spec/data/6/remoteRef/key
        value: rsky-pds-aws-access-key-id-production
      - op: replace
        path: /spec/data/7/remoteRef/key
        value: rsky-pds-aws-secret-access-key-production
```

- [ ] **Step 3: Re-render and confirm all 8 are `-production`**

```bash
kubectl kustomize k8s/applications/rsky-pds/overlays/production | grep key: | grep -c ENVIRONMENT
```
Expected: `0`.

- [ ] **Step 4: Commit (in divine-iac-coreconfig)**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/rsky-pds/overlays/production/kustomization.yaml
git -C ../divine-iac-coreconfig commit -m "fix(rsky-pds): patch all 8 prod externalsecret keys (5 were stuck on ENVIRONMENT)"
```

### Task B2: Replace the labeler PRODUCTION_DID_PLACEHOLDER

**File:** `divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/production/kustomization.yaml`

Contains `value: "did:plc:PRODUCTION_DID_PLACEHOLDER"`.

- [ ] **Step 1: Obtain the real production labeler DID** (the `did:plc:` of the prod labeler account; staging uses `did:plc:diipfbfrgwpbpoeehyovemmy` — prod must be its own). Confirm with the infra owner / the prod labeler account record.

- [ ] **Step 2: Replace the placeholder** with the real prod DID.

- [ ] **Step 3: Verify no placeholder remains**

```bash
grep -rn 'PLACEHOLDER' k8s/applications/divine-labeler/
```
Expected: no output.

- [ ] **Step 4: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/divine-labeler/overlays/production/kustomization.yaml
git -C ../divine-iac-coreconfig commit -m "fix(divine-labeler): set real production labeler DID"
```

### Task B3: Enable video routing on atbridge

**File:** `divine-iac-coreconfig/k8s/applications/divine-atbridge/base/deployment.yaml` (env block)

Without `VIDEO_SERVICE_ENABLED=true`, the bridge uploads video blobs directly to the PDS, producing unplayable "Video not found" embeds on Bluesky.

- [ ] **Step 1: Add the env var** alongside the existing `RELAY_SOURCE_NAME` / `RUST_LOG` entries:
```yaml
            - name: VIDEO_SERVICE_ENABLED
              value: "true"
```
(`VIDEO_SERVICE_URL` defaults to `https://video.bsky.app`; only add it if prod uses a different transcoder.)

- [ ] **Step 2: Render & confirm**

```bash
kubectl kustomize k8s/applications/divine-atbridge/overlays/production | grep -A1 VIDEO_SERVICE_ENABLED
```
Expected: shows `value: "true"`.

- [ ] **Step 3: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/divine-atbridge/base/deployment.yaml
git -C ../divine-iac-coreconfig commit -m "feat(divine-atbridge): enable video service routing in prod"
```

---

## Chunk C: Pin Production Images

### Task C1: Pin all 5 services to immutable tags (rsky-pds and atbridge are load-bearing)

**Files:** `overlays/production/kustomization.yaml` for `rsky-pds`, `divine-atbridge`, `divine-handle-gateway`, `divine-feedgen`, `divine-labeler`.

All are `newTag: latest`. atbridge in particular must run a build containing the startup self-migration (divine-sky `048c293`), or the prod schema won't auto-apply.

- [ ] **Step 1: List available tags in the PROD registry** (prod registry ≠ staging registry; a staging SHA is not guaranteed present)

```bash
for s in rsky-pds divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  echo "=== $s ==="
  gcloud artifacts docker tags list \
    us-central1-docker.pkg.dev/dv-platform-prod/containers-production/$s \
    --project=dv-platform-prod --format="value(tag)" --limit=10 2>&1 | head
done
```

- [ ] **Step 2: For divine-atbridge, pick a tag whose build includes the self-migration.** If no such prod image exists yet, a prod image must be built/pushed from divine-sky `main` at or after commit `048c293` (CI/registry action, outside this file). Do NOT pin a tag that predates it and assume migrations run.

- [ ] **Step 3: Replace `newTag: latest` with the chosen immutable tag** in each of the 5 production overlays.

- [ ] **Step 4: Confirm no `latest` remains in prod**

```bash
for s in rsky-pds divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  kubectl kustomize k8s/applications/$s/overlays/production | grep -E 'image:.*:latest' \
    && echo "FAIL $s" || echo "ok $s"
done
```
Expected: `ok` for all 5.

- [ ] **Step 5: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/*/overlays/production/kustomization.yaml
git -C ../divine-iac-coreconfig commit -m "chore: pin atproto production images to immutable tags"
```

---

## Chunk D: Deploy and Verify Crossposting

### Task D1: Sync ESO + ArgoCD and confirm pods start

- [ ] **Step 1: Merge the divine-iac-coreconfig changes to `main`** (ArgoCD tracks `main`). After merge, force ESO to re-read Secret Manager:

```bash
PROD=connectgateway_dv-platform-prod_us-central1_gke-production-membership
for es in divine-atbridge-runtime divine-handle-gateway-runtime rsky-pds-runtime; do
  kubectl --context $PROD annotate externalsecret $es -n sky force-sync="$(date +%s)" --overwrite
done
```

- [ ] **Step 2: Confirm ExternalSecrets sync**

```bash
kubectl --context $PROD get externalsecret -n sky
```
Expected: all three `STATUS=SecretSynced READY=True`. If still `SecretSyncedError`, a key is missing/misnamed — re-check Task A2 and B1 against the exact ESO error in `kubectl get events -n sky`.

- [ ] **Step 3: Sync ArgoCD apps and confirm pods**

```bash
# via argocd CLI if available, else ArgoCD UI hard-refresh + sync for each app
kubectl --context $PROD get applications -n argocd | grep -E 'atbridge|handle-gateway|rsky-pds'
kubectl --context $PROD get pods -n sky
```
Expected: apps `Synced`/`Healthy`; `divine-atbridge`, `divine-handle-gateway`, `rsky-pds` pods `1/1 Running`.

### Task D2: Verify the startup migration ran (validates divine-sky B2 in prod)

- [ ] **Step 1: Check the atbridge log for the migration line**

```bash
kubectl --context $PROD logs deploy/divine-atbridge -n sky | grep -iE 'migration|column.*does not exist|panic'
```
Expected: `bridge database migrations applied`, and NO `column … does not exist`. If the image predates `048c293` (Task C2 not satisfied), you'll see a schema error instead → apply migrations manually or fix the pin.

- [ ] **Step 2: Spot-check the prod schema directly** (connect with the prod DB URL from Secret Manager)

```sql
\d publish_jobs      -- must show job_source, lease_owner, lease_expires_at, event_created_at, nostr_pubkey
\d account_links     -- must show publish_backfill_state; crosspost_enabled default should be TRUE
```

### Task D3: End-to-end crosspost verification

- [ ] **Step 1: Confirm the bridge connects to the prod relay**

```bash
kubectl --context $PROD logs deploy/divine-atbridge -n sky --tail=20 | grep -iE 'connecting bridge runtime|relay'
```
Expected: `connecting bridge runtime … source=divine-sky-bridge` against the **prod** relay URL (not staging).

- [ ] **Step 2: Post a real test video** from a `ready`, `crosspost_enabled` account on prod, then confirm it appears AND plays on Bluesky (playback validates Task B3 / video routing). Confirm the handle resolves: `curl -fsS https://<username>.divine.video/.well-known/atproto-did`.

- [ ] **Step 3: Confirm no stuck jobs**

```sql
SELECT nostr_event_id, job_source, state, lease_expires_at FROM publish_jobs
  WHERE state = 'in_progress' ORDER BY lease_expires_at ASC NULLS FIRST;
SELECT nostr_pubkey, publish_backfill_error FROM account_links
  WHERE publish_backfill_state = 'failed';
```
Expected: empty (or only actively-leased rows).

---

## Chunk E: Enable Third-Party ATProto Login (completes "ATProto integration")

Crossposting (A–D) is the publish path. The *login* path — a Bluesky client signing in with `username.divine.video` — additionally needs the PDS to advertise its auth server.

### Task E1: Wire the rsky-pds entryway env in prod

**File:** `divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml` (Deployment patch)

Per `2026-05-30-atproto-production-rollout.md` Chunk B: `PDS_OAUTH_AUTHORIZATION_SERVER` and `PDS_ENTRYWAY_DID` are unset, so `/.well-known/oauth-protected-resource` 404s and entryway token-trust is inert.

- [ ] **Step 1: Add the env to the rsky-pds Deployment patch:**
```yaml
      - op: add
        path: /spec/template/spec/containers/0/env/-
        value:
          name: PDS_OAUTH_AUTHORIZATION_SERVER
          value: "https://entryway.divine.video"
      - op: add
        path: /spec/template/spec/containers/0/env/-
        value:
          name: PDS_ENTRYWAY_DID
          value: "<prod entryway service DID — confirm with keycast/entryway owner>"
```

- [ ] **Step 2: After deploy, verify discovery + token trust**

```bash
curl -fsS https://pds.divine.video/.well-known/oauth-protected-resource | jq
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server | jq '{authorization_endpoint, scopes_supported}'
bash scripts/smoke-divine-atproto-login.sh   # in divine-sky; asserts /api/atproto/oauth/* (already corrected)
```
Expected: protected-resource returns `{resource: https://pds.divine.video, authorization_servers: [https://entryway.divine.video]}`; entryway advertises `/api/atproto/oauth/*` + `scopes_supported:["atproto"]`. Run the rsky-pds entryway token-trust test (a forged-key token must be REJECTED) before widening login — see `2026-05-30` plan Chunk D; a wrong-key acceptance is a hard rollout stop.

- [ ] **Step 3: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/rsky-pds/overlays/production/kustomization.yaml
git -C ../divine-iac-coreconfig commit -m "feat(rsky-pds): advertise entryway auth server in prod"
```

---

## Self-Review

**Spec coverage:** every gap from the Evidence Baseline maps to a task — secrets→A; rsky-pds 3/8 patch→B1; labeler placeholder→B2; video routing→B3; image pinning→C; deploy/migration/e2e→D; login/entryway→E. ✓

**No placeholders in the plan itself:** the only `<…>` are genuine human-supplied production secret values and the prod entryway DID, which cannot be invented and are explicitly flagged to source from the infra owner — not plan laziness.

**Cross-repo boundary:** divine-iac-coreconfig owns A,B,C,E edits; divine-sky owns only the (already-shipped) self-migration and the verification commands in D. The plan never edits divine-sky code.

## Risks
- **Wrong/shared secret values.** Copying staging values (URLs, buckets, signing keys) into prod is the most likely mistake. Signing/rotation keys MUST be prod-unique. Task A2 calls this out per-key.
- **Image predates self-migration.** If atbridge prod can't be pinned to a `048c293`+ build, the schema won't auto-apply — Task D2 catches it; fallback is manual `psql` of the idempotent migrations.
- **Abuse surface.** Opening prod crossposting re-exposes the spam vector from the 2026-03 PDS incident. Confirm rate limits + moderation intake (see `2026-05-30` plan Chunk H) before widening past an internal cohort.
- **Verify kubectl/gcloud context constantly.** gcloud default drifts to the POC project; pass `--project=dv-platform-prod` and `--context` explicitly on every command.
