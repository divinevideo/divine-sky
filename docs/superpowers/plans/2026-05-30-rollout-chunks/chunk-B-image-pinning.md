# Chunk B — Pin 5 Production Images (Implementation Sub-Plan)

> **Parent plan:** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk B, Task B1). Read it for context.
>
> **Editability:** `cross-repo-spec-only`. The image-tag edits land in the **sibling** repo `../divine-iac-coreconfig` (and the rsky read-only confirmation in `../rsky`). This doc is a **spec** — the agent executing it makes the edits in those sibling repos. The only file this plan itself touches in `divine-sky` is `docs/runbooks/launch-checklist.md` (Step F).
>
> **REQUIRED SUB-SKILL:** Use `superpowers:executing-plans` (or `superpowers:subagent-driven-development`) to run this task-by-task. Steps use `- [ ]` checkboxes.

---

## Goal

Replace `newTag: latest` with an **immutable** tag in all five ATProto production overlays so "what is deployed" becomes a known, reproducible fact. The rsky-pds tag in particular MUST be a build that contains the `/.well-known/oauth-protected-resource` endpoint (the protocol surface the whole rollout depends on).

The five services and their exact overlay files (all currently `newTag: latest`, verified 2026-05-30):

| Service | Overlay file (relative to `../divine-iac-coreconfig`) | Prod image (`newName`) |
|---|---|---|
| rsky-pds | `k8s/applications/rsky-pds/overlays/production/kustomization.yaml` | `us-central1-docker.pkg.dev/dv-platform-prod/containers-production/rsky-pds` |
| divine-atbridge | `k8s/applications/divine-atbridge/overlays/production/kustomization.yaml` | `.../containers-production/divine-atbridge` |
| divine-handle-gateway | `k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml` | `.../containers-production/divine-handle-gateway` |
| divine-feedgen | `k8s/applications/divine-feedgen/overlays/production/kustomization.yaml` | `.../containers-production/divine-feedgen` |
| divine-labeler | `k8s/applications/divine-labeler/overlays/production/kustomization.yaml` | `.../containers-production/divine-labeler` |

Each overlay has an `images:` entry of the form:
```yaml
images:
  - name: <service>
    newName: us-central1-docker.pkg.dev/dv-platform-prod/containers-production/<service>
    newTag: latest          # <-- this is what we replace
```
**Only `newTag` changes.** Do not touch `newName`, the `patches:`, or anything else.

### Why these five and not keycast

keycast prod is **already pinned** to `bd92361` (`k8s/applications/keycast/overlays/production/kustomization.yaml`) — leave it alone. These five are the remaining floating-`latest` services in the `sky`/ATProto path.

### Tag-format decision (do this once, apply to all five)

Two acceptable immutable forms exist in this repo today; pick **one** and use it consistently:

- **Short commit SHA** — what keycast prod uses (`newTag: bd92361`) and what staging uses (rsky-pds staging `newTag: video-auth-87151fc`, atbridge `3ba324c`, handle-gateway `15accd8`). Human-readable, traceable to a git commit. **Recommended** for parity with keycast.
- **Digest pin** — `newTag: <sometag>@sha256:<digest>` — what divine-labeler **staging** uses (`latest@sha256:9d28dec82fef3a76bad69fff408e08321184ce25780a614e60ea5bf9ffc5b04b`). Maximally immutable (a tag can be re-pushed; a digest cannot). Use this if a service's prod registry only has a moving tag and no per-commit tag.

> **Recommendation:** use the **short-SHA** form for all five for keycast parity, UNLESS the gcloud listing in Step A shows a service has no commit-style tag in prod — then digest-pin that one. Record the choice in the checklist (Step F).

> ⚠️ **Registry caveat:** prod images live in `dv-platform-prod/containers-production`, which is a **different** Artifact Registry repo from staging (`dv-platform-staging/containers-staging`). A staging tag like `3ba324c` is **not guaranteed** to exist in prod. You MUST list the **production** registry (Step A) and pin to a tag/digest that actually exists there. Do not copy a staging tag blind.

> **Note — image promotion is manual for these five.** `../divine-iac-coreconfig/.github/workflows/image-deploy.yaml` only auto-bumps `newTag` for `keycast`, `funnelcake`, `inquisitor`, `divine-push-service`, `divine-live-server`, `divine-connect`, `divine-brain`. The five ATProto services are **not** in that allow-list, so their prod tag is edited by hand in the overlay. That is exactly why they drifted to `latest`.

---

## Pre-req: environment / tooling

- [ ] **Step P1: Confirm tooling is present**

```bash
kustomize version            # expect: v5.x  (verified: v5.8.1)
gcloud --version | head -1   # expect: a Google Cloud SDK version line
git -C ../rsky rev-parse --abbrev-ref HEAD >/dev/null && echo "rsky repo reachable"
```
Expected: a kustomize version, a gcloud version, and `rsky repo reachable`.

- [ ] **Step P2: Authenticate to the prod Artifact Registry** (read access is enough)

```bash
gcloud auth configure-docker us-central1-docker.pkg.dev --quiet
gcloud config get-value project   # note the active project; do NOT assume — this rollout has been bitten by silent context switches
```
> ⚠️ Per project memory: `gcloud auth login` can silently switch your kube/gcloud context. Verify the active project before trusting any listing. Listing the registry by **fully-qualified path** (below) is project-context-independent, so prefer that.

---

## Task B1 — Pin the five overlays

### Step A: Discover the immutable tag for each service from the PROD registry

For each of the five services, list the tags that actually exist in prod, newest first, and capture both the tag and the digest:

- [ ] **Step A1: List rsky-pds prod tags**

```bash
gcloud artifacts docker tags list \
  us-central1-docker.pkg.dev/dv-platform-prod/containers-production/rsky-pds \
  --format='table(tag,version)' --sort-by=~CREATE_TIME --limit=20
```
Expected: a table of tags. Identify a **non-`latest`** tag that corresponds to a build of `divinevideo/main` at/after the protected-resource commit (confirmed in Step B). If only `latest` exists, fall back to the digest that `latest` currently points to:

```bash
gcloud artifacts docker images describe \
  us-central1-docker.pkg.dev/dv-platform-prod/containers-production/rsky-pds:latest \
  --format='value(image_summary.digest)'
```
Record the digest (`sha256:...`). You will pin to `latest@sha256:<digest>` if no commit-style tag exists.

- [ ] **Step A2: Repeat the listing for the other four**

```bash
for s in divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  echo "=== $s ==="
  gcloud artifacts docker tags list \
    us-central1-docker.pkg.dev/dv-platform-prod/containers-production/$s \
    --format='table(tag,version)' --sort-by=~CREATE_TIME --limit=10
done
```
Expected: a tag table per service. For each, choose the immutable tag/digest to pin (short-SHA preferred; digest fallback). **Write the five chosen values down** — you'll need them in Step C and Step F.

> If `gcloud artifacts docker tags list` errors with a permission denial, you lack registry read on the prod project. Stop and obtain read access; do **not** guess a tag.

### Step B: Confirm the chosen rsky-pds tag contains the protected-resource endpoint

The rsky-pds prod image MUST serve `/.well-known/oauth-protected-resource`. That endpoint was added on `divinevideo/main` in commit **`413fa351e6ffd3c332edafc6ddce34e9b52ffe9d`** ("feat: publish PDS OAuth protected-resource metadata (#3)"), which is the **current HEAD of `divinevideo/main`** (verified 2026-05-30).

- [ ] **Step B1: Confirm the source endpoint exists at that commit**

```bash
git -C ../rsky show 413fa351e6ffd3c332edafc6ddce34e9b52ffe9d:rsky-pds/src/well_known.rs \
  | grep -n 'oauth-protected-resource'
git -C ../rsky show 413fa351e6ffd3c332edafc6ddce34e9b52ffe9d:rsky-pds/src/lib.rs \
  | grep -n 'well_known::oauth_protected_resource'
```
Expected:
- `well_known.rs` line ~69: `#[rocket::get("/.well-known/oauth-protected-resource")]`
- `lib.rs` line ~408: `well_known::oauth_protected_resource,` (route is mounted, not just defined)

- [ ] **Step B2: Confirm HEAD of `divinevideo/main` is that commit (so a freshly-built prod image carries it)**

```bash
git -C ../rsky fetch divinevideo >/dev/null 2>&1
git -C ../rsky rev-parse divinevideo/main
```
Expected: `413fa351e6ffd3c332edafc6ddce34e9b52ffe9d` (or a **descendant** that still contains the endpoint — re-run B1's `git show` against the printed sha if it differs).

- [ ] **Step B3: Tie the chosen prod tag to that commit.** Two acceptable proofs, in order of preference:
  1. **Tag is a commit SHA** built from `divinevideo/main` at/after `413fa35`. Verify the SHA is an ancestor-or-equal of `divinevideo/main` AND contains the endpoint:
     ```bash
     CHOSEN_RSKY_SHA=<the short-or-full sha from Step A1>
     git -C ../rsky merge-base --is-ancestor "$CHOSEN_RSKY_SHA" 413fa351e6ffd3c332edafc6ddce34e9b52ffe9d \
       && echo "tag predates protected-resource — REJECT" \
       || echo "tag is at/after protected-resource — OK to check content"
     git -C ../rsky show "$CHOSEN_RSKY_SHA:rsky-pds/src/lib.rs" | grep -q 'oauth_protected_resource' \
       && echo "endpoint present in tag commit — ACCEPT" || echo "endpoint MISSING — REJECT"
     ```
     Expected: `tag is at/after protected-resource` **and** `endpoint present in tag commit — ACCEPT`.
  2. **Tag is a digest / opaque** (no recoverable SHA): defer the proof to the **live probe** — the running PDS built from this digest must answer the endpoint. This is exactly Chunk 0 / Chunk I Step 1:
     ```bash
     curl -isS https://pds.divine.video/.well-known/oauth-protected-resource | head -20
     ```
     Expected (200 + JSON): `{"resource":"https://pds.divine.video","authorization_servers":["https://entryway.divine.video"]}`.
     If this returns **404**, the currently-deployed `latest` predates the endpoint — see the master plan's Chunk 0 Step 2: **Chunk B becomes the blocking first step and you must build/push a fresh rsky-pds image from `413fa35` to prod before pinning.** Building/pushing the image is out of scope for this doc (it's a CI/registry action); record the gap and escalate.

### Step C: Edit `newTag` in each of the five production overlays

For each service, change only the `newTag:` line. Example for rsky-pds (short-SHA form):

```yaml
# ../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml
images:
  - name: rsky-pds
    newName: us-central1-docker.pkg.dev/dv-platform-prod/containers-production/rsky-pds
    newTag: 413fa35          # <-- was: latest  (use YOUR confirmed tag from Step A1/B3)
```
Digest-pin form (fallback):
```yaml
    newTag: latest@sha256:<digest-from-Step-A>
```

- [ ] **Step C1:** Edit `k8s/applications/rsky-pds/overlays/production/kustomization.yaml` — set `newTag` to the rsky-pds value confirmed in Step B3.
- [ ] **Step C2:** Edit `k8s/applications/divine-atbridge/overlays/production/kustomization.yaml` — set `newTag` to the divine-atbridge value from Step A2.
- [ ] **Step C3:** Edit `k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml` — set `newTag` to the divine-handle-gateway value from Step A2.
- [ ] **Step C4:** Edit `k8s/applications/divine-feedgen/overlays/production/kustomization.yaml` — set `newTag` to the divine-feedgen value from Step A2.
- [ ] **Step C5:** Edit `k8s/applications/divine-labeler/overlays/production/kustomization.yaml` — set `newTag` to the divine-labeler value from Step A2.

> ⚠️ Edit **only** the `newTag` scalar. Leave `newName`, the JSON6902 `patches`, the `labels`, and the `did:plc:PRODUCTION_DID_PLACEHOLDER` labeler patch untouched. Those are out of scope for image pinning.

### Step D: Render and assert NO `:latest` remains anywhere

- [ ] **Step D1: Per-service no-`latest` assertion** (this is the kustomize-build gate)

```bash
cd ../divine-iac-coreconfig
for s in rsky-pds divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  if kustomize build "k8s/applications/$s/overlays/production" | grep -qE 'image: .*:latest($|@)'; then
    echo "FAIL $s still :latest"
  else
    echo "ok $s"
  fi
done
```
Expected:
```
ok rsky-pds
ok divine-atbridge
ok divine-handle-gateway
ok divine-feedgen
ok divine-labeler
```
> The regex `:latest($|@)` matches a bare `...:latest` and a `...:latest@sha256:...` digest-pin **only if the human tag is still `latest`**. If you used the digest-pin fallback (`newTag: latest@sha256:...`) this assertion will report FAIL even though the image is immutable. In that case switch the assertion to confirm a digest is present instead:
> ```bash
> kustomize build k8s/applications/<svc>/overlays/production | grep -E 'image:' | grep -qE '@sha256:[0-9a-f]{64}' && echo "ok (digest-pinned)" || echo "FAIL no digest"
> ```

- [ ] **Step D2: Show the exact rendered image lines for the record**

```bash
cd ../divine-iac-coreconfig
for s in rsky-pds divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  echo "=== $s ==="
  kustomize build "k8s/applications/$s/overlays/production" | grep -E '^[[:space:]]*image:'
done
```
Expected: five image lines, each ending in your chosen immutable tag/digest — **none** ending in `:latest`. Paste this output into the checklist record (Step F).

- [ ] **Step D3: Validate the manifests still build cleanly** (no kustomize errors introduced)

```bash
cd ../divine-iac-coreconfig
for s in rsky-pds divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  kustomize build "k8s/applications/$s/overlays/production" >/dev/null && echo "build ok $s" || echo "BUILD ERROR $s"
done
```
Expected: `build ok` for all five.

### Step E: Commit in the IAC sibling repo

> This commit is in `../divine-iac-coreconfig`, **not** divine-sky. Branch first if on the default branch.

- [ ] **Step E1: Stage and commit**

```bash
git -C ../divine-iac-coreconfig add \
  k8s/applications/rsky-pds/overlays/production/kustomization.yaml \
  k8s/applications/divine-atbridge/overlays/production/kustomization.yaml \
  k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml \
  k8s/applications/divine-feedgen/overlays/production/kustomization.yaml \
  k8s/applications/divine-labeler/overlays/production/kustomization.yaml
git -C ../divine-iac-coreconfig commit -m "chore: pin atproto production images to immutable tags

rsky-pds pinned to a build containing /.well-known/oauth-protected-resource
(divinevideo/main @ 413fa35). atbridge/handle-gateway/feedgen/labeler pinned
to their current prod-registry tags. No prod overlay ships :latest.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
Expected: one commit touching exactly five files. (Open a PR per repo convention — ArgoCD syncs from main.)

### Step F: Record the pinned tags + make `:latest` a launch hard-stop (divine-sky doc)

This is the only step that edits **this** (divine-sky) repo: `docs/runbooks/launch-checklist.md`.

- [ ] **Step F1: Add a "Pinned production images" record** to `docs/runbooks/launch-checklist.md` — a table of the five services and the exact immutable tag/digest each is pinned to (the values from Step A/D2), plus the date and the rsky-pds protected-resource confirmation (commit `413fa35`, live-probe 200).

- [ ] **Step F2: Add a hard-stop line** to the checklist: *"BLOCKING: no production overlay may ship `newTag: latest`. Re-run the Chunk B Step D1 assertion before any prod deploy; a `FAIL` is a launch stop."*

- [ ] **Step F3: Commit in divine-sky**

```bash
git -C /Users/rabble/code/divine/divine-sky add docs/runbooks/launch-checklist.md
git -C /Users/rabble/code/divine/divine-sky commit -m "docs: record pinned prod image tags; treat :latest as launch hard-stop

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Done-when (acceptance criteria)

- [ ] All five prod overlays have `newTag` set to an immutable value (short SHA or `@sha256:` digest); none is `latest`.
- [ ] `kustomize build` of each prod overlay succeeds and renders **no** `:latest` image (Step D1/D3 green).
- [ ] The rsky-pds pinned tag is proven to contain `/.well-known/oauth-protected-resource` — by SHA-ancestry+content (Step B3.1) **or** by a live 200 from `https://pds.divine.video/.well-known/oauth-protected-resource` (Step B3.2).
- [ ] IAC commit made (Step E); divine-sky checklist updated with the pinned-tag record + hard-stop (Step F).

## Risks / gotchas

- **404 on the live protected-resource probe** ⇒ deployed `latest` predates the endpoint. Per master-plan Chunk 0, Chunk B then becomes the **blocking first** step: a fresh rsky-pds image from `413fa35` must be built and pushed to `containers-production` **before** pinning. That build/push is a CI/registry action outside this doc — escalate, don't fake a tag.
- **Staging tag ≠ prod tag.** Prod and staging are different Artifact Registry repos. Always list the **prod** registry (Step A); never copy a staging tag.
- **Digest-pin breaks the `:latest` grep.** If you use `newTag: latest@sha256:...`, swap to the digest-presence assertion in Step D1 — otherwise it false-FAILs.
- **gcloud context drift** (project memory): verify the active project before trusting any `gcloud` listing; prefer fully-qualified registry paths.
- **Scope creep.** Edit only `newTag`. The labeler overlay still carries `did:plc:PRODUCTION_DID_PLACEHOLDER` — that is a separate gap, **not** part of image pinning.
