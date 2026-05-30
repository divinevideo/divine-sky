# ATProto Production Rollout Implementation Plan (Verified 2026-05-30)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Supersedes** `2026-04-03-phase1-atproto-production-rollout.md`. That plan was written before the protocol work landed and assumed most of it was still TODO. A cross-repo audit on 2026-05-30 (rsky, keycast, divine-iac-coreconfig, divine-web, divine-mobile, divine-name-server, divine-router, divine-sky) found the opposite: **the protocol surface is largely built and deployed.** This plan reflects verified ground truth.

**Goal:** Take the already-built ATProto login + crosspost system from "works in pieces" to a correct, pinned, end-to-end-verified production launch where a third-party Bluesky client can sign in with `username.divine.video` and a creator's Nostr videos mirror to Bluesky.

**Architecture (unchanged, now real):** `username.divine.video` (handle) → `pds.divine.video` (rsky-pds resource server) → `entryway.divine.video` (keycast ATProto Authorization Server) → `login.divine.video` (keycast human console + lifecycle source of truth). Public edge (`divine-name-server` CF Worker + `divine-router` Fastly) is deployed outside ArgoCD.

**Tech Stack:** Rust (rsky-pds, divine-atbridge, divine-router), keycast (Rust/Axum), Svelte (divine-web), Flutter (divine-mobile), Kustomize/ArgoCD/GKE, Cloudflare Workers/D1, Fastly Compute@Edge/KV, Google Secret Manager.

---

## Reality Check: What The Audit Found (read before doing anything)

### Already DONE — verified in git, NOT live-probed — DO NOT rebuild these

> **Honesty caveat:** every check below is `git show`/source-level on the deploy branches and commits. They were **not** confirmed against the running servers this session (sandbox blocks external egress). "Built" is verified; "deployed and answering" is **inferred** from IAC config + the smoke test being treated as a live contract. Chunk 0 turns that inference into fact — run it first. The `latest` image pinning gap (blocker #3) is exactly why this distinction matters for `rsky-pds`.

| Area | Status | Evidence |
|---|---|---|
| **Keycast ATProto OAuth server** | **Built & deployed** | `keycast` `origin/main` + deployed image `bd92361`: `atproto_oauth.rs` implements PAR (consumed via `find_by_request_uri`), authorize, token, **refresh-token rotation**, DPoP (ES256), **ready-gating** (`AND atproto_state = 'ready'`), scopes `["atproto"]`, **confidential client** (`private_key_jwt` + `client_id_metadata_document_supported: true`). Routes: `/api/atproto/oauth/{par,authorize,token}`. |
| **Auth-server metadata** | Correct | `keycast:api/src/api/http/atproto_oauth_metadata.rs` advertises the fields above incl. `require_pushed_authorization_requests: true`, `authorization_response_iss_parameter_supported: true`. |
| **PDS protected-resource metadata** | Built | `rsky` `divinevideo/main`: `rsky-pds/src/well_known.rs` serves `/.well-known/oauth-protected-resource` → `{resource: pds.divine.video, authorization_servers: [entryway.divine.video]}`, env-driven via `PDS_OAUTH_AUTHORIZATION_SERVER`; test `tests/oauth_metadata_test.rs`. |
| **Hostname contract** | Resolved | IAC production uses `login.divine.video` + `entryway.divine.video` + `pds.divine.video`. No `login.dvines.org` in prod. |
| **Keycast GitOps/secrets** | Wired | IAC keycast `deployment.yaml` has all 6 ATProto env vars, `external-secret.yaml` (GCP Secret Manager), control-plane → `divine-handle-gateway.sky.svc.cluster.local:3000`, entryway on HTTPRoute. ArgoCD covers all services; `sky` namespace defined. |
| **Clients (web + mobile)** | Aligned | `divine-web/src/lib/divineLoginOrigin.ts` and `divine-mobile/mobile/lib/providers/auth_providers.dart` default to `https://login.divine.video`; Android `AndroidManifest.xml` + iOS `Runner.entitlements` deep links consistent. |
| **Crosspost scheduler (this repo)** | 7/7 + tested | `divine-atbridge`: oldest-first backfill (`backfill_planner.rs:131`), job leasing + expiry recovery (`migrations/004`), live/backfill lane isolation, delete-cancel, `crosspost_enabled && ready` gate, backfill state machine. Integration tests in `tests/publish_queue_scheduler.rs`. |

> ⚠️ A subagent audit that read keycast's **stale `feat-divine-entryway-dpop-token` worktree (last touched 2026-03-29)** reported the OAuth server as incomplete (empty scopes, broken ready-gating, PAR-not-consumed, no refresh). **That report is wrong** — it read a tree a month older than what ships. The deployed commit `bd92361` (2026-05-07) and current `origin/main` (2026-05-22) have all of it. Verify against the deployed commit, never a local worktree.

### The REAL remaining work (what this plan covers)

1. **Path-contract mismatch (BLOCKING the launch smoke).** divine-sky's smoke script, specs, and plans assert the ATProto OAuth endpoints live at `/api/oauth/*`, but keycast's source serves and advertises `/api/atproto/oauth/*`. The smoke test as written would **fail against that server**. (The `atproto-auth-server-smoke-test.md` runbook was corrected in the pending merge; the script + specs are not.) Note: divine-sky's `origin/main` PRs #8/#9 deliberately normalized *toward* `/api/oauth/*`; whether that was a doc error or signals an intended future keycast route move is **not determinable from divine-sky alone** — see Chunk A.
2. **rsky-pds entryway token-trust verification.** `auth_verifier.rs` on `divinevideo/main` validates token audience against `PDS_ENTRYWAY_DID` and calls `verify_jwt` (signature check). The open question is narrow but important: **is entryway's signing key actually in the PDS trust set, end to end?** Needs a real cross-service test, not a code read.
3. **Image pinning.** Only keycast is pinned in prod (`bd92361`). `rsky-pds`, `divine-atbridge`, `divine-handle-gateway`, `divine-feedgen`, `divine-labeler` are on floating `latest` — so "what's deployed" for the PDS (which carries the protected-resource endpoint and token trust) is **unknown**.
4. **Router KV blocker.** `divine-router` code opens KV store `"divine-names"` (`src/main.rs:15`) but `fastly.toml` defines `"usernames"` with no `store_id`. Runtime mismatch.
5. **Public-edge deploy hygiene.** No CI for `divine-router`; `divine-name-server` `FASTLY_API_TOKEN` is a manual secret.
6. **Rollout gate decision.** `enableAtprotoPublishing` (web) and `FeatureFlag.atprotoPublishing` (mobile) **do not exist** — confirmed phantom. The crosspost path has a real server-side gate (`crosspost_enabled && ready`); client publishing flags do not. Decide: remove the phantom language or build real client gates.
7. **Scheduler polish + launch safety.** Cursor/enqueue transaction, lease-expiry alerting, rate limits, moderation/DMCA intake (re: the 2026-03 PDS spam incident).

---

## Swarm Decomposition & Corrections (2026-05-30, verified)

A 26-agent swarm decomposed every chunk into a detailed sub-plan under `docs/superpowers/plans/2026-05-30-rollout-chunks/` (one file per chunk) and surfaced four corrections to this master plan — three independently re-verified:

1. **CRITICAL — the PDS protocol chain is dead in prod regardless of image.** `PDS_OAUTH_AUTHORIZATION_SERVER` and `PDS_ENTRYWAY_DID` are **not set anywhere** in `divine-iac-coreconfig/k8s/applications/rsky-pds/` (independently confirmed). The `well_known.rs` handler returns **404** when `oauth_authorization_server` is `None`, so `/.well-known/oauth-protected-resource` 404s by default, and the entryway token-trust branch in `auth_verifier.rs` is **inert** (audience compares against an unset `PDS_ENTRYWAY_DID`). Consequences:
   - **Chunk B is no longer just image-pinning** — it must *also* wire both env vars (see `chunk-B-image-pinning.md`, retitled). B now precedes A.
   - **Chunk D (token trust) is blocked** until `PDS_ENTRYWAY_DID` exists; its wrong-key-rejected test can't even be meaningful until then.
   - **Chunk 0's probe alone cannot distinguish "image too old" from "env unset"** — its Step 4 must read the IAC to disambiguate.
2. **E correction:** mobile is *not* fully flag-less — a `blueskyPublishing` flag exists (off by default). `enableAtprotoPublishing`/`FeatureFlag.atprotoPublishing` are the phantom names. The only real publishing control remains server-side `crosspost_enabled && ready`. Don't delete the (correctly-named-but-different) `blueskyPublishing` line.
3. **H correction:** neither keycast nor rsky-pds has application-level rate limiting (rsky has a literal `@TODO` at `create_session.rs:112`). All limits must be enforced at the **NGINX Gateway** edge — and a duplicate `limit_req_zone` name across SnippetsFilters will fail the reload and **take down every route on the shared gateway**, so render-time uniqueness checks are mandatory.
4. **A correction:** the smoke script had **no** `token_endpoint` assertion (only `authorization_endpoint` + `par`), so this plan's earlier Task A1 referenced a phantom assertion. **A1 is now done** (script fixed to `/api/atproto/oauth/*`). **A2 is a verified no-op:** the remaining `/api/oauth/*` references in the specs are legitimate (they describe keycast's *generic* Nostr/UCAN OAuth, including the rejected "Option 1" in superseded design docs) — sweeping them would corrupt accurate history.

**Revised order given #1:** **0 → B → D → A1 → C → F → G+H → E → I.** (B jumps ahead because the protocol chain is broken until the env vars are wired.)

---

## Recommended Order

**0 (live probe) → A (unblock smoke) → D (verify token trust) → B (pin images) → C (router) → F (edge CI) → G+H (polish + safety) → E (rollout decision) → I (e2e + cohort ramp).**

Rationale: Chunk 0 confirms the running servers match the source audit — if the PDS protected-resource is 404 in prod, **B jumps ahead of A**. Then the smoke test's path contract must be correct (A); token trust (D) is the one unverified protocol claim and gates everything — do it before pinning. Then make the deploy correct and immutable (B, C, F). Then polish/safety (G, H). The rollout-gate decision (E) can run in parallel. Launch (I) last.

---

## Chunk 0: Pre-Flight Live Probe (run FIRST — it can reorder everything)

The whole "Reality Check" rests on the running servers matching the source. Confirm it from a machine with real egress (NOT the planning sandbox, which blocks it).

### Task 0: Probe the live chain and branch on the result

- [ ] **Step 1: Probe entryway + PDS metadata**

```bash
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server | jq '{authorization_endpoint, scopes_supported, token_endpoint_auth_methods_supported}'
curl -isS https://pds.divine.video/.well-known/oauth-protected-resource | head -20
curl -fsS https://pds.divine.video/xrpc/com.atproto.server.describeServer | jq '.did'
```

- [ ] **Step 2: Branch**
  - **Entryway returns `/api/atproto/oauth/*`** → confirms the keycast-deployed claim AND independently confirms Chunk A's path direction against the *running* server (stronger than reading source). Proceed A → D → B.
  - **PDS protected-resource returns the JSON** → "deployed" holds for rsky too.
  - **PDS protected-resource returns 404** → the `latest` rsky-pds image predates the protected-resource endpoint; the protocol chain is **broken in prod right now**. **Chunk B (pin + redeploy rsky-pds) becomes the blocking first step — do it before A.**
  - **Entryway returns `/api/oauth/*`** (not `/api/atproto/oauth/*`) → the running server disagrees with current source; stop and reconcile which keycast commit is actually deployed before touching divine-sky docs.

- [ ] **Step 3: Record the live results** in this plan or the launch checklist so the rest of the team isn't re-inferring.

---

## Chunk A: Reconcile the ATProto OAuth Path Contract

The launch smoke test must hit the endpoints keycast actually advertises. Source (and, pending Chunk 0, the live server) says `/api/atproto/oauth/*`; divine-sky currently asserts `/api/oauth/*`. The immediate fix — make the test match the running server — is correct **regardless of intent**: a test should assert reality.

**Before the sweep in Task A2, settle direction.** divine-sky `origin/main` PRs #8/#9 normalized *toward* `/api/oauth/*`; that was either a doc error or a signal of an intended keycast route move. Confirm with the keycast owners which path is canonical. If keycast intends to move ATProto OAuth to `/api/oauth/*`, the fix belongs on the **keycast** side (and divine-sky should not be swept). Until confirmed, treat the contract as "currently mismatched," not "divine-sky is wrong."

### Task A1: Fix the smoke script's endpoint assertions

**Files:**
- Modify: `scripts/smoke-divine-atproto-login.sh` (lines ~230–231)

- [ ] **Step 1: Confirm the live contract (source of truth is the server)**

```bash
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server | jq '{authorization_endpoint, token_endpoint, pushed_authorization_request_endpoint, scopes_supported, token_endpoint_auth_methods_supported}'
```
Expected: all three endpoints under `/api/atproto/oauth/`, `scopes_supported: ["atproto"]`, auth methods include `none` and `private_key_jwt`.

- [ ] **Step 2: Fix the assertions to match**

In `scripts/smoke-divine-atproto-login.sh`, change the asserted endpoint values:
```
authorization_endpoint            → https://entryway.divine.video/api/atproto/oauth/authorize
token_endpoint                    → https://entryway.divine.video/api/atproto/oauth/token
pushed_authorization_request_endpoint → https://entryway.divine.video/api/atproto/oauth/par
```

- [ ] **Step 3: Run the script against staging/prod and confirm the metadata step passes**

```bash
bash scripts/smoke-divine-atproto-login.sh
```
Expected: the authorization-server metadata assertions pass (previously would have failed on the path mismatch).

- [ ] **Step 4: Commit**

```bash
git add scripts/smoke-divine-atproto-login.sh
git commit -m "fix(smoke): assert real /api/atproto/oauth endpoints"
```

### Task A2: Sweep specs/plans for the wrong path convention

**Files:**
- Modify: any file under `docs/superpowers/specs/` and `docs/superpowers/plans/` referencing `/api/oauth/` as the **ATProto** OAuth path (NOT keycast's generic Nostr/UCAN `/api/oauth/*`, which is legitimate).

- [ ] **Step 1: Find candidates**

```bash
grep -rn "api/oauth/" docs/ | grep -iE 'atproto|entryway|authorization_endpoint|pushed_auth'
```

- [ ] **Step 2: Update each ATProto reference to `/api/atproto/oauth/*`**, leaving generic-OAuth (Nostr/UCAN) references alone. Add a one-line note in the boundary design spec that keycast namespaces ATProto OAuth under `/api/atproto/oauth/*`, distinct from `/api/oauth/*`.

- [ ] **Step 3: Verify no stray ATProto `/api/oauth/` references remain**

```bash
grep -rn "api/oauth/" docs/ scripts/ | grep -iE 'atproto|entryway' || echo "clean"
```
Expected: `clean`.

- [ ] **Step 4: Commit**

```bash
git add docs/
git commit -m "docs: align atproto oauth path contract with keycast"
```

---

## Chunk D: Verify rsky-pds Entryway Token Trust (the one real protocol unknown)

`auth_verifier.rs` (`divinevideo/main`) checks audience against `PDS_ENTRYWAY_DID` and verifies JWT signatures, but no test proves an **entryway-issued** token is accepted by the **PDS** and a forged one is rejected. This is the highest-risk unverified link.

### Task D1: Prove end-to-end token trust with a test

**Files:**
- Read: `../rsky/rsky-pds/src/auth_verifier.rs` (entryway path, ~lines 545–560 and the `verify_jwt` key selection)
- Create/Modify: `../rsky/rsky-pds/tests/entryway_token_trust.rs`
- Verify: IAC `rsky-pds` env (`PDS_ENTRYWAY_DID`, entryway signing public key config)

- [ ] **Step 1: Write a failing test — entryway-signed token is accepted**

Mint a token with entryway's configured signing key and `aud = PDS_ENTRYWAY_DID`; assert the PDS auth verifier accepts it and resolves the correct repo DID.

- [ ] **Step 2: Write a failing test — wrong-key token is rejected**

Mint a token with a different key but the same `aud`; assert the verifier rejects it (signature failure), proving trust is keyed on signature, not audience alone.

- [ ] **Step 3: Run**

```bash
cd ../rsky && cargo test -p rsky-pds entryway_token_trust
```
Expected: both tests pass. If the wrong-key token is accepted, that is a **security finding** — stop and fix key trust before any further rollout.

- [ ] **Step 4: Confirm the deployed PDS env actually carries entryway's public key**

Verify IAC `rsky-pds` production overlay/secret sets the entryway signing public key and `PDS_ENTRYWAY_DID`. Without it, the live PDS cannot verify entryway tokens.

- [ ] **Step 5: Commit (in rsky)**

```bash
git -C ../rsky add rsky-pds/tests/entryway_token_trust.rs
git -C ../rsky commit -m "test: prove entryway token trust on the pds"
```

---

## Chunk B: Pin Production Images

Floating `latest` means the PDS protocol fixes and keycast OAuth may or may not be what's running. Pin everything.

### Task B1: Pin the five unpinned production services

**Files:**
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-atbridge/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-handle-gateway/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-feedgen/overlays/production/kustomization.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/divine-labeler/overlays/production/kustomization.yaml`
- Modify: `docs/runbooks/launch-checklist.md`

- [ ] **Step 1: Identify the exact commit each prod service should ship**

For `rsky-pds`, the chosen tag MUST be a build of `divinevideo/main` that contains `/.well-known/oauth-protected-resource` AND the entryway token-trust test from Chunk D. Confirm:
```bash
git -C ../rsky log --oneline divinevideo/main -- rsky-pds/src/well_known.rs | head
```

- [ ] **Step 2: Replace `newTag: latest` with the immutable commit/sha tag in each of the five overlays.**

- [ ] **Step 3: Render and assert no `latest` remains**

```bash
cd ../divine-iac-coreconfig
for s in rsky-pds divine-atbridge divine-handle-gateway divine-feedgen divine-labeler; do
  kustomize build k8s/applications/$s/overlays/production | grep -E 'image:.*:latest' && echo "FAIL $s still latest" || echo "ok $s"
done
```
Expected: `ok` for all five.

- [ ] **Step 4: Make `latest` in a prod overlay a hard stop in the checklist** (`docs/runbooks/launch-checklist.md`).

- [ ] **Step 5: Commit (in IAC)**

```bash
git -C ../divine-iac-coreconfig add k8s/applications/*/overlays/production/kustomization.yaml
git -C ../divine-iac-coreconfig commit -m "chore: pin atproto production images to immutable tags"
git add docs/runbooks/launch-checklist.md && git commit -m "docs: treat latest prod image tag as hard stop"
```

---

## Chunk C: Fix the Router KV Blocker

`divine-router` will fail to read handle state until the KV store name + id match.

### Task C1: Reconcile the Fastly KV store binding

**Files:**
- Modify: `../divine-router/fastly.toml` (the `[setup.kv_stores.*]` block — name `usernames`, missing `store_id`)
- Read: `../divine-router/src/main.rs:15` (`const KV_STORE_NAME: &str = "divine-names"`)

- [ ] **Step 1: Decide the canonical store name** and make code + config agree (prefer the name the production Fastly KV store actually has). If the live store is `divine-names`, rename the config; if `usernames`, change the constant.

- [ ] **Step 2: Set the real `store_id`** for the production Fastly KV store in `fastly.toml`.

- [ ] **Step 3: Publish and verify against a known-ready user**

```bash
cd ../divine-router && fastly compute publish --non-interactive && fastly purge --all
curl -fsS https://<ready-username>.divine.video/.well-known/atproto-did   # expect the DID
curl -s -o /dev/null -w '%{http_code}' https://<not-ready>.divine.video/.well-known/atproto-did  # expect 404
```

- [ ] **Step 4: Commit (in divine-router)**

```bash
git -C ../divine-router add fastly.toml src/main.rs
git -C ../divine-router commit -m "fix: reconcile fastly kv store name and id"
```

---

## Chunk F: Public-Edge Deploy Hygiene

The edge tier is the only path outside ArgoCD; make it reproducible.

### Task F1: Add router CI and document edge secrets

**Files:**
- Create: `../divine-router/.github/workflows/deploy.yml`
- Modify: `../divine-name-server/README.md`, `../divine-router/README.md`
- Modify: `docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Add a `fastly compute publish` GitHub Actions workflow** for `divine-router` (Fastly API token + service id from `fastly.toml`), gated on main.
- [ ] **Step 2: Document the required secrets**: name-server `FASTLY_API_TOKEN`, `FASTLY_STORE_ID`, Cloudflare D1 binding; router Fastly service + KV access.
- [ ] **Step 3: Document the deploy ORDER** (name-server writes state first, then router publishes) in `login-divine-video.md`.
- [ ] **Step 4: Commit** (in each repo + this one).

---

## Chunk G: Scheduler Production Polish

The scheduler is staging-ready; these close correctness/ops gaps before widening.

### Task G1: Transactionalize relay-cursor advancement with enqueue

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs` (~lines 246–286, `enqueue_live_event`)
- Test: `crates/divine-atbridge/tests/runtime_resilience.rs`

- [ ] **Step 1: Failing test** — simulate a crash between `enqueue_publish_job` and `persist_relay_cursor`; assert no event is silently skipped on replay.
- [ ] **Step 2:** Wrap enqueue + cursor persistence in one DB transaction (idempotency masks the gap today, but make it correct).
- [ ] **Step 3:** `cargo test -p divine-atbridge runtime_resilience` passes.
- [ ] **Step 4:** Commit.

### Task G2: Lease-expiry watchdog + alerting

**Files:**
- Modify: `crates/divine-atbridge/src/health.rs` (or the metrics surface)

- [ ] **Step 1:** Emit a metric/alert when `SELECT COUNT(*) FROM publish_jobs WHERE state='in_progress' AND lease_expires_at < NOW()` > 0, and on `publish_backfill_state='failed'`.
- [ ] **Step 2:** Wire the alert into the existing alerting stack (checklist "Safety").
- [ ] **Step 3:** Commit.

---

## Chunk H: Launch Safety / Abuse Controls

The PDS already suffered a repost-bot spam incident (2026-03) that forced invite-only lockdown. A public OAuth login + crosspost path reopens that surface.

### Task H1: Rate limits + moderation intake

- [ ] **Step 1:** Set/verify rate limits on entryway PAR + token, and PDS XRPC writes; document the limits.
- [ ] **Step 2:** Confirm DMCA/takedown intake routes into the moderation queue before any public creator onboarding.
- [ ] **Step 3:** Confirm the disable flow clears public `atproto_did` resolution (router 404) and blocks new mirrored posts. Cross-check with Chunk C's 404 assertion.
- [ ] **Step 4:** Commit doc updates.

---

## Chunk E: Resolve Rollout Gating (parallelizable)

### Task E1: Kill phantom flags or build real ones

**Files:**
- Modify: `docs/runbooks/launch-checklist.md`, `docs/runbooks/atproto-opt-in-smoke-test.md`

- [ ] **Step 1:** Decide:
  - **Option A:** remove `enableAtprotoPublishing` / `FeatureFlag.atprotoPublishing` language from runbooks; rely on the real server-side gate (`crosspost_enabled && ready`) + cohorted opt-in.
  - **Option B:** implement real client gates in `divine-web` (`API_CONFIG.features`) and `divine-mobile` (`FeatureFlag` enum) and wire them.
- [ ] **Step 2:** Recommended: **Option A** — the gate that matters (publishing) is enforced in `divine-atbridge`; phantom client flags add nothing but confusion. Update docs to describe the real gate.
- [ ] **Step 3:** Commit.

---

## Chunk I: End-to-End Launch Smoke + Cohort Ramp

### Task I1: Run the real chain, then ramp

- [ ] **Step 1: Protocol smoke (corrected paths)**

```bash
bash scripts/smoke-divine-atproto-login.sh
curl -fsS https://pds.divine.video/.well-known/oauth-protected-resource | jq
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server | jq
```

- [ ] **Step 2: Real client login** — use an actual Bluesky-compatible client (or the runbook's PAR→authorize→token→refresh walk in `docs/runbooks/atproto-auth-server-smoke-test.md`) to confirm a `ready` account authenticates and the token works against `pds.divine.video`. This exercises confidential-client + refresh, which the runbook (restored) documents.

- [ ] **Step 3: Lifecycle walk** — claim username → enable ATProto → `pending→ready` → `username.divine.video/.well-known/atproto-did` resolves → disable → stops resolving.

- [ ] **Step 4: Crosspost walk** — a `ready`, `crosspost_enabled` creator posts a Nostr video; confirm it mirrors to Bluesky; confirm backlog drains oldest-first; confirm a delete cancels a queued job.

- [ ] **Step 5: Ramp** — internal cohort → small creator cohort → broader opt-in, watching the Chunk G alerts and the checklist ops queries between steps.

---

## Self-Review (spec coverage)

- Path contract (blocker #1) → Chunk A ✓
- Token trust (blocker #2) → Chunk D ✓
- Image pinning (#3) → Chunk B ✓
- Router KV (#4) → Chunk C ✓
- Edge CI/secrets (#5) → Chunk F ✓
- Rollout gates (#6) → Chunk E ✓
- Scheduler polish + safety (#7) → Chunks G, H ✓
- End-to-end verification → Chunk I ✓
- "Do not rebuild" guardrails for already-done work → Reality Check table ✓

## Risks

- **Reading stale trees.** The biggest documented risk this rollout already hit: auditing worktrees/local-HEAD instead of the deployed commit. Always verify against the image tag that ships (Chunk B makes that tag knowable).
- **Token-trust false-confidence.** If Chunk D's wrong-key test is skipped, a misconfigured trust set could accept forged tokens. Treat D as a gate, not a nicety.
- **Edge drift.** name-server/router are outside ArgoCD; without Chunk F they drift silently. The disable→404 path (H/C) is the user-facing safety contract.
- **Abuse.** Public login + crosspost reopens the spam surface; H must land before broad opt-in.
