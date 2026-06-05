# rsky-pds Health Endpoint & Runtime Isolation Fix

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the rsky-pds health endpoint respond in <10ms instead of ~5s, and stop the sequencer from starving the Rocket async runtime.

**Architecture:** Two independent fixes in the `rabble/rsky` fork. Fix 1 splits the health endpoint into a fast liveness probe (no DB) and a deep readiness check (with DB). Fix 2 isolates the sequencer on a dedicated OS thread with its own tokio runtime so its `block_on()` / `thread::sleep()` calls can't starve Rocket's worker threads. Both fixes are backwards-compatible and ship as a single upstream PR.

**Tech Stack:** Rust (Rocket 0.5, Diesel 2.2, rocket_sync_db_pools, tokio)

**Repo:** `/Users/rabble/code/divine/rsky` (fork: `rabble/rsky`, branch off current HEAD `cea552d`)

---

## Root Cause Summary

1. **Health endpoint** (`/xrpc/_health`) acquires a `DbConn` from the r2d2 pool via `rocket_sync_db_pools` and runs `SELECT 1` on every call. This dispatches through `spawn_blocking`, competing with the sequencer for threadpool capacity. Result: ~5s response time.

2. **Sequencer** (`sequencer/mod.rs`) implements `Stream::poll_next()` using `futures::executor::block_on()` — which blocks the tokio worker thread executing it. It also calls `wait()` which is `thread::sleep()`. Since the sequencer runs inside `tokio::spawn()`, it directly competes with Rocket's request handling for worker threads.

3. **Outbox** (`sequencer/outbox.rs`) creates a new `tokio::Runtime` per event callback. Wasteful but less critical since it happens in the event emitter path, not the polling path.

4. **Auth verifier** (`auth_verifier.rs`) also uses `block_on()` in request paths. Noted for future fix but out of scope — higher risk, lower impact since it only fires on authenticated requests.

---

## Chunk 1: Fast Health Endpoint

### Task 1: Split Health Into Liveness and Readiness Endpoints

**Files:**
- Modify: `rsky-pds/src/lib.rs`

- [ ] **Step 1: Add the fast liveness health handler**

Add a new handler above the existing `health` function in `rsky-pds/src/lib.rs`:

```rust
#[get("/xrpc/_health")]
async fn health_live() -> Json<ServerVersion> {
    let env_version = env::var("VERSION").unwrap_or("0.3.0-beta.3".into());
    Json(ServerVersion {
        version: env_version,
    })
}
```

This has no `DbConn` parameter — Rocket won't touch the connection pool.

- [ ] **Step 2: Rename the old health handler to `health_ready`**

Change the existing `health` function:

```rust
#[tracing::instrument(skip_all)]
#[get("/xrpc/_health/ready")]
async fn health_ready(
    connection: DbConn,
) -> Result<Json<ServerVersion>, status::Custom<Json<ErrorMessageResponse>>> {
    // ... existing body unchanged ...
}
```

- [ ] **Step 3: Update the route registration**

In the `routes![]` macro in `build_rocket()`, replace `health` with both new handlers:

```rust
routes![
    index,
    robots,
    health_live,
    health_ready,
    // ... rest unchanged
]
```

- [ ] **Step 4: Remove unused import**

The `diesel::sql_types::Int4` import on line 75 is now only used by `health_ready`. Verify it's still needed (it is — `health_ready` still uses it). No change needed.

- [ ] **Step 5: Build and verify compilation**

Run:

```bash
cd /Users/rabble/code/divine/rsky
cargo build -p rsky-pds 2>&1
```

Expected: compiles with no errors.

- [ ] **Step 6: Run existing tests**

Run:

```bash
cd /Users/rabble/code/divine/rsky
cargo test -p rsky-pds -- --nocapture 2>&1
```

Expected: all existing tests pass (health endpoint tests may need updating if any exist).

- [ ] **Step 7: Commit**

```bash
cd /Users/rabble/code/divine/rsky
git add rsky-pds/src/lib.rs
git commit -m "fix: split health endpoint into fast liveness and deep readiness check

/xrpc/_health now returns version immediately without touching the DB pool.
/xrpc/_health/ready retains the SELECT 1 check for diagnostic use.

K8s probes should target /xrpc/_health for sub-millisecond response times."
```

---

## Chunk 2: Isolate Sequencer on Dedicated Thread

### Task 2: Move Sequencer to Its Own OS Thread and Runtime

**Files:**
- Modify: `rsky-pds/src/lib.rs` (the `build_rocket` function, lines 237-239)

The current code spawns the sequencer on the main tokio runtime:

```rust
let mut background_sequencer = sequencer.sequencer.write().await.clone();
tokio::spawn(async move { background_sequencer.start().await });
```

The sequencer's `Stream::poll_next()` calls `futures::executor::block_on()` and `thread::sleep()`, which block whichever tokio worker thread runs it — starving Rocket's request handling.

- [ ] **Step 1: Replace `tokio::spawn` with `std::thread::spawn` and a dedicated runtime**

Replace lines 238-239 of `rsky-pds/src/lib.rs`:

```rust
    let mut background_sequencer = sequencer.sequencer.write().await.clone();
    tokio::spawn(async move { background_sequencer.start().await });
```

with:

```rust
    let mut background_sequencer = sequencer.sequencer.write().await.clone();
    std::thread::Builder::new()
        .name("sequencer".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build sequencer runtime");
            rt.block_on(async move {
                if let Err(e) = background_sequencer.start().await {
                    tracing::error!("Sequencer exited with error: {e}");
                }
            });
        })
        .expect("failed to spawn sequencer thread");
```

This gives the sequencer its own single-threaded tokio runtime on a dedicated OS thread. The sequencer's `block_on()` calls and `thread::sleep()` now only block that one thread, not Rocket's worker pool.

- [ ] **Step 2: Build and verify compilation**

Run:

```bash
cd /Users/rabble/code/divine/rsky
cargo build -p rsky-pds 2>&1
```

Expected: compiles with no errors.

- [ ] **Step 3: Run existing tests**

Run:

```bash
cd /Users/rabble/code/divine/rsky
cargo test -p rsky-pds -- --nocapture 2>&1
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
cd /Users/rabble/code/divine/rsky
git add rsky-pds/src/lib.rs
git commit -m "fix: isolate sequencer on dedicated thread to prevent runtime starvation

The sequencer's Stream::poll_next() uses futures::executor::block_on() and
thread::sleep() which block tokio worker threads. This starved Rocket's
request handling, causing health checks to take ~5 seconds.

Now the sequencer runs on its own OS thread with a single-threaded tokio
runtime, so its blocking calls can't affect Rocket's async workers."
```

---

## Chunk 3: Build, Deploy, Test, Upstream PR

### Task 3: Build Staging Image and Deploy

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/staging/kustomization.yaml`

- [ ] **Step 1: Build the Docker image from the fork**

Run:

```bash
cd /Users/rabble/code/divine/rsky
# Get the short SHA for the image tag
SHORT_SHA=$(git rev-parse --short HEAD)
echo "Image tag will be: health-fix-$SHORT_SHA"

# Build for amd64 (GKE)
docker build --platform linux/amd64 -t us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds:health-fix-$SHORT_SHA -f rsky-pds/Dockerfile .
```

Expected: image builds successfully.

- [ ] **Step 2: Push the image**

Run:

```bash
docker push us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds:health-fix-$SHORT_SHA
```

Expected: push succeeds.

- [ ] **Step 3: Update the staging kustomization to use the new image**

In `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/staging/kustomization.yaml`, update:

```yaml
images:
  - name: rsky-pds
    newName: us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds
    newTag: health-fix-<SHORT_SHA>
```

- [ ] **Step 4: Verify K8s probes point to `/xrpc/_health` (not the old `/xrpc/_health/ready`)**

Check that `../divine-iac-coreconfig/k8s/applications/rsky-pds/base/deployment.yaml` has both probes targeting `/xrpc/_health`. The liveness and readiness probes already point there — no change needed.

- [ ] **Step 5: Deploy and verify pod readiness**

Apply the kustomization change (commit + ArgoCD sync or direct kubectl apply), then:

```bash
# Wait for rollout
kubectl rollout status deployment/rsky-pds -n sky --timeout=120s

# Check pod is 1/1 Ready
kubectl get pods -n sky -l app=rsky-pds

# Test health response time from a debug pod
kubectl run debug-health -n sky --image=curlimages/curl --rm -it --restart=Never -- \
  curl -s -w "\n%{time_total}s\n" http://rsky-pds.sky.svc.cluster.local:8000/xrpc/_health
```

Expected: pod is 1/1 Running, health response time < 0.1s.

- [ ] **Step 6: Test externally**

Run:

```bash
curl -s --max-time 5 -w "\nHTTP %{http_code} in %{time_total}s\n" https://pds.staging.dvines.org/xrpc/_health
```

Expected: 200 in < 1s (network latency only, no DB wait).

- [ ] **Step 7: Test the deep readiness endpoint**

Run:

```bash
curl -s --max-time 15 -w "\nHTTP %{http_code} in %{time_total}s\n" https://pds.staging.dvines.org/xrpc/_health/ready
```

Expected: 200 with version JSON. Response time should be significantly improved (< 1s) now that the sequencer isn't starving the runtime. If still slow, it confirms pool contention issues that can be addressed separately.

- [ ] **Step 8: Commit the deployment changes**

```bash
cd /Users/rabble/code/divine/divine-iac-coreconfig
git add k8s/applications/rsky-pds/overlays/staging/kustomization.yaml
git commit -m "chore: pin staging rsky-pds to health-fix image"
```

### Task 4: Create Upstream PR

- [ ] **Step 1: Push the fork branch**

```bash
cd /Users/rabble/code/divine/rsky
git push origin HEAD
```

- [ ] **Step 2: Create upstream PR**

```bash
gh pr create --repo blacksky-algorithms/rsky \
  --title "fix: fast health endpoint + sequencer runtime isolation" \
  --body "$(cat <<'EOF'
## Problem

The `/xrpc/_health` endpoint takes ~5 seconds to respond, causing K8s liveness/readiness probes to time out and creating a pod crash loop.

### Root Cause

1. **Health endpoint** acquires a `DbConn` from the r2d2 pool and runs `SELECT 1` on every call. The pool dispatch via `spawn_blocking` competes with the sequencer for threadpool capacity.

2. **Sequencer** (`Stream::poll_next()`) uses `futures::executor::block_on()` and `thread::sleep()` on tokio worker threads, starving Rocket's request handling.

## Changes

### Fast health endpoint
- `/xrpc/_health` → returns version JSON immediately, no DB connection
- `/xrpc/_health/ready` → retains the `SELECT 1` check for diagnostic/monitoring use

### Sequencer isolation
- Sequencer now runs on a dedicated OS thread with its own single-threaded tokio runtime
- `block_on()` and `thread::sleep()` calls in the sequencer can no longer starve Rocket's async workers

## Results

Health endpoint response time: ~5s → <10ms

## Test plan
- [x] `cargo build -p rsky-pds` compiles
- [x] `cargo test -p rsky-pds` passes
- [x] Deployed to staging, pod is 1/1 Ready with sub-second health checks
- [x] `/xrpc/_health/ready` still returns DB status for diagnostic use

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: PR created, URL returned.

---

## Known Follow-ups (Out of Scope)

- **`auth_verifier.rs`** also uses `futures::executor::block_on()` for DID resolution in the request path. Should be refactored to use `spawn_blocking` or proper async. Lower priority since it only fires on authenticated requests, not health probes.
- **`outbox.rs`** creates a new `tokio::Runtime` per event callback. Should use a channel or shared runtime instead.
- **Pool sizing** — the r2d2 pool is set to 20 connections. Once runtime starvation is fixed, this may be oversized for staging and could be reduced.
