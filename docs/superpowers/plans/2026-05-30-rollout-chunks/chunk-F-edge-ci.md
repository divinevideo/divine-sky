# Chunk F — Public-Edge Deploy Hygiene (CI + secrets + deploy order)

> **Sub-plan of** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk F, Task F1).
> **Editability:** `cross-repo-spec-only`. The *implementation* lands in sibling repos
> `divine-router` and `divine-name-server`. **This worker MUST NOT edit those siblings** —
> it only reads them and writes this divine-sky-side spec. Whoever executes this plan does so
> from inside the sibling repos (or via a PR to them), not from divine-sky.
>
> **REQUIRED SUB-SKILL:** use `superpowers:executing-plans` (or `subagent-driven-development`) to
> run this task-by-task. Steps are checkboxes (`- [ ]`).

---

## Why this chunk exists

The public edge is the **only** tier outside ArgoCD/GitOps. Today it drifts silently:

1. **`divine-router` has no deploy CI.** Its only workflow is `semantic_pr.yml`
   (`/Users/rabble/code/divine/divine-router/.github/workflows/semantic_pr.yml`). Every
   production publish is a human running `fastly compute publish` from a laptop. There is no
   build/test gate and no audit trail.
2. **`divine-name-server`'s Fastly write credential is an undocumented manual secret.** The
   worker pushes handle/atproto state into Fastly KV via the Fastly REST API
   (`/Users/rabble/code/divine/divine-name-server/src/utils/fastly-sync.ts`), authenticating with
   `FASTLY_API_TOKEN`. That value is set by hand
   (`npx wrangler secret put FASTLY_API_TOKEN`) and is **not** referenced in the deploy workflow
   (`/Users/rabble/code/divine/divine-name-server/.github/workflows/deploy.yml`) nor enumerated
   anywhere as a required secret. If it rotates or is lost, the read model the router serves goes
   stale with no signal.
3. **The deploy ORDER is implicit.** The router reads a Fastly KV store the name-server writes.
   Publishing the router before the name-server has populated/reconciled KV serves a stale or
   empty read model. Today this ordering lives only as one prose sentence in each README.

This chunk makes the edge **reproducible and ordered**: add router CI, document every required
secret on both sides, and pin the deploy order into the runbook.

---

## Verified ground truth (read before writing anything)

All facts below were confirmed by reading the deployed branches on 2026-05-30. **Reference the
deployed branch, never a local worktree.**

### divine-name-server (Cloudflare Worker)
- Deployed branch: `origin/main`. Local checkout was on `feat/admin-restore-endpoint` @ `523ab19`
  during audit — **do not treat that as the deploy ref**; the workflow deploys on push to `main`.
- Deploy CI: `.github/workflows/deploy.yml` — triggers on `push` to `main`, builds `admin-ui`,
  deploys with `cloudflare/wrangler-action@v3`. Secrets it consumes today:
  `secrets.CLOUDFLARE_API_TOKEN`, `secrets.CLOUDFLARE_ACCOUNT_ID`.
- `wrangler.toml` bindings:
  - D1: `binding = "DB"`, `database_name = "divine-name-server-db"`,
    `database_id = "e7e081c4-830d-449c-9de5-d93eaacefb34"`.
  - KV (Cloudflare, session): `binding = "SESSION_KV"`, `id = "f134d6b3b36b4890a0a272e6ca392bcd"`.
  - `[vars] FASTLY_STORE_ID = "gclbp6suv4bjnqpctp2b7n"` — the **Fastly** KV store the worker writes to.
  - Hourly reconciliation cron: `[triggers] crons = ["0 * * * *"]` — re-syncs D1 → Fastly KV.
- Fastly write path: `src/utils/fastly-sync.ts` hits
  `https://api.fastly.com/resources/stores/kv/${FASTLY_STORE_ID}/keys/${key}` with header
  `Fastly-Key: ${FASTLY_API_TOKEN}`. Env type (`src/index.ts:20-21`) declares both
  `FASTLY_API_TOKEN?: string` and `FASTLY_STORE_ID?: string`. When either is unset, sync is a
  no-op (logs an error) — i.e. **a missing token silently stops KV propagation**.
- `wrangler.toml` already documents (in comments) that `FASTLY_API_TOKEN` is a
  `wrangler secret put` value and that a stray `FASTLY_STORE_ID` *secret* would shadow the var.

### divine-router (Fastly Compute@Edge, Rust → wasm32-wasip1)
- Deployed branch: `origin/main`, HEAD `ab7f623` ("remove dvine.video from owned-domain allow-list").
- Workflows present: **only** `.github/workflows/semantic_pr.yml`. **No deploy workflow exists.**
- `fastly.toml`: `name = "divine-router"`, `service_id = "76fTayX6mBKa8faLeZ1fet"`.
  Build script: `cargo build --profile release --target wasm32-wasip1`.
- Toolchain (`rust-toolchain.toml`): channel `1.83.0`, target `wasm32-wasip1`.
- KV: router reads a Fastly KV store. **KV-name blocker (owned by Chunk C, not F):**
  `src/main.rs:15` opens `KV_STORE_NAME = "divine-names"` (used at `src/main.rs:395`
  `KVStore::open(KV_STORE_NAME)`), but `fastly.toml` declares the store as `usernames` with **no
  `store_id`** (the `[setup.kv_stores.usernames]` block is truncated/empty at end of file). This
  mismatch is **Chunk C's fix** — Chunk F must not duplicate it, but the router CI workflow below
  must NOT mask it: the publish step will surface a KV-binding error until Chunk C lands.

### The data-flow that fixes the deploy order
```
divine-name-server (CF Worker)                      divine-router (Fastly Compute)
  D1 "divine-name-server-db" (source of truth)
        │  write/admin/cron("0 * * * *")
        ▼
  Fastly REST API (FASTLY_API_TOKEN)
        │  PUT /resources/stores/kv/gclbp6suv4bjnqpctp2b7n/keys/<username>
        ▼
  Fastly KV store gclbp6suv4bjnqpctp2b7n  ◄────────── KVStore::open(...) reads same store
                                                        serves /.well-known/atproto-did + nostr.json
```
**Order: name-server populates/reconciles Fastly KV FIRST, then router publishes.** Publishing the
router first serves a stale/empty read model.

---

## Scope / non-goals

- **In scope (F):** router deploy CI workflow; a single canonical "required edge secrets" reference;
  documented deploy order in the divine-sky runbook.
- **Out of scope:** the KV store-name/`store_id` mismatch (**Chunk C**); image pinning (**Chunk B**);
  disable→404 abuse contract (**Chunk H/C**). Do not fix those here. Where F's steps touch the same
  files, they only *document* the Chunk C dependency.

---

## Pre-req: confirm you are on deployed refs (do this first)

- [ ] **Step 0.1 — confirm router deploy ref**

```bash
git -C /Users/rabble/code/divine/divine-router fetch origin
git -C /Users/rabble/code/divine/divine-router log --oneline origin/main -1
```
Expected: top commit is `ab7f623 fix(router): remove dvine.video from owned-domain allow-list (#9)`
(or newer). If the SHA differs, re-read `fastly.toml`/`src/main.rs` before trusting the facts above.

- [ ] **Step 0.2 — confirm name-server deploy ref + Fastly store id**

```bash
git -C /Users/rabble/code/divine/divine-name-server fetch origin
git -C /Users/rabble/code/divine/divine-name-server show origin/main:wrangler.toml | grep -E 'FASTLY_STORE_ID|database_id|name ='
```
Expected output contains:
```
name = "divine-name-server"
database_id = "e7e081c4-830d-449c-9de5-d93eaacefb34"
FASTLY_STORE_ID = "gclbp6suv4bjnqpctp2b7n"
```
If `FASTLY_STORE_ID` differs, use the value from `origin/main` everywhere below.

---

## Task F1: add a Fastly publish CI workflow to divine-router

> Implement in the **`divine-router`** repo (NOT divine-sky). Open a PR there.

**Files (in divine-router):**
- Create: `.github/workflows/fastly-publish.yml`

- [ ] **Step F1.1 — author the workflow.** Create
  `/Users/rabble/code/divine/divine-router/.github/workflows/fastly-publish.yml` with exactly:

```yaml
# ABOUTME: CI that builds and publishes divine-router to Fastly Compute@Edge.
# ABOUTME: PRs build+test only; pushes to main publish. Service id lives in fastly.toml.

name: Fastly Publish

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

permissions:
  contents: read

concurrency:
  group: fastly-publish-${{ github.ref }}
  cancel-in-progress: false

jobs:
  build-and-publish:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust toolchain (from rust-toolchain.toml)
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-wasip1

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Build (wasm32-wasip1, release)
        run: cargo build --profile release --target wasm32-wasip1

      - name: Test
        run: cargo test

      - name: Install Fastly CLI
        run: |
          curl -L https://github.com/fastly/cli/releases/latest/download/fastly_$(uname -s)_$(uname -m).tar.gz -o fastly.tar.gz
          tar -xzf fastly.tar.gz fastly
          sudo mv fastly /usr/local/bin/fastly
          fastly version

      - name: Publish to Fastly (main only)
        if: github.ref == 'refs/heads/main' && github.event_name == 'push'
        env:
          FASTLY_API_TOKEN: ${{ secrets.FASTLY_API_TOKEN }}
        run: |
          fastly compute publish --non-interactive --token "$FASTLY_API_TOKEN"
          fastly purge --all --service-id 76fTayX6mBKa8faLeZ1fet --token "$FASTLY_API_TOKEN"
```

  Notes the implementer must verify against the repo:
  - `cargo build --profile release --target wasm32-wasip1` is copied verbatim from
    `fastly.toml`'s `[scripts] build`. The toolchain channel (`1.83.0`) and target come from
    `rust-toolchain.toml`; `dtolnay/rust-toolchain@stable` will honor `rust-toolchain.toml` if
    present, otherwise pin `toolchain: 1.83.0`.
  - `service_id` `76fTayX6mBKa8faLeZ1fet` is read from `fastly.toml`. `fastly compute publish`
    already reads it from `fastly.toml`; it is repeated on `purge` because `purge` needs it
    explicitly.
  - The publish step is gated to `push` on `main` so PRs build+test but never deploy.

- [ ] **Step F1.2 — verify the workflow locally before pushing.**

```bash
cd /Users/rabble/code/divine/divine-router
# YAML well-formed:
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/fastly-publish.yml')); print('yaml ok')"
# Build reproduces what CI runs:
rustup target add wasm32-wasip1
cargo build --profile release --target wasm32-wasip1
cargo test
```
Expected: `yaml ok`; release wasm build succeeds; tests pass. **If the build/test fails because of
the KV store-name mismatch (`divine-names` vs `usernames`), that is the Chunk C blocker — note it
and proceed; do NOT fix it here.**

- [ ] **Step F1.3 — register the `FASTLY_API_TOKEN` repo secret in divine-router.** This is the
  same Fastly account token the name-server uses to write KV; for the router it authenticates
  `compute publish` + `purge`. (See Task F2 for the canonical secret table.)

```bash
gh secret set FASTLY_API_TOKEN --repo divinevideo/divine-router
# paste the Fastly automation token when prompted
gh secret list --repo divinevideo/divine-router | grep FASTLY_API_TOKEN
```
Expected: `FASTLY_API_TOKEN` appears in the secret list. Confirm the actual GitHub org/repo slug
with `gh repo view --json nameWithOwner` from inside the router checkout before running.

- [ ] **Step F1.4 — open the PR in divine-router** (do not merge straight to main):

```bash
cd /Users/rabble/code/divine/divine-router
git checkout -b ci/fastly-publish-workflow
git add .github/workflows/fastly-publish.yml
git commit -m "ci: add fastly compute publish workflow gated on main"
git push -u origin ci/fastly-publish-workflow
gh pr create --fill --base main
```
Expected: PR opens; the `Fastly Publish` job runs build+test (no publish, since it's a PR).

---

## Task F2: document every required edge secret (canonical table)

> Implement in the **sibling READMEs** (divine-router + divine-name-server) AND record the same
> table in the divine-sky runbook (Task F3). The READMEs are the per-repo home; the runbook is the
> single operator reference.

**Files:**
- Modify (in divine-router): `README.md` — add a "Required CI secrets" section.
- Modify (in divine-name-server): `README.md` — add/extend a "Required secrets" section.
- (Runbook copy handled in Task F3.)

- [ ] **Step F2.1 — write the canonical secrets table.** Use exactly this content (it is the
  source of truth for F3 as well):

| Repo | Secret / var | Kind | Where set | Purpose | Verified source |
|---|---|---|---|---|---|
| divine-name-server | `CLOUDFLARE_API_TOKEN` | GH Actions secret | repo secret | Worker deploy (`wrangler-action`) | `deploy.yml:39` |
| divine-name-server | `CLOUDFLARE_ACCOUNT_ID` | GH Actions secret | repo secret | Worker deploy account scope | `deploy.yml:40` |
| divine-name-server | `FASTLY_API_TOKEN` | Worker runtime secret | `wrangler secret put FASTLY_API_TOKEN` | Auth Fastly REST writes to KV | `src/utils/fastly-sync.ts`, `src/index.ts:20` |
| divine-name-server | `FASTLY_STORE_ID` | `[vars]` (non-secret) | `wrangler.toml` (`gclbp6suv4bjnqpctp2b7n`) | Target Fastly KV store id | `wrangler.toml:29` |
| divine-name-server | `DB` (D1 `divine-name-server-db`, `e7e081c4-…`) | binding | `wrangler.toml` | Source-of-truth registry | `wrangler.toml:38-41` |
| divine-name-server | `SESSION_KV` (CF KV `f134d6b3…`) | binding | `wrangler.toml` | Admin session store | `wrangler.toml:34-36` |
| divine-router | `FASTLY_API_TOKEN` | GH Actions secret | repo secret | `compute publish` + `purge` | `fastly-publish.yml` (Task F1) |
| divine-router | Fastly KV store binding | Fastly resource | `fastly.toml` `[setup.kv_stores]` + `store_id` | Read handle/atproto state | `fastly.toml`, `src/main.rs:15,395` — **store name/id mismatch is Chunk C** |
| divine-router | `service_id` `76fTayX6mBKa8faLeZ1fet` | `fastly.toml` (non-secret) | `fastly.toml` | Target Fastly service | `fastly.toml` |

  Operator rules to include verbatim in the README sections:
  - **`FASTLY_API_TOKEN` appears in two places** for two distinct uses. Both reference the same
    Fastly account but are stored independently: name-server as a *Worker runtime secret*
    (`wrangler secret put`), router as a *GitHub Actions secret* (`gh secret set`). Rotating the
    Fastly token requires updating **both**.
  - **Never store `FASTLY_STORE_ID` as a `wrangler secret`** — a same-named secret shadows the
    `[vars]` value and is hard to debug (already warned in `wrangler.toml` comments). It is a
    non-secret resource id; keep it in `[vars]`.
  - A missing/expired `FASTLY_API_TOKEN` on the name-server side does **not** error loudly — KV
    sync becomes a silent no-op. Operators must verify propagation (Task F3 Step F3.3), not assume.

- [ ] **Step F2.2 — verify each cited line still says what the table claims** (guards against drift):

```bash
git -C /Users/rabble/code/divine/divine-name-server show origin/main:.github/workflows/deploy.yml | grep -nE 'CLOUDFLARE_API_TOKEN|CLOUDFLARE_ACCOUNT_ID'
git -C /Users/rabble/code/divine/divine-name-server show origin/main:src/index.ts | grep -nE 'FASTLY_API_TOKEN|FASTLY_STORE_ID'
git -C /Users/rabble/code/divine/divine-router show origin/main:fastly.toml | grep -nE 'service_id|kv_stores'
git -C /Users/rabble/code/divine/divine-router show origin/main:src/main.rs | grep -nE 'KV_STORE_NAME|KVStore::open'
```
Expected: each grep returns the lines cited in the table. If any has moved, update the table's
"Verified source" column before committing.

- [ ] **Step F2.3 — commit each README in its own repo** (separate PRs):

```bash
# divine-router
cd /Users/rabble/code/divine/divine-router && git add README.md \
  && git commit -m "docs: document required FASTLY_API_TOKEN ci secret + deploy order"
# divine-name-server
cd /Users/rabble/code/divine/divine-name-server && git add README.md \
  && git commit -m "docs: enumerate required fastly/cloudflare secrets"
```

---

## Task F3: pin the deploy ORDER into the divine-sky runbook

> This is the **only** file edited inside divine-sky. It is the operator's single source for edge
> deploys.

**Files (in divine-sky):**
- Modify: `/Users/rabble/code/divine/divine-sky/docs/runbooks/login-divine-video.md`

- [ ] **Step F3.1 — add a "Public-edge deploy order" section** to `login-divine-video.md` with the
  data-flow diagram from "Verified ground truth" above and this ordered procedure:

  1. **name-server first.** Merge to `divinevideo/divine-name-server` `main` → `deploy.yml`
     deploys the Worker. The Worker (and the hourly `0 * * * *` cron) reconcile D1 → Fastly KV
     store `gclbp6suv4bjnqpctp2b7n` using `FASTLY_API_TOKEN`. **Confirm KV is current before
     touching the router** (Step F3.3).
  2. **router second.** Merge to `divinevideo/divine-router` `main` → `fastly-publish.yml`
     publishes Compute service `76fTayX6mBKa8faLeZ1fet` and purges cache. The router reads the
     **already-populated** KV store and serves `/.well-known/atproto-did` + `/.well-known/nostr.json`.
  3. **Rule:** never publish the router expecting *new* handle state that the name-server has not
     yet written. The router is a read-only edge over the name-server's KV write model.

- [ ] **Step F3.2 — copy the canonical secrets table** (from Task F2.1) into the runbook so
  operators don't have to open two READMEs.

- [ ] **Step F3.3 — add the propagation verification commands** the order depends on:

```bash
# After name-server deploy: confirm a known-ready handle is in Fastly KV (router-visible).
# Use a username known to be status=active, atproto_state=ready.
curl -fsS "https://<ready-username>.divine.video/.well-known/atproto-did"
# Expect: the user's did:plc:... on a single line.

# A not-ready / disabled handle must NOT resolve (this is the safety contract shared with Chunk H/C):
curl -s -o /dev/null -w '%{http_code}\n' "https://<not-ready>.divine.video/.well-known/atproto-did"
# Expect: 404
```
Expected: ready handle returns its DID; not-ready returns `404`. If the ready handle 404s **after**
a name-server deploy, KV propagation failed — check `FASTLY_API_TOKEN` validity and the
name-server logs for the silent-no-op error before publishing the router.

- [ ] **Step F3.4 — note the Chunk C dependency explicitly** in the runbook: until Chunk C
  reconciles the router's KV store name (`divine-names` in `src/main.rs:15` vs `usernames` in
  `fastly.toml`) and sets the real `store_id`, the router publish will fail to open KV and serve no
  handle data. Chunk F's CI surfaces that failure; it does not hide it.

- [ ] **Step F3.5 — commit (in divine-sky):**

```bash
cd /Users/rabble/code/divine/divine-sky
git add docs/runbooks/login-divine-video.md docs/superpowers/plans/2026-05-30-rollout-chunks/chunk-F-edge-ci.md
git commit -m "docs(rollout): chunk F edge CI sub-plan + deploy-order runbook"
```

---

## Self-review (coverage vs master plan Task F1)

- Router `fastly compute publish` CI workflow, gated on main → **Task F1** ✓
- Document required secrets (name-server `FASTLY_API_TOKEN`/`FASTLY_STORE_ID`/D1; router Fastly + KV)
  → **Task F2** canonical table ✓
- Document deploy ORDER (name-server writes state first, then router) → **Task F3** ✓
- Commit in each repo + this one → F1.4 (router), F2.3 (both READMEs), F3.5 (divine-sky) ✓
- Cross-repo-spec-only honored: this worker only *reads* siblings; all sibling edits are described,
  not made; the only divine-sky file edited is the runbook + this plan ✓

## Risks / gotchas

- **Silent KV no-op.** A missing/expired name-server `FASTLY_API_TOKEN` does not error — it stops
  KV propagation quietly. The deploy order is meaningless without the Step F3.3 propagation check.
- **Two homes for one token.** `FASTLY_API_TOKEN` lives as a Worker secret (name-server) and a
  GitHub Actions secret (router). Rotation must hit both or the edge desyncs (writes stop, or
  publishes 401).
- **Chunk C entanglement.** The router's KV binding is broken on `origin/main` (name/`store_id`
  mismatch). F's CI is designed to *expose* that on the first publish, not mask it. Do not "fix" it
  inside the F workflow; let Chunk C own it.
- **Fastly CLI install drift.** The `latest` Fastly CLI release URL in F1.1 is convenient but
  unpinned; if CI reproducibility matters, pin a CLI version. Mirrors the master-plan warning about
  floating tags — call it out, don't silently float.
- **Wrong-ref auditing.** Local checkouts were on feature branches (`feat/admin-restore-endpoint`,
  router HEAD `ab7f623`). Always re-confirm against `origin/main` (Step 0.1–0.2) before trusting
  any line numbers here.
