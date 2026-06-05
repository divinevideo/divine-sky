# Rollout Chunk H — Launch Safety / Abuse Controls (sub-plan)

> **Parent plan:** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk H, lines 306–315). Read it first.
> **Primary repo:** `ops` (this doc lives in `divine-sky`; the change surface is cross-repo).
> **Editability:** `cross-repo-spec-only`. The concrete config/code changes land in **sibling repos** (`rsky`, `keycast`, `divine-iac-coreconfig`, `divine-moderation-service`, `divine-router`). This sub-plan is a **spec** for those changes. **Do NOT edit the sibling repos while executing this doc** — the only file you may write in divine-sky is `docs/runbooks/launch-checklist.md` (Task H4). Sibling edits happen in their own PRs, owned by their repo owners, gated on this spec.
> **Why this chunk exists:** the staging PDS already suffered a repost-bot spam incident on 2026-03-26 (`PDS_INVITE_REQUIRED=false` → 23 hex-handle bot accounts, ~17,400 repost spam). A **public** OAuth login + crosspost path reopens that exact attack surface. This chunk must land **before broad opt-in** (parent "Recommended Order": G+H before E/I).

## Ground truth established by the 2026-05-30 audit (do not re-derive — these were verified this session)

| Claim | Evidence (verified) |
|---|---|
| **rsky-pds has NO application-level rate limiting.** | `rsky` `divinevideo/main`: only reference is the literal comment `// @TODO: Add rate limiting` at `rsky-pds/src/apis/com/atproto/server/create_session.rs:112`. No `governor`/`tower_governor`/`RateLimiter` anywhere in `rsky-pds/Cargo.toml` or `src/`. |
| **keycast entryway has NO application-level rate limiting on the ATProto OAuth endpoints.** | `keycast` `origin/main` (deploy lineage of `bd92361`, current tip `c235095`): `api/src/api/http/atproto_oauth.rs` has no `rate`/`throttle`/`limit` middleware; no `SnippetsFilter`/tower rate layer in the tree. |
| **Therefore both PAR/token and PDS XRPC write rate limits must be enforced at the EDGE tier.** | The edge in front of both `entryway.divine.video` (keycast) and `pds.divine.video` (rsky-pds) is **NGINX Gateway Fabric** — `gatewayClassName: nginx`, `Gateway/main-gateway` in `divine-iac-coreconfig/k8s/nginx-gateway/base/gateway.yaml`. Both apps attach via `parentRefs: [name: main-gateway, sectionName: https]` HTTPRoutes (`k8s/applications/{keycast,rsky-pds}/base/httproute.yaml`). |
| **A proven rate-limit mechanism already exists in this cluster.** | `SnippetsFilter` (CRD `gateway.nginx.org/v1alpha1`) is already used in-repo: `k8s/applications/divine-inquisitor/base/snippets-filter.yaml` injects an NGINX directive at `context: http.server.location`, wired into the HTTPRoute via `filters: [type: ExtensionRef, extensionRef: {group: gateway.nginx.org, kind: SnippetsFilter, name: ...}]`. NGINX `limit_req`/`limit_req_zone` is the directive family we add. |
| **A real moderation pipeline already exists for takedowns.** | `divine-moderation-service` (Cloudflare Worker): `user_reports` D1 table + escalation in `src/reports.mjs`; ATProto label ingest + system-label actions in `src/atproto/inbound-publisher.mjs` (`!takedown`→`delete`, `!suspend`→`ban`); `src/atproto/label-webhook.mjs`; `src/moderation/label-writer.mjs`. The IAC ships `divine-labeler` (`k8s/applications/divine-labeler/`) and rsky ships `rsky-labeler`. **The gap is a documented DMCA/takedown intake path that lands a record into this queue, not the queue itself.** |
| **The disable→404 contract is real and split across three services.** | `divine-sky` `crates/divine-handle-gateway/src/routes/disable.rs` → `sync_disabled_state` (`crates/divine-handle-gateway/src/lib.rs:196`) calls keycast `sync_disabled` + name-server `sync_state_for_handle(handle, None, "disabled")`. `divine-router/src/main.rs:336` `handle_atproto_did` returns `404` **unless** `status == "active" && atproto_state == "ready" && atproto_did.is_some()`. **This 404 contract depends on Chunk C's KV-store fix being live** (`KV_STORE_NAME = "divine-names"` at `divine-router/src/main.rs:15` must match the published store id). |

> **Editing rule restated:** when a step below says "Modify `../rsky/...`" or "Modify `../divine-iac-coreconfig/...`", that is the **spec** of what the sibling-repo PR must contain. While executing *this* doc you only **read** those files to confirm the spec still matches reality, and you only **write** to `docs/runbooks/launch-checklist.md`.

---

## Task H1: Edge rate limits on entryway PAR/token (keycast) and PDS XRPC writes (rsky-pds)

Both limits are enforced by `SnippetsFilter` resources attached to the existing HTTPRoutes — no code change to keycast or rsky-pds. We split each HTTPRoute so the auth/write paths get their own `limit_req` while read/discovery paths stay unthrottled.

**Sibling-repo files (spec — edited in a `divine-iac-coreconfig` PR, NOT here):**
- Create: `../divine-iac-coreconfig/k8s/applications/keycast/base/ratelimit-snippets-filter.yaml`
- Create: `../divine-iac-coreconfig/k8s/applications/rsky-pds/base/ratelimit-snippets-filter.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/keycast/base/httproute.yaml` (add per-path route + `ExtensionRef`)
- Modify: `../divine-iac-coreconfig/k8s/applications/rsky-pds/base/httproute.yaml`
- Modify: `../divine-iac-coreconfig/k8s/applications/{keycast,rsky-pds}/base/kustomization.yaml` (add the new resource)

- [ ] **Step 1: Confirm the spec still matches reality (read-only, run from divine-sky).**

```bash
grep -n 'TODO: Add rate limiting' ../rsky/rsky-pds/src/apis/com/atproto/server/create_session.rs
grep -rn 'parentRefs\|sectionName\|name: main-gateway' ../divine-iac-coreconfig/k8s/applications/keycast/base/httproute.yaml
cat ../divine-iac-coreconfig/k8s/applications/divine-inquisitor/base/snippets-filter.yaml
```
Expected: the `@TODO` line is still present (no app-level limiter snuck in); keycast HTTPRoute still attaches to `main-gateway` / `https`; the inquisitor SnippetsFilter still uses `apiVersion: gateway.nginx.org/v1alpha1`, `context: http.server.location`. If any of these drifted, **stop and re-spec** before the IAC PR.

- [ ] **Step 2: Define the rate-limit zones (spec content for the keycast SnippetsFilter).**

`limit_req_zone` must be declared at `http.main` context; the `limit_req` that consumes it goes at `http.server.location`. SnippetsFilter supports both contexts, so use two snippets in one resource:

```yaml
# ../divine-iac-coreconfig/k8s/applications/keycast/base/ratelimit-snippets-filter.yaml
apiVersion: gateway.nginx.org/v1alpha1
kind: SnippetsFilter
metadata:
  name: entryway-oauth-ratelimit
spec:
  snippets:
    # Zone declaration (http.main). 10MB ~= 160k unique client IPs tracked.
    - context: http
      value: |
        limit_req_zone $binary_remote_addr zone=entryway_par:10m rate=10r/m;
        limit_req_zone $binary_remote_addr zone=entryway_token:10m rate=60r/m;
        limit_req_status 429;
    # Enforcement (http.server.location) — burst absorbs legitimate retries.
    - context: http.server.location
      value: |
        limit_req zone=entryway_par burst=5 nodelay;
        limit_req zone=entryway_token burst=20 nodelay;
```

Rationale for the numbers (document them, they are defensible defaults, not magic):
- **PAR (`/api/atproto/oauth/par`)**: 10 req/min/IP, burst 5. PAR is the start-of-login step; a human starts a login a handful of times per minute at most. This is the spam-bot chokepoint.
- **token (`/api/atproto/oauth/token`)**: 60 req/min/IP, burst 20. Higher because refresh-token rotation (keycast does rotate — `atproto_oauth.rs` `next_refresh_token`) means legitimate clients hit `/token` more often than `/par`.
- `429` (not the NGINX default 503) so clients see a correct, retryable status.

- [ ] **Step 3: Attach the filter to ONLY the PAR/token paths (spec content for the keycast HTTPRoute).**

Split the keycast HTTPRoute so discovery/metadata (`/.well-known/oauth-authorization-server`) and the human console are **not** throttled, only the two abuse-prone endpoints:

```yaml
  rules:
    - matches:
        - path: { type: PathPrefix, value: /api/atproto/oauth/par }
        - path: { type: PathPrefix, value: /api/atproto/oauth/token }
      filters:
        - type: ExtensionRef
          extensionRef:
            group: gateway.nginx.org
            kind: SnippetsFilter
            name: entryway-oauth-ratelimit
      backendRefs:
        - name: keycast
          port: 3000   # confirm against existing backendRefs in httproute.yaml
    # ...existing catch-all rule for everything else stays UNCHANGED...
```

- [ ] **Step 4: Define the PDS XRPC-write SnippetsFilter (spec content).**

Throttle the write/mutation XRPC namespace, not reads. The repo-write entry points are `com.atproto.repo.{createRecord,putRecord,deleteRecord,applyWrites,uploadBlob}` and `com.atproto.server.createAccount`/`createSession`.

```yaml
# ../divine-iac-coreconfig/k8s/applications/rsky-pds/base/ratelimit-snippets-filter.yaml
apiVersion: gateway.nginx.org/v1alpha1
kind: SnippetsFilter
metadata:
  name: pds-xrpc-write-ratelimit
spec:
  snippets:
    - context: http
      value: |
        limit_req_zone $binary_remote_addr zone=pds_writes:10m rate=120r/m;
        limit_req_zone $binary_remote_addr zone=pds_account:10m rate=5r/m;
        limit_req_status 429;
    - context: http.server.location
      value: |
        limit_req zone=pds_writes burst=40 nodelay;
```

Add a **separate** route rule for account creation with the tighter `pds_account` zone (the 2026-03 incident was account-creation spam — even though prod is `PDS_INVITE_REQUIRED=true`, defense-in-depth):

```yaml
    - matches:
        - path: { type: Exact, value: /xrpc/com.atproto.server.createAccount }
      filters:
        - type: ExtensionRef
          extensionRef: { group: gateway.nginx.org, kind: SnippetsFilter, name: pds-account-create-ratelimit }
```
(Define `pds-account-create-ratelimit` with `limit_req zone=pds_account burst=2 nodelay;`.)

- [ ] **Step 5: Register the new resources in each kustomization (spec).**

Add `ratelimit-snippets-filter.yaml` to `resources:` in `k8s/applications/keycast/base/kustomization.yaml` and `k8s/applications/rsky-pds/base/kustomization.yaml`.

- [ ] **Step 6: Validate the rendered manifests (run in the IAC PR, recorded here as the gate).**

```bash
cd ../divine-iac-coreconfig
kustomize build k8s/applications/keycast/overlays/production | grep -A3 'kind: SnippetsFilter'
kustomize build k8s/applications/rsky-pds/overlays/production | grep -A3 'kind: SnippetsFilter'
```
Expected: both `SnippetsFilter` objects render; `limit_req_zone` lines appear exactly once each (duplicate zone names across SnippetsFilters in the same gateway will fail NGINX reload).

- [ ] **Step 7: Live-verify the limit fires (after ArgoCD sync, NOT against prod with real users — use a synthetic loop against the PAR endpoint).**

```bash
# Expect the first ~5 to pass (burst), then 429s.
for i in $(seq 1 30); do
  curl -s -o /dev/null -w '%{http_code}\n' -X POST https://entryway.divine.video/api/atproto/oauth/par
done | sort | uniq -c
```
Expected: a mix of `400`/`401` (bad PAR body, but request accepted) for the first burst, then `429` once the zone is exhausted. **If you see zero `429`, the filter is not attached** — recheck the HTTPRoute path match.

- [ ] **Step 8: Document the chosen limits** in `docs/runbooks/launch-checklist.md` (this is the only divine-sky write — see Task H4) and in `../divine-iac-coreconfig` PR description.

---

## Task H2: DMCA / takedown intake into the moderation queue

The queue and the action machinery already exist (`user_reports` D1 table; `inbound-publisher.mjs` `!takedown`→delete). What is missing is a **documented, auditable intake path** that turns a DMCA notice (email/web form) into a row the moderation pipeline acts on, and that records the legally-required fields.

**Sibling-repo files (spec — edited in a `divine-moderation-service` PR, NOT here):**
- Read first: `../divine-moderation-service/src/reports.mjs` (the `user_reports` schema + `addReport`)
- Read first: `../divine-moderation-service/src/atproto/inbound-publisher.mjs` (`!takedown`/`!suspend` mapping)
- Create: `../divine-moderation-service/src/moderation/dmca-intake.mjs`
- Modify: `../divine-moderation-service/src/index.mjs` (route registration)

- [ ] **Step 1: Confirm the queue + action path still exist (read-only).**

```bash
grep -n 'CREATE TABLE IF NOT EXISTS user_reports' ../divine-moderation-service/src/reports.mjs
grep -n "'!takedown'\|action: 'delete'" ../divine-moderation-service/src/atproto/inbound-publisher.mjs
```
Expected: the `user_reports` DDL and the `!takedown`→`delete` mapping are both present. If absent, **stop and re-spec** — the intake design assumes both.

- [ ] **Step 2: Spec a dedicated DMCA intake record.** The generic `user_reports` row (`sha256, reporter_pubkey, report_type, reason`) is insufficient for a DMCA notice (no claimant identity, no sworn statement, no counter-notice tracking). Spec a `dmca_notices` D1 table with the legally-meaningful fields:

```sql
CREATE TABLE IF NOT EXISTS dmca_notices (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  sha256 TEXT,                       -- target content hash (nullable until matched)
  nostr_event_id TEXT,               -- target Nostr event (nullable)
  atproto_uri TEXT,                  -- target at:// record (nullable)
  claimant_name TEXT NOT NULL,
  claimant_email TEXT NOT NULL,
  work_description TEXT NOT NULL,
  good_faith_statement INTEGER NOT NULL,   -- bool: claimant asserted good-faith belief
  accuracy_statement INTEGER NOT NULL,     -- bool: claimant swore accuracy under penalty of perjury
  status TEXT NOT NULL DEFAULT 'received', -- received|actioned|rejected|counter-noticed
  takedown_ref TEXT,                       -- ties to the PDS takedownRef, e.g. DMCA-2026-05-30-001
  created_at TEXT DEFAULT CURRENT_TIMESTAMP,
  actioned_at TEXT
);
```

- [ ] **Step 3: Spec the intake handler** (`dmca-intake.mjs`): accept a notice (authenticated admin route or a form-backed endpoint), validate the required sworn fields are present, insert the `dmca_notices` row with `status='received'`, and on operator confirmation emit the **existing** `!takedown` action so it flows through `inbound-publisher.mjs` → Nostr `delete` AND through the labeler to the AT repo. Set `takedown_ref` to a stable id so the PDS-side takedown (the `takedownRef = SPAM-BOT-TAKEDOWN-...` pattern from the 2026-03 incident) is auditable.

- [ ] **Step 4: Spec the PDS-side takedown linkage.** A DMCA takedown must also stop the AT record from resolving. Confirm the path: a `!takedown` label written by `divine-labeler` against the `at://` URI is honored by the appview, **and** for hard removal the operator runs the admin takedown (same mechanism as the 2026-03 incident: set `takedownRef` on the record/repo via `com.atproto.admin.updateSubjectStatus`). Document both in the runbook (H4).

- [ ] **Step 5: Define the human SLA + audit trail in the runbook** (H4): where notices arrive (the DMCA agent email/form), who triages, target turnaround, and that every `dmca_notices` row maps to exactly one `takedown_ref`. **This task is "done" when a test DMCA notice can be filed end-to-end and produces both a `dmca_notices` row and a takedown action — verified in the moderation-service PR, gated here.**

- [ ] **Step 6: Acceptance gate.** Before any public creator onboarding, confirm:
```bash
# In the moderation-service PR's deployed worker (staging), file a synthetic notice and confirm it lands.
npx wrangler d1 execute <moderation-db> --command \
  "SELECT id, status, takedown_ref FROM dmca_notices ORDER BY created_at DESC LIMIT 1;"
```
Expected: one row, `status` advancing `received`→`actioned`, non-null `takedown_ref`.

---

## Task H3: Verify the disable ⇒ 404 contract (cross-check Chunk C)

The user-facing safety contract: disabling an account must make `username.divine.video/.well-known/atproto-did` stop resolving (router 404) and must stop new mirrored posts. This spans `divine-handle-gateway` (writes the disabled state), keycast + `divine-name-server` (propagate it), and `divine-router` (serves the 404). **It depends on Chunk C** — the router only reads the right KV record if `KV_STORE_NAME` ("divine-names") matches the published Fastly store id.

**Files (read-only verification + assertions; no edits in any repo for this task):**
- Read: `crates/divine-handle-gateway/src/routes/disable.rs` (this repo)
- Read: `crates/divine-handle-gateway/src/lib.rs:196` (`sync_disabled_state`)
- Read: `../divine-router/src/main.rs:336` (`handle_atproto_did`) and `:15` (`KV_STORE_NAME`)

- [ ] **Step 1: Confirm the disable code path still enforces the contract (read-only).**

```bash
grep -n 'sync_disabled_state\|NOT_FOUND\|BAD_GATEWAY' crates/divine-handle-gateway/src/routes/disable.rs
grep -n 'sync_disabled\|sync_state_for_handle\|"disabled"' crates/divine-handle-gateway/src/lib.rs
grep -n 'status == "active"\|atproto_state\|"ready"\|atproto_did' ../divine-router/src/main.rs
```
Expected:
- `disable.rs`: on success calls `sync_disabled_state` and returns `NOT_FOUND` when the record is missing, `BAD_GATEWAY` if the downstream sync fails (the disable is not silently swallowed).
- `lib.rs`: `sync_disabled_state` calls `keycast_client.sync_disabled` **and** `name_server_client.sync_state_for_handle(handle, None, "disabled")`.
- `main.rs`: `handle_atproto_did` returns `404` unless `status == "active" && atproto_state == "ready" && atproto_did.is_some()`.

- [ ] **Step 2: Cross-check Chunk C is satisfied (the 404 path reads the right store).**

```bash
grep -n 'KV_STORE_NAME\|"divine-names"\|"usernames"' ../divine-router/src/main.rs
grep -n 'store_id\|kv_stores\|divine-names\|usernames' ../divine-router/fastly.toml
```
Expected: the constant in `main.rs` and the store name/id in `fastly.toml` agree (Chunk C). **If they disagree, Chunk H3's live test below will give a false 404 (right answer, wrong reason) — do not sign off H3 until Chunk C is merged and verified.**

- [ ] **Step 3: Live lifecycle test (run after Chunk C is live).** Pick a disposable `ready` test username (the 2026-03 incident preserved `vinetest.divine.video` etc. as safe test accounts — reuse one, do not touch a real creator).

```bash
U=vinetest   # a known test handle; confirm it is ready first
# Pre: resolves to a DID
curl -fsS https://$U.divine.video/.well-known/atproto-did ; echo " <- expect a did:plc:..."

# Disable via the handle-gateway disable route (admin path; use the real internal endpoint)
# (exact endpoint/auth per crates/divine-handle-gateway/src/routes/mod.rs)
curl -fsS -X POST https://login.divine.video/api/user/atproto/disable -H "Authorization: Bearer $ADMIN" -d "{\"nostr_pubkey\":\"<pk>\"}"

# Post: 404 and empty body
curl -s -o /dev/null -w '%{http_code}\n' https://$U.divine.video/.well-known/atproto-did   # expect 404
```
Expected: DID before, `404` after. If still resolving after disable, the failure is one of: (a) name-server didn't get `"disabled"`, (b) Fastly KV not purged, (c) Chunk C store mismatch. Triage in that order.

- [ ] **Step 4: Confirm disable also stops new mirrored posts.** The crosspost gate is `crosspost_enabled && ready` (parent plan, `divine-atbridge`). Disabling flips `ready`→not-ready, so the bridge stops enqueuing. Confirm no new publish job is created for the disabled pubkey after disable:
```bash
# In the bridge DB (staging):
# SELECT count(*) FROM publish_jobs WHERE nostr_pubkey='<pk>' AND created_at > '<disable ts>';  -- expect 0
```

- [ ] **Step 5: Confirm the re-enable path restores resolution** (so disable is reversible, matching the runbook's "without deleting existing AT records" rollback). Re-enable and re-check the DID resolves. This proves disable is a gate, not data loss.

---

## Task H4: Land the safety contract in the launch checklist (the ONLY divine-sky write)

The launch checklist already has a `## Safety` section (`docs/runbooks/launch-checklist.md:47–54`) with the three Chunk-H bullets, but they are aspirational ("Route DMCA…", "Confirm disable flow…"). Replace them with the **concrete, verifiable** form produced by H1–H3 so the checklist is a real gate.

**Files:**
- Modify: `docs/runbooks/launch-checklist.md` (this repo — allowed)

- [ ] **Step 1: Read the current Safety + Pre-Flight sections** (lines ~43–54) to anchor the edits.

- [ ] **Step 2: Replace the three soft bullets** with the verified contract:
  - Rate limits: name the `SnippetsFilter` resources (`entryway-oauth-ratelimit`, `pds-xrpc-write-ratelimit`, `pds-account-create-ratelimit`), the chosen rates (PAR 10r/m, token 60r/m, PDS writes 120r/m, account-create 5r/m), and the Step-7 `429` synthetic check as the proof command.
  - DMCA: name the `dmca_notices` table + intake handler, the DMCA-agent intake address, the `takedown_ref` audit linkage, and the Step-6 D1 acceptance query.
  - Disable→404: link to Chunk C as a hard dependency; embed the H3 Step-3 before/after `atproto-did` check as the proof.
- [ ] **Step 3: Add an explicit "blocks broad opt-in" gate line:** "Do not advance past the small-creator cohort until H1 (429 observed), H2 (test DMCA notice actioned), and H3 (disable→404 observed) are all green."
- [ ] **Step 4: Cross-link** to this sub-plan and to `chunk-C` for the KV dependency.
- [ ] **Step 5: Verify the doc has no remaining soft language for these three items.**

```bash
grep -n -iE 'rate limit|dmca|disable flow|404' docs/runbooks/launch-checklist.md
```
Expected: every hit references a concrete resource/command, not a "should/route/confirm" placeholder.

- [ ] **Step 6: Commit (divine-sky).**

```bash
git add docs/runbooks/launch-checklist.md docs/superpowers/plans/2026-05-30-rollout-chunks/chunk-H-safety.md
git commit -m "docs: chunk H launch safety — edge rate limits, DMCA intake, disable->404 contract"
```

---

## Execution order within Chunk H

1. **H3 first (read-only)** — it's pure verification and surfaces the Chunk C dependency early; cheap and de-risks the rest.
2. **H1** (IAC PR) — highest abuse-mitigation value; the rate limits are the direct lesson of the 2026-03 incident.
3. **H2** (moderation-service PR) — legally required before public onboarding; can proceed in parallel with H1.
4. **H4** (this repo) — lands once H1–H3 land their concrete values, so the checklist cites real resources/commands.

## Done = all true

- [ ] `429` observed from the synthetic PAR loop against entryway, and from a synthetic write loop against PDS (H1 Step 7).
- [ ] A synthetic DMCA notice produces a `dmca_notices` row that advances to `actioned` with a non-null `takedown_ref`, and the target stops resolving (H2 Step 6).
- [ ] A test username goes DID→disable→`404`→re-enable→DID, with Chunk C confirmed merged (H3 Steps 3 & 5).
- [ ] `docs/runbooks/launch-checklist.md` Safety section cites the real resources/commands and the "blocks broad opt-in" gate (H4).

## Risks

- **Duplicate `limit_req_zone` names** across SnippetsFilters attached to the same NGINX gateway cause a config-reload failure that can take down *all* routes on `main-gateway`, not just entryway/PDS. Zone names must be unique cluster-wide; verify with the H1 Step 6 render check before sync.
- **False-green disable→404.** If Chunk C's KV store mismatch is unresolved, the router 404s for *every* lookup (store open fails), so H3 Step 3 passes for the wrong reason. H3 Step 2 guards this — do not skip it.
- **Rate limits too tight** throttle legitimate refresh traffic (keycast rotates refresh tokens, so `/token` is hot). The token zone (60r/m, burst 20) is deliberately looser than PAR; watch for `429` on `/token` in the cohort ramp and loosen if real users hit it.
- **DMCA intake without a human SLA** is theater. H2 Step 5 makes the operator turnaround + audit trail part of "done," not optional.
