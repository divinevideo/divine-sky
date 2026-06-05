# rsky Fork ↔ Upstream Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.
>
> **This plan operates on the sibling repo `/Users/rabble/code/divine/rsky`, NOT divine-sky.** It is a standalone effort — NOT on the critical path to a first Bluesky crosspost (that is bridge-side: divine-sky Walls 1–4). Sequence it after, or in parallel by a different operator.

**Goal:** Bring the `divinevideo/rsky` fork up to date with `blacksky-algorithms/rsky` upstream (56 commits ahead) while preserving all 21 Divine-specific fork commits, and ship the result to the staging PDS.

**Architecture:** The deployed staging PDS image (`87151fc`) is 4 commits behind even the fork tip (`divinevideo/main` @ `7e5e78a`), and the fork is 56 behind upstream. Upstream's changes to the files the fork also touches are mostly **refactors** (env-var renames `JWT_KEY`→`PDS_JWT_KEYPAIR`, key-derivation moved to startup, single-call env consolidation, clippy/rustfmt) — not auth-semantic changes — but the renames mean **coordinated secret/IaC updates** are required, which is why this is its own project. Use a **merge** (not rebase) to integrate: one conflict-resolution pass, preserves fork history, far easier to review than replaying 21 commits.

**Tech Stack:** Rust workspace (rsky-pds + crates), Diesel/Postgres, Docker, GKE/ArgoCD, GCP Secret Manager, `cargo test`.

---

## Divergence Map (measured 2026-06-05)

- `divinevideo/main` is **21 commits ahead** of `upstream/main` (fork-unique — MUST preserve) and **56 behind** (to integrate).
- Deployed image `87151fc` is **4 behind `divinevideo/main`** — so shipping the fork tip is itself an improvement (includes `413fa35` protected-resource + `2e8e426` entryway-as-auth-server, the pieces the divine-sky audit flagged missing in prod).
- **14 colliding files** (touched by both fork-unique and upstream-56):
  `Cargo.lock`, `rsky-pds/Dockerfile`, `account_manager/helpers/auth.rs`, `actor_store/mod.rs`, `apis/com/atproto/server/create_account.rs`, `apis/com/atproto/server/mod.rs`, `apis/com/atproto/sync/subscribe_repos.rs`, `auth_verifier.rs`, `config/mod.rs`, `lib.rs`, `repo/prepare.rs`, `sequencer/mod.rs`, `sequencer/outbox.rs`, `tests/common/mod.rs`.

### Fork-unique commits by risk (resolve carefully where RISKY)
**RISKY (auth/security/account/blob/federation — verify each conflict against intent):**
`52053ab` ATProto federation + video service auth + PDS security hardening · `2e8e426` entryway as pds auth server · `413fa35` PDS OAuth protected-resource metadata · `877aa8d` validate bsky video embed blobs · `776ccf2` embed lexicons at compile time · `1d4d9ee` associate/promote blobs on putRecord · `8be48e7` skip object ACLs for gcs blobstore · `c9b6ca2` align env contracts for account flows · `41fd70e` require direct service-domain handles · `54d2878`/`a8b7ae1` DID-resolution TLS (native-tls/rustls-native-roots) · `59c8df2` S3 bucket naming + DID URL encoding · `354a010` split health/liveness + isolate sequencer thread

**MECHANICAL (build/docker — resolve by taking fork's intent):**
`7e5e78a` guardrails · `0889301` bookworm runtime · `8f7bdb9` libpq · `10a1c4c` docker release flags · `d98caba` trim debuginfo · `cea552d` build parallelism · `fcf29cd` build from fork workspace

---

## Chunk A: Prepare and integrate (isolated, non-destructive)

### Task A1: Create the integration branch and fetch
**Repo:** `/Users/rabble/code/divine/rsky`

- [ ] **Step 1:** Confirm clean tree + fresh remotes.
```bash
cd /Users/rabble/code/divine/rsky
git status --short      # expect empty
git fetch --all
git log --oneline -1 divinevideo/main   # 7e5e78a (or newer — re-baseline if moved)
git log --oneline -1 upstream/main
```
- [ ] **Step 2:** Branch from the fork tip (never work on `divinevideo/main` directly).
```bash
git checkout -B sync/upstream-$(date +%Y%m%d) divinevideo/main
```
- [ ] **Step 3:** Record the pre-sync baseline (for diffing behavior later).
```bash
git rev-parse HEAD > /tmp/rsky-presync-sha.txt
```

### Task A2: Merge upstream (single resolution pass)
- [ ] **Step 1:** Start the merge.
```bash
git merge upstream/main --no-commit --no-ff
```
- [ ] **Step 2:** List conflicts; triage mechanical vs risky.
```bash
git diff --name-only --diff-filter=U
```
- [ ] **Step 3 (mechanical files):** For `Cargo.lock` regenerate rather than hand-merge:
```bash
git checkout --theirs Cargo.lock 2>/dev/null || true
cargo update -w   # then `cargo build` reconciles; re-add
```
For `Dockerfile` and build files, keep the **fork's** runtime choices (bookworm, libpq, release flags, build-from-fork-workspace) while accepting any upstream base-image/security bumps — read both sides, take the union of intent.
- [ ] **Step 4 (RISKY files — one at a time):** For each of `auth_verifier.rs`, `create_account.rs`, `config/mod.rs`, `account_manager/helpers/auth.rs`, `repo/prepare.rs`, `actor_store/mod.rs`, `sequencer/*`, `subscribe_repos.rs`, `lib.rs`: open the conflict, and for **each hunk** decide using this rule — *upstream owns refactors (renames, env-var consolidation, key-on-startup, fn sync/async), the fork owns behavior (entryway trust, protected-resource, video/blob handling, ACL skip, S3 naming, service-domain handle rule).* Where upstream renamed something the fork uses (e.g. `JWT_KEY`→`PDS_JWT_KEYPAIR`), keep the fork's behavior but adopt the new name/location. Note every env-var rename in `/tmp/rsky-env-renames.txt` for Chunk C.
- [ ] **Step 5:** Stage resolved files as you finish each; do NOT commit until it builds (next task).

### Task A3: Make it compile + pass tests
- [ ] **Step 1:** Build (libpq env as in divine-sky AGENTS.md).
```bash
LIBPQ=$(brew --prefix libpq); export LIBRARY_PATH="$LIBPQ/lib" PKG_CONFIG_PATH="$LIBPQ/lib/pkgconfig" CPATH="$LIBPQ/include"
cargo build -p rsky-pds 2>&1 | tail -20
```
Expected: clean build. Fix any rename fallout (the most likely failures are env-var/key-handle renames from upstream — apply the new names while keeping fork logic).
- [ ] **Step 2:** Run the rsky-pds test suite.
```bash
cargo test -p rsky-pds 2>&1 | tail -30
```
Expected: green. If upstream added tests that assume upstream behavior the fork intentionally changed (e.g. handle rules, ACLs), reconcile per fork intent and note it.
- [ ] **Step 3:** Clippy + fmt.
```bash
cargo clippy -p rsky-pds --all-targets -- -D warnings 2>&1 | tail; cargo fmt --check
```
- [ ] **Step 4:** Commit the merge.
```bash
git commit   # keep the generated merge message + a summary of risky-file resolutions
```

### Task A4: Verify fork behavior survived (regression guard)
- [ ] **Step 1:** Confirm the Divine-critical endpoints/behaviors are still present in the merged tree (these are the reasons the fork exists):
```bash
grep -rn 'oauth-protected-resource' rsky-pds/src/well_known.rs          # 413fa35 survived
grep -rn 'PDS_ENTRYWAY_DID\|entryway' rsky-pds/src/auth_verifier.rs     # 2e8e426 survived
grep -rn 'verify_jwt.*issuer\|iss.*subject' rsky-pds/src/              # 87151fc video-auth survived
grep -rniE 'skip.*acl|gcs' rsky-pds/src/actor_store/                    # 8be48e7 survived
```
Expected: each still present. Anything missing = a conflict was resolved the wrong way → revisit A2.
- [ ] **Step 2:** Diff env-var contract vs the deployed secret keys (catch renames before deploy):
```bash
grep -rnoE 'env::var\("[A-Z_]+"\)' rsky-pds/src | grep -oE '"[A-Z_]+"' | sort -u > /tmp/rsky-env-after.txt
# compare against the rsky-pds ExternalSecret keys in divine-iac-coreconfig
```

---

## Chunk B: Build + deploy to staging (prove before prod)

### Task B1: Build and push the staging image
- [ ] **Step 1:** Tag = git short-SHA of the merge commit.
```bash
SHA=$(git rev-parse --short HEAD)
docker build -f rsky-pds/Dockerfile -t us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds:${SHA} .
docker push us-central1-docker.pkg.dev/dv-platform-staging/containers-staging/rsky-pds:${SHA}
```
(Match however `video-auth-87151fc` was built if the Dockerfile context differs.)

### Task B2: Apply any env-var renames in IaC FIRST, then bump the image
**Repo:** `/Users/rabble/code/divine/divine-iac-coreconfig`
- [ ] **Step 1:** For every rename in `/tmp/rsky-env-renames.txt`, add the new-named secret in `dv-platform-staging` Secret Manager (copy the value from the old key) and update `k8s/applications/rsky-pds/overlays/staging/kustomization.yaml` env/ExternalSecret keys. **Do this before the image rollout** so the new binary finds its config.
- [ ] **Step 2:** Bump `rsky-pds` staging overlay `newTag` to `${SHA}`; commit/push/merge → ArgoCD sync.
- [ ] **Step 3:** Verify rollout + health.
```bash
SC=connectgateway_dv-platform-staging_us-central1_gke-staging-membership
kubectl --context $SC rollout status deploy/rsky-pds -n sky
curl -fsS https://pds.staging.dvines.org/xrpc/_health
curl -fsS https://pds.staging.dvines.org/.well-known/oauth-protected-resource | jq   # 413fa35 live
```

### Task B3: Regression-test the live PDS against known-good behavior
- [ ] **Step 1:** The existing `*.staging.dvines.org` repos must still resolve and serve:
```bash
curl -fsS "https://pds.staging.dvines.org/xrpc/com.atproto.sync.listRepos?limit=5" | jq '.repos[].did'
curl -fsS "https://pds.staging.dvines.org/xrpc/com.atproto.repo.describeRepo?repo=<an-existing-did>" | jq '.handle'
```
- [ ] **Step 2:** Re-run `scripts/smoke-divine-atproto-login.sh` (in divine-sky) and the video path if available.
- [ ] **Step 3:** This is the gate to consider the sync good on staging. Do NOT promote to prod until green.

---

## Chunk C: Production (separate, gated on staging soak)

### Task C1: Promote
- [ ] **Step 1:** Create the renamed secrets in `dv-platform-prod` Secret Manager (same renames as B2).
- [ ] **Step 2:** Build/push the prod image to `containers-production`; pin the prod `rsky-pds` overlay `newTag` off `latest` to `${SHA}`.
- [ ] **Step 3:** ArgoCD sync; verify health + protected-resource + an existing repo resolves.
- [ ] **Step 4:** Soak before widening (watch logs for auth/sequencer regressions from the 56-commit jump).

---

## Self-Review
- Spec coverage: integrate-56 (A2), preserve-21 (A4 regression checks), env-rename handling (A2 step 4 → B2/C1), build/test gates (A3), staging-before-prod (B before C). ✓
- The plan uses **merge not rebase** despite the request wording — flagged in Architecture with rationale (one resolution pass, reviewable, preserves history). If a linear history is required, the fallback is `git rebase upstream/main` replaying 21 commits (first conflict already scouted: `fcf29cd` on the Dockerfile) — same per-hunk rule in A2 step 4 applies, just 21× more conflict points.

## Risks
- **Env-var/key renames are the silent killer.** Upstream moved `JWT_KEY`→`PDS_JWT_KEYPAIR` and consolidated key env vars; if IaC isn't updated in lockstep (B2/C1) the new binary boots but fails auth at runtime. A4 step 2 + B2 step 1 exist to catch this before rollout.
- **Wrong-side conflict resolution in auth code** could silently weaken security (e.g. dropping the entryway trust or the `did != credentials.did` checks). A4 step 1 greps for the fork's security behaviors as a guard; pair-review the `auth_verifier.rs`/`create_account.rs` resolutions.
- **56-commit behavioral drift** beyond refactors (sequencer/outbox, subscribe_repos) could change firehose/federation behavior the relay depends on — B3 + soak.
- **Not on the crosspost critical path** — don't let this block the bridge-side Wall 4 work.
