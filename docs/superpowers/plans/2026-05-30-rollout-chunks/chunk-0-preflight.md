# Chunk 0 — Pre-Flight Live Probe (run FIRST)

> Parent plan: `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (§ "Chunk 0").
> Editability: **cross-repo-spec-only**. This doc lives in `divine-sky` and only *reads* sibling repos (`rsky`, `keycast`, `divine-iac-coreconfig`). It instructs an operator to probe **running servers**; it does not modify any sibling repo.

**Purpose.** Every "already done, deployed and answering" claim in the parent plan's Reality Check is source-level inference — the planning sandbox blocks external egress, so nothing was confirmed against the live servers. This chunk turns that inference into fact from a machine **with real network egress** (your laptop, a bastion, or CI with outbound internet — NOT the planning sandbox). The result can reorder the whole rollout: if the PDS protected-resource is 404 in prod, **Chunk B (pin + redeploy rsky-pds) jumps ahead of Chunk A.**

**Who runs this.** An operator with: outbound HTTPS to `*.divine.video`, `curl`, `jq`, and read access to the three sibling repos at `/Users/rabble/code/divine/{rsky,keycast,divine-iac-coreconfig}`.

**Time budget.** ~15 minutes for the probes; the branch you land on dictates downstream effort.

---

## Ground truth this chunk checks against (already read from source — do NOT re-derive)

These are the source-of-truth values the live probes are compared to. They were read on 2026-05-30 from the deploy branches/commits, NOT from local `HEAD` or worktrees.

- **Entryway auth-server metadata** — `keycast` `origin/main`, `api/src/api/http/atproto_oauth_metadata.rs`. Endpoints are namespaced under `/api/atproto/oauth/*`:
  - `authorization_endpoint` → `https://entryway.divine.video/api/atproto/oauth/authorize`
  - `token_endpoint` → `https://entryway.divine.video/api/atproto/oauth/token`
  - `pushed_authorization_request_endpoint` → `https://entryway.divine.video/api/atproto/oauth/par`
  - `issuer` → the auth-server origin (expected `https://entryway.divine.video`)
  - `scopes_supported` → `["atproto"]`
  - `token_endpoint_auth_methods_supported` → `["none","private_key_jwt"]`
  - `token_endpoint_auth_signing_alg_values_supported` → `["ES256"]`
  - `dpop_signing_alg_values_supported` → `["ES256"]`
  - `code_challenge_methods_supported` → `["S256"]`
  - `require_pushed_authorization_requests` → `true`
  - `authorization_response_iss_parameter_supported` → `true`
  - `client_id_metadata_document_supported` → `true`
  - `grant_types_supported` → `["authorization_code","refresh_token"]`
  - Deployed keycast image is pinned: `divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml` → `newTag: bd92361`.

- **PDS protected-resource metadata** — `rsky` `divinevideo/main` (commit `413fa35` "feat: publish PDS OAuth protected-resource metadata (#3)"), `rsky-pds/src/well_known.rs`:
  - Route `GET /.well-known/oauth-protected-resource` returns `{ resource: <PDS public_url>, authorization_servers: [<auth server>] }`.
  - **The response is gated on config.** The handler returns the JSON **only if** `cfg.identity.oauth_authorization_server` is `Some` (env `PDS_OAUTH_AUTHORIZATION_SERVER`, read in `rsky-pds/src/config/mod.rs:105`). If that env is unset, the handler returns **404 "OAuth protected resource metadata not configured"** even when the endpoint exists in the binary.
  - ⚠️ **Two independent reasons this can 404 in prod** — the probe cannot distinguish them, so Step 4 inspects IAC to disambiguate:
    1. **Image too old.** `rsky-pds` production overlay is on `newTag: latest` (`divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml:8`) — the running image may predate commit `413fa35` and not contain the route at all.
    2. **Env not set.** `PDS_OAUTH_AUTHORIZATION_SERVER` and `PDS_ENTRYWAY_DID` are **not set anywhere** in `divine-iac-coreconfig/k8s/applications/rsky-pds/` (base, overlays, or external-secret) as of 2026-05-30. With the env unset, even a correct image returns 404, **and** the entryway-token-trust chain (parent plan Chunk D) cannot work because `PDS_ENTRYWAY_DID` drives the audience check.

  This means a 404 here is the **expected default** on the current IAC, not a surprise — Chunk B must pin the image **and** add the two env vars.

- **describeServer** — `rsky-pds` exposes `GET /xrpc/com.atproto.server.describeServer`; `.did` should be the PDS service DID (`did:web:pds.divine.video` per the IAC host contract). Used here only as a liveness + identity sanity check on the PDS.

---

## Task 0: Probe the live chain and branch on the result

### Step 1 — Confirm you are on an egress-capable host

- [ ] **Verify outbound HTTPS works** (the planning sandbox fails this; a real host returns `200`):

```bash
curl -s -o /dev/null -w '%{http_code}\n' https://entryway.divine.video/ || echo "NO EGRESS — move to a host with real network access"
```

Expected: an HTTP status (any of `200`/`301`/`302`/`404` proves egress). A hang/`000`/connection error means **stop — you are on a sandboxed host; this chunk is invalid there.**

### Step 2 — Probe entryway auth-server metadata

- [ ] **Fetch and pretty-print the key fields:**

```bash
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server \
  | jq '{issuer, authorization_endpoint, token_endpoint, pushed_authorization_request_endpoint, scopes_supported, token_endpoint_auth_methods_supported, token_endpoint_auth_signing_alg_values_supported, require_pushed_authorization_requests, authorization_response_iss_parameter_supported, client_id_metadata_document_supported}'
```

Expected (matches keycast `origin/main` source above):

```json
{
  "issuer": "https://entryway.divine.video",
  "authorization_endpoint": "https://entryway.divine.video/api/atproto/oauth/authorize",
  "token_endpoint": "https://entryway.divine.video/api/atproto/oauth/token",
  "pushed_authorization_request_endpoint": "https://entryway.divine.video/api/atproto/oauth/par",
  "scopes_supported": ["atproto"],
  "token_endpoint_auth_methods_supported": ["none", "private_key_jwt"],
  "token_endpoint_auth_signing_alg_values_supported": ["ES256"],
  "require_pushed_authorization_requests": true,
  "authorization_response_iss_parameter_supported": true,
  "client_id_metadata_document_supported": true
}
```

- [ ] **Capture the exact endpoint path namespace** (used by the Step 5 branch). Quick assertion:

```bash
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server \
  | jq -r '.authorization_endpoint' \
  | grep -q '/api/atproto/oauth/authorize' \
  && echo 'ENTRYWAY_PATH=atproto-namespaced (expected)' \
  || echo 'ENTRYWAY_PATH=UNEXPECTED — record the literal value and see Branch 4'
```

### Step 3 — Probe PDS protected-resource metadata (the pivotal probe)

- [ ] **Fetch with headers + status visible** (use `-i`, NOT `-f`, so a 404 body/status is shown rather than failing the command):

```bash
curl -isS https://pds.divine.video/.well-known/oauth-protected-resource | head -20
```

Expected if the PDS is fully wired:

```
HTTP/2 200
content-type: application/json
...

{"resource":"https://pds.divine.video","authorization_servers":["https://entryway.divine.video"]}
```

Expected if the image predates `413fa35` **or** the env is unset (the current-IAC default):

```
HTTP/2 404
...
OAuth protected resource metadata not configured   # env unset on a NEW image
# — or —
HTTP/2 404                                          # route absent on an OLD image (generic 404 / Rocket 404 page)
```

- [ ] **Record a single machine-readable status line:**

```bash
printf 'PDS_PROTECTED_RESOURCE_STATUS=%s\n' \
  "$(curl -s -o /dev/null -w '%{http_code}' https://pds.divine.video/.well-known/oauth-protected-resource)"
```

### Step 4 — Disambiguate a 404 (image-too-old vs env-unset) — only if Step 3 was 404

The live probe cannot tell *why* it 404'd. Read the deployed config (no network needed) to decide which fix Chunk B owns:

- [ ] **Is the prod rsky-pds image floating?**

```bash
grep -n 'newTag' /Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/rsky-pds/overlays/production/kustomization.yaml
```

Expected today: `newTag: latest` → image identity is unknown; could predate the endpoint. Chunk B must pin it to a build of `divinevideo/main` ≥ `413fa35`.

- [ ] **Is the auth-server env set?**

```bash
grep -rn 'PDS_OAUTH_AUTHORIZATION_SERVER\|PDS_ENTRYWAY_DID' \
  /Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/rsky-pds/ \
  || echo 'AUTH_ENV=NOT_SET — Chunk B must add PDS_OAUTH_AUTHORIZATION_SERVER and PDS_ENTRYWAY_DID'
```

Expected today: `NOT_SET`. Even a correct image 404s until this is added; and **Chunk D (token trust) is blocked** without `PDS_ENTRYWAY_DID`.

### Step 5 — Probe describeServer (PDS liveness + identity)

- [ ] **Confirm the PDS answers and reports its DID:**

```bash
curl -fsS https://pds.divine.video/xrpc/com.atproto.server.describeServer | jq '{did, availableUserDomains, inviteCodeRequired}'
```

Expected (host contract is `pds.divine.video`, invite-only per the 2026-03 spam lockdown):

```json
{
  "did": "did:web:pds.divine.video",
  "availableUserDomains": ["..."],
  "inviteCodeRequired": true
}
```

A non-200 / hang here means the PDS itself is unhealthy — escalate before any other branch (the protected-resource 404 is moot if the PDS is down).

### Step 6 — Land on a branch and set the rollout order

Pick the **first** matching row. Record the chosen branch in the results template (Task 1).

- [ ] **Branch 1 — Entryway `/api/atproto/oauth/*` AND PDS protected-resource `200`.** Best case: source audit confirmed against running servers. The live entryway result *also* independently confirms Chunk A's path direction (stronger than a source read). **Order: A → D → B → C → F → G/H → E → I (parent plan default).**

- [ ] **Branch 2 — Entryway `/api/atproto/oauth/*` OK, but PDS protected-resource `404`.** The protocol chain is **broken in prod right now**: clients discovering the PDS get no authorization-server pointer. Use Step 4 to record whether it's image-too-old, env-unset, or both. **Order changes: Chunk B (pin rsky-pds to ≥ `413fa35` AND add `PDS_OAUTH_AUTHORIZATION_SERVER` + `PDS_ENTRYWAY_DID`) becomes the blocking FIRST step, before A.** Re-run Step 3 after B redeploys; do not start Chunk D until protected-resource returns `200` (D also needs `PDS_ENTRYWAY_DID`, which B adds).

- [ ] **Branch 3 — PDS describeServer non-200 / unreachable.** PDS is down or misrouted. **Stop the rollout.** Open an incident on `rsky-pds` health (pod status, ingress/HTTPRoute, MinIO, relay) before re-running any other step. All protocol branches are blocked until the PDS is green.

- [ ] **Branch 4 — Entryway returns `/api/oauth/*` (NOT `/api/atproto/oauth/*`).** The running keycast disagrees with current `origin/main` source. **Do not touch any divine-sky docs.** Stop and reconcile which keycast commit is actually deployed:

```bash
grep -n 'newTag' /Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/keycast/overlays/production/kustomization.yaml   # expect bd92361
cd /Users/rabble/code/divine/keycast && git show bd92361:api/src/api/http/atproto_oauth_metadata.rs | grep -n 'authorization_endpoint\|/api/'
```

If the deployed commit serves `/api/oauth/*`, the path move belongs on the **keycast** side and Chunk A's divine-sky sweep is wrong — escalate to keycast owners (parent plan Chunk A "settle direction" note) before proceeding.

- [ ] **Branch 5 — Entryway metadata itself non-200 / unreachable.** Auth server is down or the metadata route changed. Treat like Branch 3 for entryway: open an incident on keycast/entryway routing; the login chain is dead until it returns. Confirm the HTTPRoute and `keycast` pod health in `divine-iac-coreconfig`.

---

## Task 1: Record the live results (so the team stops re-inferring)

- [ ] **Fill in this template and paste it into `docs/runbooks/launch-checklist.md`** (under a new "Chunk 0 — Live Probe Results" heading) **and** link it from the parent plan's Chunk 0 section. Replace every `<…>` with the literal probe output.

```markdown
## Chunk 0 — Live Probe Results

- Probed at: <YYYY-MM-DD HH:MM TZ>
- Probed from: <hostname / who ran it> (egress confirmed: <yes>)

### Entryway (entryway.divine.video/.well-known/oauth-authorization-server)
- HTTP status: <200>
- issuer: <https://entryway.divine.video>
- authorization_endpoint: <…>
- token_endpoint: <…>
- pushed_authorization_request_endpoint: <…>
- scopes_supported: <["atproto"]>
- token_endpoint_auth_methods_supported: <["none","private_key_jwt"]>
- require_pushed_authorization_requests: <true>
- Path namespace: <atproto-namespaced | /api/oauth | other>

### PDS protected-resource (pds.divine.video/.well-known/oauth-protected-resource)
- HTTP status: <200 | 404 | other>
- Body (if 200): <{"resource":...,"authorization_servers":[...]}>
- If 404 — IAC disambiguation (Task 0 Step 4):
  - prod rsky-pds newTag: <latest | sha>
  - PDS_OAUTH_AUTHORIZATION_SERVER / PDS_ENTRYWAY_DID set in IAC: <NOT_SET | set>

### PDS describeServer (pds.divine.video/xrpc/com.atproto.server.describeServer)
- HTTP status: <200>
- did: <did:web:pds.divine.video>
- inviteCodeRequired: <true>

### Decision
- Branch landed: <1 | 2 | 3 | 4 | 5>
- Resulting rollout order: <e.g. "B → A → D → … (Branch 2)">
- Follow-ups opened: <links to issues / Chunk B tasks>
```

- [ ] **Update the parent plan** (`docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` § "Recommended Order") if Branch 2 reordered B ahead of A — note the live-probe date and result so the ordering rationale stays honest.

- [ ] **Commit the recorded results** (divine-sky only — this chunk never edits a sibling repo):

```bash
cd /Users/rabble/code/divine/divine-sky
git add docs/runbooks/launch-checklist.md docs/superpowers/plans/2026-05-30-atproto-production-rollout.md
git commit -m "docs: record chunk-0 live probe results and confirmed rollout order"
```

---

## Done-when

- [ ] All three probes (entryway metadata, PDS protected-resource, PDS describeServer) ran from an egress-capable host with their literal outputs recorded.
- [ ] Exactly one branch (1–5) is selected and the resulting rollout order is written down.
- [ ] If PDS protected-resource was 404, Task 0 Step 4 recorded *why* (image-too-old vs env-unset vs both) so Chunk B has an unambiguous scope.
- [ ] Results are committed in divine-sky and the parent plan's order reflects reality.
- [ ] No sibling repo (`rsky`, `keycast`, `divine-iac-coreconfig`) was modified by this chunk.
