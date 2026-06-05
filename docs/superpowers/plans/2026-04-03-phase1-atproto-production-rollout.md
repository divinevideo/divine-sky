# Phase 1 ATProto Production Rollout Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Divine's Phase 1 ATProto opt-in path deployable and operable in staging and production, with the full hostname, protocol, GitOps, and public-edge chain working for real users.

**Architecture:** Keep `pds.divine.video` as the public ATProto resource server, keep `entryway.divine.video` as the ATProto Authorization Server origin, and keep the login UI as the human-facing control plane. Use `divine-iac-coreconfig` as the source of truth for all Kubernetes deploy wiring, then finish the non-Argo public edge by deploying `divine-name-server` and `divine-router` so handle resolution and DID discovery work on the public internet.

**Tech Stack:** Rust, Svelte, Flutter, Rocket, Axum, Kustomize, ArgoCD, GKE Gateway API, Cloudflare Workers, D1, Fastly Compute@Edge, Fastly KV, Google Secret Manager

---

## Workstream Map

1. **Finish the public ATProto login chain**
   Add `/.well-known/oauth-protected-resource` to `rsky-pds`, keep `entryway.divine.video` as the ATProto Authorization Server, and reconcile the smoke/runbook expectations so third-party ATProto clients can discover the live production chain.

2. **Unify the hostname contract**
   Pick one canonical login hostname, then align `divine-iac-coreconfig`, `divine-web`, `divine-mobile`, and the supporting docs so auth redirects, app links, and runtime URLs all point at the same production origin.

3. **Wire keycast ATProto runtime through GitOps**
   Add the missing `keycast` deployment env and secrets in `divine-iac-coreconfig`, point keycast at the real in-cluster handle gateway, and expose the required entryway hostname through Gateway routing.

4. **Close the non-Argo public edge**
   Treat `divine-name-server` and `divine-router` as first-class production deploy steps, because ArgoCD only covers the GKE services and does not publish the Cloudflare Worker or Fastly Compute edge.

5. **Harden production deploy hygiene**
   Replace floating `latest` tags with immutable images, codify that in the launch checklist, and only widen to all users once the rendered production manifests and full end-to-end smoke pass.

**Recommended order:** 1 -> 2 -> 3 -> 5 -> 4

**Why this order:** The protocol and hostname contracts have to settle first, then `keycast` GitOps can be wired correctly, then production image pinning can lock a release candidate, and only then is it worth publishing the public edge and running the final all-users smoke.

---

## Chunk 1: Freeze The Hostname And Protocol Contract

### Task 1: Resolve the production hostname split and align clients

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/deployment.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/httproute.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/docs/how-to/services/keycast.md`
- Modify: `../divine-iac-coreconfig/docs/for-platform/configure-dns.md`
- Modify: `../divine-web/src/lib/divineLogin.ts`
- Modify: `../divine-mobile/mobile/lib/providers/app_providers.dart`
- Modify: `../divine-mobile/mobile/android/app/src/main/AndroidManifest.xml`
- Modify: `../divine-mobile/mobile/ios/Runner/Runner.entitlements`
- Test: `../divine-web/src/lib/divineLogin.test.ts`
- Test: `../divine-mobile/mobile/packages/keycast_flutter/test/oauth_client_test.dart`

- [ ] **Step 1: Audit the current hostname contract before editing**

Run:

```bash
rg -n "login\\.divine\\.video|login\\.dvines\\.org|entryway\\.divine\\.video|pds\\.divine\\.video" \
  ../divine-iac-coreconfig ../divine-web ../divine-mobile ../keycast ../divine-sky
```

Expected: conflicting hits showing that IaC currently publishes `login.dvines.org` while clients and ATProto docs still assume `login.divine.video` plus `entryway.divine.video`.

- [ ] **Step 2: Decide and document the stable production contract**

Record one explicit rule set and use it everywhere:

```text
Human login UI: chosen once, then used consistently by web/mobile/IaC
ATProto Authorization Server: https://entryway.divine.video
ATProto Resource Server / PDS: https://pds.divine.video
```

Do not leave `login.dvines.org` in production if the rest of the product and protocol stack still points at `login.divine.video`.

- [ ] **Step 3: Update client defaults and deep links to match the chosen login origin**

Update the web and mobile defaults so auth redirects, RPC calls, and app links all point at the same live login origin.

- [ ] **Step 4: Update the keycast Gateway/IaC docs to match reality**

Make `docs/how-to/services/keycast.md` and `docs/for-platform/configure-dns.md` describe the same hostname contract the code uses. Remove the current "Cloud Run vs GKE" ambiguity once the production host is final.

- [ ] **Step 5: Run the focused client checks**

Run:

```bash
cd ../divine-web && pnpm test -- divineLogin
cd ../divine-mobile/mobile && flutter test packages/keycast_flutter/test/oauth_client_test.dart
```

Expected: the targeted login-origin tests pass against the chosen hostname.

- [ ] **Step 6: Commit**

```bash
git add \
  ../divine-iac-coreconfig/k8s/applications/keycast/base/deployment.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/base/httproute.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml \
  ../divine-iac-coreconfig/docs/how-to/services/keycast.md \
  ../divine-iac-coreconfig/docs/for-platform/configure-dns.md \
  ../divine-web/src/lib/divineLogin.ts \
  ../divine-mobile/mobile/lib/providers/app_providers.dart \
  ../divine-mobile/mobile/android/app/src/main/AndroidManifest.xml \
  ../divine-mobile/mobile/ios/Runner/Runner.entitlements
git commit -m "chore: align divine login hostnames"
```

### Task 2: Add the missing PDS protected-resource metadata

**Files:**
- Modify: `../rsky/rsky-pds/src/well_known.rs`
- Modify: `../rsky/rsky-pds/src/lib.rs`
- Create: `../rsky/rsky-pds/tests/oauth_protected_resource.rs`
- Modify: `docs/runbooks/atproto-auth-server-smoke-test.md`
- Modify: `docs/runbooks/atproto-opt-in-smoke-test.md`
- Test: `scripts/smoke-divine-atproto-login.sh`

- [ ] **Step 1: Add a failing test for `/.well-known/oauth-protected-resource`**

The new test should require:

```json
{
  "resource": "https://pds.divine.video",
  "authorization_servers": ["https://entryway.divine.video"]
}
```

This is currently missing from the PDS code path.

- [ ] **Step 2: Implement the protected-resource endpoint in `rsky-pds`**

Extend `well_known.rs` with a new route for `/.well-known/oauth-protected-resource`, then mount it in `src/lib.rs` beside the existing `/.well-known/atproto-did` and `/.well-known/did.json` routes.

- [ ] **Step 3: Run the focused PDS tests**

Run:

```bash
cd ../rsky && cargo test -p rsky-pds oauth_protected_resource
```

Expected: the new protected-resource test passes and existing `well_known` behavior stays green.

- [ ] **Step 4: Update the smoke docs to use the same auth-server origin**

`docs/runbooks/atproto-auth-server-smoke-test.md` currently still points at `login.divine.video`, while `docs/runbooks/atproto-opt-in-smoke-test.md` expects `entryway.divine.video`. Reconcile them so there is one live production story.

- [ ] **Step 5: Commit**

```bash
git add \
  ../rsky/rsky-pds/src/well_known.rs \
  ../rsky/rsky-pds/src/lib.rs \
  ../rsky/rsky-pds/tests/oauth_protected_resource.rs \
  docs/runbooks/atproto-auth-server-smoke-test.md \
  docs/runbooks/atproto-opt-in-smoke-test.md
git commit -m "feat: publish pds oauth protected-resource metadata"
```

## Chunk 2: Wire GitOps And Runtime Secrets

### Task 3: Teach `divine-iac-coreconfig` how to run the live keycast ATProto surfaces

**Files:**
- Create: `../divine-iac-coreconfig/k8s/applications/keycast/base/external-secret.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/deployment.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/httproute.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml`
- Modify: `../keycast/docs/DEPLOYMENT.md`

- [ ] **Step 1: Add secret-backed runtime env for the ATProto control plane**

Wire these env vars into the keycast deployment:

```text
DIVINE_SKY_ATPROTO_CONTROL_PLANE_URL
KEYCAST_ATPROTO_TOKEN
DIVINE_HANDLE_DOMAIN
ATPROTO_ENTRYWAY_ENABLED
ATPROTO_ENTRYWAY_ORIGIN
ATPROTO_ENTRYWAY_HOSTS
```

Use an `ExternalSecret` or another Secret Manager-backed mechanism instead of hard-coding sensitive values in overlays.

- [ ] **Step 2: Point keycast at the real in-cluster handle gateway**

Use the `divine-handle-gateway` service in the `sky` namespace as the control-plane target, for example:

```text
http://divine-handle-gateway.sky.svc.cluster.local:3000
```

Do not leave the default `http://127.0.0.1:3201` in staging or production.

- [ ] **Step 3: Publish the entryway hostname through the same Gateway route**

If keycast is serving the authorization-server metadata and OAuth endpoints, its `HTTPRoute` must advertise both the human login hostname and `entryway.divine.video` (plus staging equivalents if used).

- [ ] **Step 4: Validate the manifests before merging**

Run:

```bash
cd ../divine-iac-coreconfig
kustomize build k8s/applications/keycast/overlays/staging >/tmp/keycast-staging.yaml
kustomize build k8s/applications/keycast/overlays/production >/tmp/keycast-production.yaml
```

Expected: both builds succeed, and the rendered deployment includes the new ATProto env vars plus the required hostnames.

- [ ] **Step 5: Commit**

```bash
git add \
  ../divine-iac-coreconfig/k8s/applications/keycast/base/external-secret.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/base/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/base/deployment.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/base/httproute.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml \
  ../keycast/docs/DEPLOYMENT.md
git commit -m "feat: wire keycast atproto runtime through gitops"
```

### Task 4: Replace floating production images with immutable release tags

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml`
- Modify: `docs/runbooks/launch-checklist.md`

- [ ] **Step 1: Build and push immutable images for the exact commits being deployed**

Produce release tags for:

```text
keycast
rsky-pds
divine-atbridge
divine-handle-gateway
```

Do not ship production off `latest`.

- [ ] **Step 2: Patch the production overlays to those immutable tags**

Update the four production kustomizations and the keycast staging/production overlays so every environment points at a specific build artifact.

- [ ] **Step 3: Update the launch runbook to require pinned tags**

`docs/runbooks/launch-checklist.md` should treat "production overlay still uses `latest`" as a hard stop.

- [ ] **Step 4: Validate the rendered production manifests**

Run:

```bash
cd ../divine-iac-coreconfig
kustomize build k8s/applications/rsky-pds/overlays/production >/tmp/rsky-pds-prod.yaml
kustomize build k8s/applications/divine-atbridge/overlays/production >/tmp/divine-atbridge-prod.yaml
kustomize build k8s/applications/divine-handle-gateway/overlays/production >/tmp/divine-handle-gateway-prod.yaml
```

Expected: all rendered manifests use explicit image tags, not `latest`.

- [ ] **Step 5: Commit**

```bash
git add \
  ../divine-iac-coreconfig/k8s/applications/keycast/overlays/staging/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/production/kustomization.yaml \
  ../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml \
  docs/runbooks/launch-checklist.md
git commit -m "chore: pin atproto production images"
```

## Chunk 3: Deploy The Public Edge And Launch

### Task 5: Deploy the non-Argo public handle path and run the full launch smoke

**Files:**
- Modify: `../divine-name-server/README.md`
- Modify: `../divine-name-server/wrangler.toml`
- Modify: `../divine-router/README.md`
- Modify: `docs/runbooks/login-divine-video.md`
- Modify: `docs/runbooks/atproto-opt-in-smoke-test.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Test: `scripts/smoke-divine-atproto-login.sh`

- [ ] **Step 1: Verify production secrets for the public edge**

Before deploy, confirm:

```text
divine-name-server: FASTLY_API_TOKEN, FASTLY_STORE_ID, Cloudflare D1 binding
divine-router: Fastly service access and KV store access
```

- [ ] **Step 2: Deploy the name server**

Run:

```bash
cd ../divine-name-server
npx wrangler d1 migrations apply divine-name-server-db --remote
npx wrangler deploy
```

Expected: the worker is live on `divine.video/.well-known/*`, `names.divine.video/*`, and `names.admin.divine.video/*`.

- [ ] **Step 3: Publish the router and purge Fastly**

Run:

```bash
cd ../divine-router
fastly compute publish --non-interactive && fastly purge --all
```

Expected: `username.divine.video/.well-known/atproto-did` reflects the Fastly KV state written by `divine-name-server`.

- [ ] **Step 4: Merge the IaC changes and let Argo sync staging first, then production**

After the `divine-iac-coreconfig` PR merges to `main`, verify the Argo applications for `keycast`, `rsky-pds`, `divine-atbridge`, and `divine-handle-gateway` are healthy and rolled out on the target clusters.

- [ ] **Step 5: Run the end-to-end smoke in order**

Run:

```bash
bash scripts/smoke-divine-atproto-login.sh
curl -fsS https://pds.divine.video/xrpc/com.atproto.server.describeServer
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server
```

Then exercise the full user lifecycle from the Phase 1 control plane:

1. Sign in to the live login UI.
2. Claim a username.
3. Enable ATProto.
4. Wait for `pending -> ready`.
5. Confirm `username.divine.video/.well-known/atproto-did` returns the DID.
6. Disable ATProto and confirm the public handle stops resolving.

- [ ] **Step 6: Resolve rollout policy instead of leaving stale gating language**

Choose one path and update code plus docs to match:

```text
Option A: implement real rollout gates in keycast/mobile/web, then widen to 100%
Option B: remove the stale gate language and ship Phase 1 to all users directly
```

Do not keep runbooks that require `enableAtprotoPublishing` / `FeatureFlag.atprotoPublishing` if those flags do not actually exist.

- [ ] **Step 7: Commit the final operational doc cleanup**

```bash
git add \
  ../divine-name-server/README.md \
  ../divine-name-server/wrangler.toml \
  ../divine-router/README.md \
  docs/runbooks/login-divine-video.md \
  docs/runbooks/atproto-opt-in-smoke-test.md \
  docs/runbooks/launch-checklist.md
git commit -m "docs: finalize atproto production rollout runbooks"
```
