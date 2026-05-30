# Chunk I — End-to-End Launch + Cohort Ramp (sub-plan)

> **Parent plan:** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk I). Read it first.
>
> **Primary repo:** ops. **Editability:** `cross-repo-spec-only`.
>
> **This chunk does NOT edit code.** It is a *run-and-verify* sequence. The only artifacts it changes are operator records: the launch checklist and the live-results log. Everything else is `curl`, `psql`, `kubectl`, and reading output. Where a fix is needed, the fix belongs to an earlier chunk (A/B/C/D/F/G/H) — Chunk I consumes those outcomes, it does not redo them.
>
> **Sibling repos are read-only here.** Do not modify `../rsky`, `../keycast`, `../divine-router`, `../divine-name-server`, `../divine-iac-coreconfig`. Reference them; do not patch them.

---

## Entry gates (ALL must be green before any step below)

Chunk I is the LAST chunk in the order `0 → A → D → B → C → F → G+H → E → I`. Do not start until:

- [ ] **Chunk 0 (live probe) recorded** — entryway returns `/api/atproto/oauth/*`, PDS protected-resource returns JSON (not 404). Live results logged in `docs/runbooks/launch-checklist.md` or the plan.
- [ ] **Chunk A merged** — `scripts/smoke-divine-atproto-login.sh` asserts `/api/atproto/oauth/{authorize,par,token}` (NOT `/api/oauth/*`). Verify with:
  ```bash
  grep -n 'api/atproto/oauth' scripts/smoke-divine-atproto-login.sh
  ```
  Expected: lines for `authorization_endpoint` and `pushed_authorization_request_endpoint` both contain `/api/atproto/oauth/`. If you still see `/api/oauth/authorize` here, **stop — Chunk A is not merged.**
- [ ] **Chunk D green (SECURITY GATE)** — `cargo test -p rsky-pds entryway_token_trust` passes in `../rsky` (entryway-signed token accepted, wrong-key token rejected). Without this, do not ramp.
- [ ] **Chunk B green (CORRECTNESS GATE)** — no `latest` tag in any of the five production overlays. The PDS image that ships carries `/.well-known/oauth-protected-resource` + the Chunk D test. Without pinned images, "what's deployed" is unknown — do not ramp.
- [ ] **Chunk C merged** — `divine-router` Fastly KV store name + id reconciled; `/.well-known/atproto-did` serves for ready users and 404s otherwise.
- [ ] **Chunk G live** — lease-expiry watchdog + `publish_backfill_state='failed'` alert wired into the alerting stack. These are the *between-cohort gates* consumed in Task I4.
- [ ] **Chunk H live** — entryway PAR/token + PDS XRPC rate limits set; DMCA/takedown intake routed into moderation; disable→404 contract confirmed.

> **Top risk:** ramping creators onto an **unpinned PDS (B not done)** or an **unverified entryway token-trust chain (D not done)**. Either makes the protocol claims unproven against the bytes actually running. B and D are gates, not nice-to-haves.

---

## Preconditions for a test account

You need one DiVine account that has walked the full lifecycle to `ready`. The opt-in runbook (`docs/runbooks/atproto-opt-in-smoke-test.md`) provisions it. For the e2e walk below, assume:

- `HANDLE=rabble.divine.video` (or another `ready` handle you control)
- A cookie-auth session on `https://login.divine.video` for the same account (for the human/console steps)
- Operator access to the `divine-handle-gateway` internal API (Bearer `INTERNAL_AUTH_TOKEN`) for verification queries
- Operator `psql` access to the `divine-atbridge` Postgres (the `publish_jobs` / `account_links` DB)
- `curl`, `jq`, `openssl`, `ruby` (>= 3, with `openssl` stdlib) on the operator workstation

Set shared env once:

```bash
export HANDLE=rabble.divine.video
export ENTRYWAY=https://entryway.divine.video
export PDS=https://pds.divine.video
export LOGIN=https://login.divine.video
# Internal verification surface (NOT the creator path):
export GW=http://divine-handle-gateway.sky.svc.cluster.local:3000   # via kubectl port-forward in practice
export GW_TOKEN='<INTERNAL_AUTH_TOKEN from GCP Secret Manager>'
```

---

## Task I1 — Protocol smoke (corrected paths)

The discovery chain must be healthy before any client login. This runs the Chunk-A-corrected script plus the two raw metadata probes.

- [ ] **Step 1: Run the corrected login-chain smoke**

  ```bash
  bash scripts/smoke-divine-atproto-login.sh
  ```
  Expected last line:
  ```
  PASS: Divine ATProto login contract is healthy for rabble.divine.video
  ```
  This validates: handle → DID (`com.atproto.identity.resolveHandle`), `username.divine.video/.well-known/atproto-did` matches, PLC doc serviceEndpoint = `https://pds.divine.video`, `describeServer` did = `did:web:pds.divine.video`, protected-resource `authorization_servers` includes `https://entryway.divine.video`, entryway metadata endpoints under `/api/atproto/oauth/`.

- [ ] **Step 2: Probe PDS protected-resource directly**

  ```bash
  curl -fsS "$PDS/.well-known/oauth-protected-resource" | jq '{resource, authorization_servers}'
  ```
  Expected:
  ```json
  {
    "resource": "https://pds.divine.video",
    "authorization_servers": ["https://entryway.divine.video"]
  }
  ```
  A `404` here means the running PDS predates the protected-resource endpoint → **stop, return to Chunk B (pin + redeploy rsky-pds).**

- [ ] **Step 3: Probe entryway authorization-server metadata directly**

  ```bash
  curl -fsS "$ENTRYWAY/.well-known/oauth-authorization-server" \
    | jq '{issuer, authorization_endpoint, token_endpoint, pushed_authorization_request_endpoint, scopes_supported, token_endpoint_auth_methods_supported, require_pushed_authorization_requests, client_id_metadata_document_supported}'
  ```
  Expected:
  ```json
  {
    "issuer": "https://entryway.divine.video",
    "authorization_endpoint": "https://entryway.divine.video/api/atproto/oauth/authorize",
    "token_endpoint": "https://entryway.divine.video/api/atproto/oauth/token",
    "pushed_authorization_request_endpoint": "https://entryway.divine.video/api/atproto/oauth/par",
    "scopes_supported": ["atproto"],
    "token_endpoint_auth_methods_supported": ["none", "private_key_jwt"],
    "require_pushed_authorization_requests": true,
    "client_id_metadata_document_supported": true
  }
  ```
  If `authorization_endpoint` is `/api/oauth/authorize` (no `atproto`), the running keycast disagrees with current source → **stop, reconcile the deployed keycast commit (Chunk 0 branch), do not proceed.**

---

## Task I2 — Real Bluesky-client login (PAR → authorize → token → refresh → PDS call)

Exercise the confidential-/public-client OAuth path against the *running* entryway, ending in an authenticated call to `pds.divine.video`. This is the one path that proves DPoP, ready-gating, refresh rotation, and entryway→PDS token trust in production.

Two ways to do this; do at least one:

- [ ] **Step 1 (preferred): Real third-party Bluesky client.**
  In an actual Bluesky-compatible client (e.g. a self-hosted `@atproto/oauth-client-node` test app, or a Statusphere-style demo client), sign in with `rabble.divine.video`.
  - Expected: client resolves handle → DID → PDS → entryway, opens `https://login.divine.video` consent, and after approval the client receives a working DPoP-bound session.
  - Confirm the client can read the session (e.g. `app.bsky.actor.getProfile` for the same DID) without error.

- [ ] **Step 2 (scripted fallback): Run the auth-server smoke runbook end to end.**
  Execute `docs/runbooks/atproto-auth-server-smoke-test.md` sections **3 → 7** verbatim against the env above (it already targets `/api/atproto/oauth/*` and `pds.divine.video`):
  - **§3 PAR** → `request_uri` returned, `DPoP-Nonce` header present.
  - **§4 Authorize** (browser, logged into `login.divine.video`) → callback carries `code`, `state=smoke-test-state`, `iss=https://entryway.divine.video`.
  - **§5 Token exchange** → `token_type=DPoP`, `scope=atproto`, `sub=<did:plc>`, `access_token` + `refresh_token` present, access token payload has `cnf.jkt`.
  - **§6 Refresh rotation** → new `access_token` AND new `refresh_token`; the returned refresh token differs from the prior one.
  - **§7 PDS call** → first request `400` + `DPoP-Nonce`, retry returns `200` with `did` == the token `sub`.

  > Do NOT inline the DPoP ruby here — it lives in the runbook and inlining it risks drift. Cite §3–§7 and run them.

- [ ] **Step 3: Record evidence** (the runbook's "Evidence To Capture" list): both discovery JSONs, PAR payload, callback URL, token `sub`, refresh rotation, `getSession` DID. Paste into the launch-results log.

> **Gate:** if §6 returns the *same* refresh token, or §7 never reaches `200`, the production OAuth/token-trust chain is broken. **Stop. This is a launch blocker** (and, if §7 accepts a token the PDS should reject, a security finding for Chunk D).

---

## Task I3 — Lifecycle walk (claim → enable → ready → resolves → disable → 404)

Walk one account through the full state machine. The **creator-facing** path is the keycast console (`login.divine.video` settings, backed by keycast `/api/user/atproto/*`); the `divine-handle-gateway` `/api/account-links/*` API is **internal Bearer-authed operator verification**, not the creator path. The public **resolve / 404** contract is `divine-router` `/.well-known/atproto-did`.

- [ ] **Step 1: Claim username (console).**
  In `login.divine.video` → `settings/security` → `Bluesky Account` card, claim `username.divine.video` if not already claimed.
  - Expected UI: `Enable Bluesky account`; status `enabled = false`, `state = null`.
  - Claiming must NOT auto-enable ATProto.

- [ ] **Step 2: Verify NOT-resolving yet (public edge).**
  ```bash
  curl -s -o /dev/null -w '%{http_code}\n' "https://${HANDLE}/.well-known/atproto-did"
  ```
  Expected: `404` (claimed but not enabled → no public resolution).

- [ ] **Step 3: Enable ATProto (console).**
  Click `Enable Bluesky account`.
  - Expected UI: provisioning in progress; status `enabled = true`, `state = pending`.

- [ ] **Step 4: Verify `pending` then `ready` (internal operator surface).**
  Port-forward and query the gateway status endpoint for the account's npub:
  ```bash
  kubectl -n sky port-forward svc/divine-handle-gateway 3000:3000 &
  curl -fsS -H "Authorization: Bearer $GW_TOKEN" \
    "http://localhost:3000/api/account-links/<npub>/status" | jq '{provisioning_state, crosspost_enabled, did, publish_backfill_state}'
  ```
  Expected progression across two polls:
  ```json
  { "provisioning_state": "pending", "crosspost_enabled": true, "did": null, "publish_backfill_state": "not_started" }
  ...
  { "provisioning_state": "ready", "crosspost_enabled": true, "did": "did:plc:...", "publish_backfill_state": "..." }
  ```
  Console should also show `@username.divine.video` + the `did:plc:...`.

- [ ] **Step 5: Verify public read model + handle resolution (ready).**
  ```bash
  curl -fsS "https://${HANDLE}/.well-known/atproto-did"          # bare DID, e.g. did:plc:...
  curl -fsS "https://public.api.bsky.app/xrpc/com.atproto.identity.resolveHandle?handle=${HANDLE}" | jq -r .did
  ```
  Expected: both print the same `did:plc:...`. (This is the user-facing "handle resolves" contract.)

- [ ] **Step 6: Disable ATProto (console).**
  Click disable in the `Bluesky Account` card.
  - Expected UI: `enabled = false`, `state = disabled`; "public DID resolution and future cross-posting are disabled".

- [ ] **Step 7: Verify disable → 404 (public edge).**
  ```bash
  curl -s -o /dev/null -w '%{http_code}\n' "https://${HANDLE}/.well-known/atproto-did"
  ```
  Expected: `404`. (Cross-checks Chunk H / Chunk C disable-safety contract.)
  Internal confirmation:
  ```bash
  curl -fsS -H "Authorization: Bearer $GW_TOKEN" \
    "http://localhost:3000/api/account-links/<npub>/status" | jq '.provisioning_state'
  ```
  Expected: `"disabled"`.

> For Task I4 you will RE-ENABLE this account (or use a separate dedicated `ready` + `crosspost_enabled` account). Re-enable via the console; confirm `provisioning_state=ready`, `crosspost_enabled=true` before crossposting.

---

## Task I4 — Crosspost walk (post → mirror → oldest-first backlog → delete-cancel)

Prove the durable scheduler: live posts mirror immediately, historical backlog drains oldest-first, deletes cancel queued-but-unpublished jobs. Verification is SQL against the `divine-atbridge` Postgres (`publish_jobs`, `account_links`). Mirrors the opt-in runbook steps 12–17.

Set up `psql`:
```bash
kubectl -n sky port-forward svc/divine-atbridge-db 5432:5432 &   # or your DB access path
export PGURL='postgres://...'   # the bridge DB
```

- [ ] **Step 1: Confirm backfill seeding for the freshly-ready account.**
  ```bash
  psql "$PGURL" -c "SELECT nostr_pubkey, provisioning_state, crosspost_enabled, publish_backfill_state, publish_backfill_error FROM account_links WHERE nostr_pubkey = '<hex-pubkey>';"
  ```
  Expected: `publish_backfill_state` walks `not_started → in_progress → completed`; `publish_backfill_error` null.

- [ ] **Step 2: Verify backlog rows drain oldest-first.**
  ```bash
  psql "$PGURL" -c "SELECT nostr_event_id, job_source, state, event_created_at, completed_at FROM publish_jobs WHERE nostr_pubkey = '<hex-pubkey>' AND job_source = 'backfill' ORDER BY event_created_at ASC;"
  ```
  Expected: rows ordered by `event_created_at` ASC; earlier `event_created_at` rows reach `state='published'` (with `completed_at`) before later ones. No later-timestamp backfill job completes before an earlier still-queued one.

- [ ] **Step 3: Publish a NEW live post and confirm it mirrors immediately.**
  As the creator, publish a fresh NIP-71 video event. Then:
  ```bash
  psql "$PGURL" -c "SELECT nostr_event_id, job_source, state, event_created_at, completed_at FROM publish_jobs WHERE nostr_pubkey = '<hex-pubkey>' AND job_source = 'live' ORDER BY created_at DESC LIMIT 5;"
  ```
  Expected: the new event appears with `job_source='live'`, reaches `state='published'` quickly — **without** waiting for remaining `backfill` rows to drain (live lane overtakes backlog; this is the intended trade-off).
  Confirm on Bluesky: the post is visible on the creator's `username.divine.video` profile (e.g. via `app.bsky.feed.getAuthorFeed` for the DID, or the Bluesky app).

- [ ] **Step 4: Confirm a delete cancels a queued backlog job (delete-cancel).**
  Identify a backfill job still `state='queued'` (not yet published):
  ```bash
  psql "$PGURL" -c "SELECT nostr_event_id FROM publish_jobs WHERE nostr_pubkey='<hex-pubkey>' AND job_source='backfill' AND state='queued' ORDER BY event_created_at ASC LIMIT 1;"
  ```
  As the creator, issue a Nostr delete (kind 5) targeting that event id. Then:
  ```bash
  psql "$PGURL" -c "SELECT nostr_event_id, job_source, state FROM publish_jobs WHERE nostr_event_id = '<that-event-id>';"
  ```
  Expected: `state='skipped'` (the create never publishes; no ATProto delete is emitted for a record that never existed). Confirm no corresponding record on Bluesky.

> **Gate:** if a deleted-target job still publishes (`state='published'`), the delete-cancel path is broken — **stop before ramp.** If live posts block behind backlog (`live` job stuck `queued` while backfill drains), lane isolation is broken — **stop.**

---

## Task I5 — Cohort ramp (internal → creator → opt-in), Chunk G alerts as gates

Ramp in three stages. Between every stage, the two launch-checklist ops queries and the Chunk G alerts MUST be clean. Do not advance on a dirty gate.

### The between-cohort gate (run before EACH widening)

- [ ] **Gate A — stuck leased jobs** (`docs/runbooks/launch-checklist.md` lines 70-71):
  ```bash
  psql "$PGURL" -c "SELECT nostr_event_id, job_source, lease_owner, lease_expires_at FROM publish_jobs WHERE state = 'in_progress' ORDER BY lease_expires_at ASC NULLS FIRST;"
  ```
  Expected: no rows with `lease_expires_at` in the past. Any expired-lease row in `in_progress` → the Chunk G lease-expiry watchdog should already be alerting — **hold the ramp until it clears.**

- [ ] **Gate B — failed backlog planning** (`docs/runbooks/launch-checklist.md` lines 72-73):
  ```bash
  psql "$PGURL" -c "SELECT nostr_pubkey, did, publish_backfill_error FROM account_links WHERE publish_backfill_state = 'failed' ORDER BY updated_at DESC;"
  ```
  Expected: zero rows. Any row here is a live Chunk G `publish_backfill_state='failed'` alert — **hold the ramp**, triage, clear before widening.

- [ ] **Gate C — protocol smoke still green:** re-run `bash scripts/smoke-divine-atproto-login.sh` → `PASS`.

- [ ] **Gate D — alerting stack quiet:** confirm no firing alerts for relay disconnect loops, PDS write failures, expired publish-job leases, or `publish_backfill_state=failed` (Chunk G/H safety alerts).

### Stage 1 — Internal cohort

- [ ] **Step 1:** Opt in only internal/staff accounts (start with the I2/I3/I4 test account).
- [ ] **Step 2:** Run Tasks I1, I3, I4 end to end on at least one internal account.
- [ ] **Step 3:** Hold ≥ 24h. Run the between-cohort gate (A–D). All clean → proceed.

### Stage 2 — Small creator cohort

- [ ] **Step 1:** Opt in a hand-picked small set of real creators (single-digit count) via the console.
- [ ] **Step 2:** Spot-check each: handle resolves (I3 §5), at least one live post mirrors (I4 §3), backlog drains oldest-first (I4 §2).
- [ ] **Step 3:** Confirm Chunk H abuse controls are exercised under real traffic: rate limits not tripping legitimate creators; DMCA/takedown intake reachable.
- [ ] **Step 4:** Hold ≥ 48h. Run the between-cohort gate (A–D). All clean → proceed.

### Stage 3 — Broader opt-in

- [ ] **Step 1:** Open self-serve opt-in (the real server-side gate `crosspost_enabled && ready` — per Chunk E, there is no separate client publishing flag).
- [ ] **Step 2:** Watch Gate A/B continuously (these are the Chunk G alerts; they should be wired to page, not just queried by hand).
- [ ] **Step 3:** If any gate goes dirty, use the launch-checklist rollback path: disable new opt-ins, stop the bridge **without deleting existing AT records**, confirm the Fastly KV record for affected usernames no longer advertises `atproto_did` / `atproto_state=ready`.

---

## Task I6 — Record launch results

- [ ] **Step 1:** Append a dated "Launch e2e results" section to `docs/runbooks/launch-checklist.md` with: live probe results (Chunk 0), I1 metadata JSONs, I2 token `sub` + refresh-rotation evidence, I3 resolve/404 transitions, I4 SQL outputs (oldest-first + delete-skip), and the ramp gate readings between each stage.
- [ ] **Step 2: Commit (this repo only — ops record, not code):**
  ```bash
  git add docs/runbooks/launch-checklist.md docs/superpowers/plans/2026-05-30-rollout-chunks/chunk-I-launch.md
  git commit -m "docs: record atproto launch e2e results and cohort ramp gates"
  ```

---

## Done criteria

- [ ] I1 protocol smoke PASS against production paths `/api/atproto/oauth/*`.
- [ ] I2 real client (or runbook §3–§7) login + refresh-rotation + authenticated PDS `200`.
- [ ] I3 full lifecycle: claim → enable → ready → handle resolves → disable → 404.
- [ ] I4 crosspost: live mirrors immediately, backlog oldest-first, delete cancels a queued job (`state='skipped'`).
- [ ] I5 ramp completed internal → creator → broad, every between-cohort gate (Chunk G alerts + the two launch-checklist SQL queries) clean at each step.
- [ ] No sibling repo modified by this chunk (verify: `git -C ../rsky status`, `git -C ../keycast status`, `git -C ../divine-router status`, `git -C ../divine-iac-coreconfig status` all clean of Chunk-I edits).
