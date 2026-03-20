# divine-sky Staging and Production Deploy Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the runnable `divine-sky` services for Kubernetes and add staging/production ArgoCD deployments for them in `../divine-iac-coreconfig`.

**Architecture:** Runtime behavior and env contracts live in `divine-sky`; staging/production manifests, secrets, namespace, routes, and promotion live in `divine-iac-coreconfig`. The four services are deployed as separate apps in a shared `sky` namespace, with only `divine-feedgen` and `divine-labeler` exposed publicly.

**Tech Stack:** Rust (Axum, Tokio, Diesel), Kubernetes (Deployment, Service, HTTPRoute), Kustomize overlays, ArgoCD ApplicationSets, External Secrets, GKE/Workload Identity patterns from `divine-iac-coreconfig`.

---

## Existing Infrastructure (Reference)

| Component | Location | Notes |
|---|---|---|
| Handle gateway app | `crates/divine-handle-gateway/src/lib.rs` | Internal HTTP service, currently no health endpoints |
| Handle gateway main | `crates/divine-handle-gateway/src/main.rs` | Currently binds `127.0.0.1:3000` |
| Feedgen app | `crates/divine-feedgen/src/lib.rs` | Public XRPC surface, no health endpoints |
| Feedgen main | `crates/divine-feedgen/src/main.rs` | Currently binds `127.0.0.1:3002` |
| Labeler app | `crates/divine-labeler/src/lib.rs` | Has `/health`, no `/health/ready` |
| Labeler config | `crates/divine-labeler/src/config.rs` | Already env-driven |
| AT bridge worker | `crates/divine-atbridge/src/main.rs`, `crates/divine-atbridge/src/runtime.rs`, `crates/divine-atbridge/src/config.rs` | Worker runtime, no health surface yet |
| Coreconfig app pattern | `../divine-iac-coreconfig/k8s/applications/keycast/` | Base + env overlays |
| Coreconfig worker pattern | `../divine-iac-coreconfig/k8s/applications/osprey-nostr-bridge/` | Internal worker deployment |
| Coreconfig app registration | `../divine-iac-coreconfig/k8s/argocd/apps/keycast.yaml` | ApplicationSet pattern |
| Coreconfig namespace config | `../divine-iac-coreconfig/k8s/cluster-config/namespaces/` | Namespace manifests |

## Chunk 1: Harden HTTP Runtimes in divine-sky

### Task 1: Make divine-handle-gateway Kubernetes-friendly

**Files:**
- Modify: `crates/divine-handle-gateway/src/lib.rs`
- Modify: `crates/divine-handle-gateway/src/main.rs`
- Modify: `crates/divine-handle-gateway/tests/control_plane.rs`
- Create or modify: `crates/divine-handle-gateway/tests/health.rs`

- [ ] **Step 1: Write failing tests for `/health` and `/health/ready`**

Add tests asserting both endpoints return `200 OK` without auth and that the app can still enforce auth on protected routes.

- [ ] **Step 2: Run the new handle-gateway health tests and verify they fail**

Run: `cargo test -p divine-handle-gateway health -- --nocapture`

- [ ] **Step 3: Add unauthenticated health endpoints**

Expose:

```rust
GET /health
GET /health/ready
```

Both should return simple `200 OK` responses.

- [ ] **Step 4: Replace hardcoded localhost binding with env-driven bind config**

In `main.rs`, load bind host/port or a combined bind address from env instead of hardcoding `127.0.0.1:3000`.

- [ ] **Step 5: Run focused verification**

Run:

```bash
cargo test -p divine-handle-gateway --test control_plane -- --nocapture
cargo check -p divine-handle-gateway --all-targets
```

- [ ] **Step 6: Commit**

```bash
git add crates/divine-handle-gateway
git commit -m "feat: harden handle gateway runtime for kubernetes"
```

### Task 2: Make divine-feedgen Kubernetes-friendly

**Files:**
- Modify: `crates/divine-feedgen/src/lib.rs`
- Modify: `crates/divine-feedgen/src/main.rs`
- Create: `crates/divine-feedgen/tests/health.rs`

- [ ] **Step 1: Write failing tests for `/health` and `/health/ready`**

Add tests for:

- `GET /health` returns `200`
- `GET /health/ready` returns `200`
- existing XRPC endpoints still behave the same

- [ ] **Step 2: Run the new feedgen tests and verify they fail**

Run: `cargo test -p divine-feedgen -- --nocapture`

- [ ] **Step 3: Add health routes to `app()`**

Expose lightweight health handlers alongside existing XRPC routes.

- [ ] **Step 4: Replace hardcoded bind with env-driven host/port**

Make the feedgen binary listen from env rather than `127.0.0.1:3002`.

- [ ] **Step 5: Run focused verification**

Run:

```bash
cargo test -p divine-feedgen -- --nocapture
cargo check -p divine-feedgen --all-targets
```

- [ ] **Step 6: Commit**

```bash
git add crates/divine-feedgen
git commit -m "feat: harden feedgen runtime for kubernetes"
```

### Task 3: Add readiness to divine-labeler

**Files:**
- Modify: `crates/divine-labeler/src/lib.rs`
- Modify: `crates/divine-labeler/tests/query_labels.rs`
- Create or modify: `crates/divine-labeler/tests/health.rs`

- [ ] **Step 1: Write a failing test for `/health/ready`**

Add a route test proving `GET /health/ready` returns `200 OK`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p divine-labeler health -- --nocapture`

- [ ] **Step 3: Add `/health/ready` to the labeler app**

Keep `/health` unchanged; add a separate readiness endpoint.

- [ ] **Step 4: Run focused verification**

Run:

```bash
cargo test -p divine-labeler -- --nocapture
cargo check -p divine-labeler --all-targets
```

- [ ] **Step 5: Commit**

```bash
git add crates/divine-labeler
git commit -m "feat: add labeler readiness endpoint"
```

## Chunk 2: Add Worker Health and Runtime Contract Docs

### Task 4: Add a lightweight health surface to divine-atbridge

**Files:**
- Modify: `crates/divine-atbridge/src/config.rs`
- Modify: `crates/divine-atbridge/src/main.rs`
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Create: `crates/divine-atbridge/src/health.rs`
- Create or modify: `crates/divine-atbridge/tests/runtime_health.rs`

- [ ] **Step 1: Write a failing test for the AT bridge health surface**

Cover the chosen health contract:

- if HTTP: `GET /health` and `GET /health/ready`
- if simpler internal heartbeat state is exposed through a sidecar-free HTTP server, test that server directly

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p divine-atbridge runtime_health -- --nocapture`

- [ ] **Step 3: Add health configuration to `BridgeConfig`**

Introduce env needed for the health surface, for example:

```rust
pub health_bind_addr: String
```

- [ ] **Step 4: Implement the minimal health surface**

The worker must remain singleton for now; do not add multi-replica lease logic in this chunk.

- [ ] **Step 5: Run focused verification**

Run:

```bash
cargo test -p divine-atbridge --test provisioning_lifecycle -- --nocapture
cargo test -p divine-atbridge runtime_health -- --nocapture
cargo check -p divine-atbridge --all-targets
```

- [ ] **Step 6: Commit**

```bash
git add crates/divine-atbridge
git commit -m "feat: add atbridge health surface for kubernetes"
```

### Task 5: Document the runtime env contract in divine-sky

**Files:**
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Create: `docs/runbooks/staging-production-deploy.md`

- [ ] **Step 1: Document per-service required env vars**

List required env for:

- `divine-atbridge`
- `divine-handle-gateway`
- `divine-feedgen`
- `divine-labeler`

- [ ] **Step 2: Document intended staging/production hostnames**

Use:

- `feed.staging.dvines.org`
- `feed.divine.video`
- `labeler.staging.dvines.org`
- `labeler.divine.video`

- [ ] **Step 3: Run doc sanity checks**

Run:

```bash
rg -n "feed\\.divine\\.video|labeler\\.divine\\.video|divine-atbridge|divine-handle-gateway" docs/runbooks
```

- [ ] **Step 4: Commit**

```bash
git add docs/runbooks
git commit -m "docs: record divine-sky deploy contract"
```

## Chunk 3: Add sky Namespace and Internal Service Deployments in divine-iac-coreconfig

### Task 6: Create the `sky` namespace and register it with cluster-config

**Files:**
- Create: `../divine-iac-coreconfig/k8s/cluster-config/namespaces/sky.yaml`
- Modify: `../divine-iac-coreconfig/k8s/cluster-config/namespaces/kustomization.yaml`

- [ ] **Step 1: Add the namespace manifest**

Follow the existing namespace pattern, with Linkerd injection enabled unless there is a specific reason not to.

- [ ] **Step 2: Register the namespace in cluster-config kustomization**

- [ ] **Step 3: Render cluster-config to verify it builds**

Run:

```bash
kubectl kustomize ../divine-iac-coreconfig/k8s/cluster-config >/tmp/sky-cluster-config.yaml
```

- [ ] **Step 4: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/cluster-config/namespaces
git -C ../divine-iac-coreconfig commit -m "feat: add sky namespace"
```

### Task 7: Add internal service apps for divine-atbridge and divine-handle-gateway

**Files:**
- Create: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/base/{deployment.yaml,kustomization.yaml,service.yaml,serviceaccount.yaml}`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/staging/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/production/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/base/{deployment.yaml,kustomization.yaml,service.yaml,serviceaccount.yaml}`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/staging/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/argocd/apps/divine-atbridge.yaml`
- Create: `../divine-iac-coreconfig/k8s/argocd/apps/divine-handle-gateway.yaml`

- [ ] **Step 1: Build base manifests for divine-atbridge**

Use the internal worker pattern:

- `Deployment`
- `ServiceAccount`
- `Service` only if needed for health port
- no `HTTPRoute`

- [ ] **Step 2: Build base manifests for divine-handle-gateway**

Use the internal HTTP service pattern:

- `Deployment`
- `Service`
- `ServiceAccount`
- no `HTTPRoute`

- [ ] **Step 3: Add staging overlays**

Set:

- staging image registry
- staging env values
- staging replica counts
- staging service account annotation if needed

- [ ] **Step 4: Add production overlays**

Set:

- production image registry
- production env values
- production replica counts

- [ ] **Step 5: Register both ArgoCD applications**

Follow the existing `ApplicationSet` pattern under `k8s/argocd/apps/`.

- [ ] **Step 6: Render all internal overlays**

Run:

```bash
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/staging >/tmp/divine-atbridge-staging.yaml
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/production >/tmp/divine-atbridge-production.yaml
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/staging >/tmp/divine-handle-gateway-staging.yaml
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/production >/tmp/divine-handle-gateway-production.yaml
```

- [ ] **Step 7: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/divine-atbridge k8s/applications/divine-handle-gateway k8s/argocd/apps/divine-atbridge.yaml k8s/argocd/apps/divine-handle-gateway.yaml
git -C ../divine-iac-coreconfig commit -m "feat: add internal divine-sky service deployments"
```

## Chunk 4: Add Public Service Deployments in divine-iac-coreconfig

### Task 8: Add public apps for divine-feedgen and divine-labeler

**Files:**
- Create: `../divine-iac-coreconfig/k8s/applications/divine-feedgen/base/{deployment.yaml,service.yaml,httproute.yaml,kustomization.yaml,serviceaccount.yaml}`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/staging/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/production/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-labeler/base/{deployment.yaml,service.yaml,httproute.yaml,kustomization.yaml,serviceaccount.yaml}`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-labeler/base/external-secret.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/staging/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/production/kustomization.yaml`
- Create: `../divine-iac-coreconfig/k8s/argocd/apps/divine-feedgen.yaml`
- Create: `../divine-iac-coreconfig/k8s/argocd/apps/divine-labeler.yaml`

- [ ] **Step 1: Build base manifests for divine-feedgen**

Include:

- `Deployment`
- `Service`
- `HTTPRoute`
- `ServiceAccount`

Public hosts:

- staging: `feed.staging.dvines.org`
- production: `feed.divine.video`

- [ ] **Step 2: Build base manifests for divine-labeler**

Include:

- `Deployment`
- `Service`
- `HTTPRoute`
- `ServiceAccount`
- `ExternalSecret` for signing key and webhook token

Public hosts:

- staging: `labeler.staging.dvines.org`
- production: `labeler.divine.video`

- [ ] **Step 3: Add staging overlays**

Set image registry, replica count, hostnames, env values, and service account annotations.

- [ ] **Step 4: Add production overlays**

Set production values and image references.

- [ ] **Step 5: Register both ArgoCD applications**

Follow the same `ApplicationSet` pattern as `keycast`.

- [ ] **Step 6: Render all public overlays**

Run:

```bash
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/staging >/tmp/divine-feedgen-staging.yaml
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/production >/tmp/divine-feedgen-production.yaml
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/staging >/tmp/divine-labeler-staging.yaml
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/production >/tmp/divine-labeler-production.yaml
```

- [ ] **Step 7: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/divine-feedgen k8s/applications/divine-labeler k8s/argocd/apps/divine-feedgen.yaml k8s/argocd/apps/divine-labeler.yaml
git -C ../divine-iac-coreconfig commit -m "feat: add public divine-sky service deployments"
```

## Chunk 5: Add Secret Wiring and Final Verification

### Task 9: Add external secret wiring and env-specific values in divine-iac-coreconfig

**Files:**
- Create or modify: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/base/external-secret.yaml`
- Create or modify: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/base/external-secret.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-labeler/base/external-secret.yaml`
- Modify: `../divine-iac-coreconfig/k8s/external-secrets/overlays/staging/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/external-secrets/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/docs/reference/deployments.md`
- Modify: `../divine-iac-coreconfig/docs/reference/service-repositories.md`

- [ ] **Step 1: Define secret-backed env for atbridge**

Include at minimum `DATABASE_URL` and `PDS_AUTH_TOKEN`.

- [ ] **Step 2: Define secret-backed env for handle-gateway**

Include:

- `DATABASE_URL`
- `KEYCAST_ATPROTO_TOKEN`
- `ATPROTO_PROVISIONING_TOKEN`
- `ATPROTO_NAME_SERVER_SYNC_TOKEN`

- [ ] **Step 3: Verify labeler secret template covers signing key and webhook token**

- [ ] **Step 4: Register the new apps in coreconfig docs**

Update deployment and repository reference docs so `divine-sky` stops being invisible to the platform repo.

- [ ] **Step 5: Render one final manifest sweep**

Run:

```bash
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/staging >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/staging >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/staging >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/staging >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/production >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/production >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/production >/dev/null
kubectl kustomize ../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/production >/dev/null
```

- [ ] **Step 6: Run final divine-sky verification**

Run:

```bash
cargo check -p divine-atbridge --all-targets
cargo check -p divine-handle-gateway --all-targets
cargo check -p divine-feedgen --all-targets
cargo check -p divine-labeler --all-targets
```

- [ ] **Step 7: Commit**

```bash
git -C ../divine-iac-coreconfig add k8s/applications ../divine-iac-coreconfig/k8s/external-secrets ../divine-iac-coreconfig/docs/reference
git -C ../divine-iac-coreconfig commit -m "feat: wire divine-sky staging and production secrets"
```
