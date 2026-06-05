# Chunk A — Reconcile the ATProto OAuth Path Contract (divine-sky)

> **Parent plan:** `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk A). Read it first.
> **Repo:** divine-sky only. **Editability:** in-repo-divine-sky (all edits land in this repo).
> **Sub-skill:** Use `superpowers:executing-plans` or `superpowers:subagent-driven-development`. Steps use `- [ ]` checkboxes.

## Problem (verified ground truth)

keycast's deployed ATProto Authorization Server (`origin/main`, deployed image `bd92361`) serves and advertises its ATProto OAuth endpoints under **`/api/atproto/oauth/{par,authorize,token}`** — a namespace distinct from keycast's pre-existing **generic Nostr/UCAN** OAuth at `/api/oauth/{authorize,token}`.

Inside divine-sky the contract is **internally inconsistent today**:

- `docs/runbooks/atproto-auth-server-smoke-test.md` (working tree) already uses the correct `/api/atproto/oauth/*` — it was corrected in PR #8 (`f164e4d`, "docs: align entryway smoke test with keycast contract").
- `scripts/smoke-divine-atproto-login.sh` still asserts `/api/oauth/*` for the **entryway** metadata. Those two assertions predate PR #8 (they came in with PR #6, `f35258c`) and PR #8 only added the `--help` mode to the script — it never touched the path assertions. So the script is the stale outlier, disagreeing with the runbook in the same repo.
- One ATProto **spec** (`...atproto-boundary.md`) still describes the auth-server metadata document with `/api/oauth/*`.

A test should assert reality. The smoke script as written would **fail** against the running entryway. This chunk makes divine-sky's script + the one stale ATProto spec match the deployed `/api/atproto/oauth/*` contract, while **leaving keycast's legitimate generic `/api/oauth/*` references untouched**.

## Direction-confirmation caveat (DO THIS BEFORE the Task A2 spec sweep)

The master plan flags that divine-sky `origin/main` PRs #8/#9 normalized *toward* `/api/oauth/*` in places, and that this could mean either (a) a doc error, or (b) an intended future keycast route move. From divine-sky alone this is **not determinable**.

- The **smoke script (Task A1)** is safe to fix **unconditionally**: it must assert whatever the *running* entryway returns. Live source-of-truth is the server, not intent. Confirm with the curl in A1 Step 1 and fix to match.
- The **spec sweep (Task A2)** changes design intent, not just a test. **Before sweeping the spec, get a one-line confirmation from the keycast owners** that `/api/atproto/oauth/*` is the canonical ATProto OAuth namespace and that keycast does **not** intend to move it to `/api/oauth/*`. If keycast intends the move, the fix belongs on the keycast side and divine-sky's spec should track that decision instead — do not sweep blindly.
- This is **in-repo-divine-sky**: even after confirming, you edit only divine-sky files. You never edit the keycast repo here.

---

## Inventory: every `/api/oauth/` reference and its disposition

Authoritative scan (run from repo root):

```bash
grep -rn "api/oauth/" docs/ scripts/
```

| File:line | Context | ATProto OAuth path? | Disposition |
|---|---|---|---|
| `scripts/smoke-divine-atproto-login.sh:230` | `assert_json_string ... authorization_endpoint "https://entryway.divine.video/api/oauth/authorize"` | **YES** (entryway ATProto auth-server metadata) | **FIX** → `/api/atproto/oauth/authorize` (Task A1) |
| `scripts/smoke-divine-atproto-login.sh:231` | `assert_json_string ... pushed_authorization_request_endpoint "https://entryway.divine.video/api/oauth/par"` | **YES** (entryway ATProto PAR) | **FIX** → `/api/atproto/oauth/par` (Task A1) |
| `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md:152` | `"authorization_endpoint": "https://login.divine.video/api/oauth/authorize"` — ATProto auth-server **metadata document** test expectation (Chunk 2, Task 3) | **YES** | **FIX** → `/api/atproto/oauth/authorize` (Task A2) |
| `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md:153` | `"token_endpoint": "https://login.divine.video/api/oauth/token"` — same metadata test | **YES** | **FIX** → `/api/atproto/oauth/token` (Task A2) |
| `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md:154` | `"pushed_authorization_request_endpoint": "https://login.divine.video/api/oauth/par"` — same metadata test | **YES** | **FIX** → `/api/atproto/oauth/par` (Task A2) |
| `docs/superpowers/specs/2026-03-28-login-divine-video-atproto-auth-server-design.md:64` | "Option 1: Reuse the existing keycast `/api/oauth/authorize` and `/api/oauth/token` flow" — describes the **rejected** option of reusing the pre-existing generic UCAN/Nostr flow | **NO** (generic, and a rejected option) | **LEAVE** |
| `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-auth-server.md:183` | "do not change the existing **generic** `/api/oauth/*` behavior unless a shared helper extraction is necessary" | **NO** (explicitly generic Nostr/UCAN) | **LEAVE** |
| `docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (multiple lines: 35, 71, 79, 81, 121, 126, 129, 131, 134) | The **master rollout plan itself**, documenting the mismatch and the fix procedure (it intentionally names both `/api/oauth/*` and `/api/atproto/oauth/*` to contrast them) | **N/A** (meta — describes the contract, is not a contract assertion) | **LEAVE** |

**Net: 5 lines to fix across 2 files** — `scripts/smoke-divine-atproto-login.sh` (2 lines) and `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md` (3 lines). Everything else is either a legitimate generic `/api/oauth/*` reference or the master plan's own prose.

> ⚠️ **Master-plan discrepancy to know about:** Master-plan Task A1 Step 2 says to change a `token_endpoint` assertion in the script to `/api/atproto/oauth/token`. **The script has no `token_endpoint` assertion** — it only asserts `authorization_endpoint` (line 230) and `pushed_authorization_request_endpoint` (line 231). Do not add a `token_endpoint` assertion just to satisfy the master plan's wording; fix only the two lines that exist. (If you *want* token-endpoint coverage, that is a separate enhancement, out of scope for this chunk.)

---

## Task A1 — Fix the smoke script's entryway endpoint assertions (unconditional)

**File to modify:** `scripts/smoke-divine-atproto-login.sh` (lines 230–231)

- [ ] **Step 1: Confirm the live contract (server is the source of truth).** Run from a machine with real egress (NOT the planning sandbox):

```bash
curl -fsS https://entryway.divine.video/.well-known/oauth-authorization-server \
  | jq '{issuer, authorization_endpoint, token_endpoint, pushed_authorization_request_endpoint, scopes_supported, token_endpoint_auth_methods_supported}'
```

Expected:
```json
{
  "issuer": "https://entryway.divine.video",
  "authorization_endpoint": "https://entryway.divine.video/api/atproto/oauth/authorize",
  "token_endpoint": "https://entryway.divine.video/api/atproto/oauth/token",
  "pushed_authorization_request_endpoint": "https://entryway.divine.video/api/atproto/oauth/par",
  "scopes_supported": ["atproto"],
  "token_endpoint_auth_methods_supported": ["none", "private_key_jwt"]
}
```
If the server returns `/api/oauth/*` (no `atproto` segment), **stop** — the running server disagrees with the deployed-source audit; reconcile which keycast commit is actually deployed before editing anything (this is the master plan's Chunk 0 Step 2 "Entryway returns `/api/oauth/*`" branch).

- [ ] **Step 2: Edit line 230 — `authorization_endpoint`.** Replace:
```
assert_json_string "$ENTRYWAY_AUTHZ_BODY" authorization_endpoint "https://entryway.divine.video/api/oauth/authorize" "entryway.divine.video authorization-server metadata"
```
with:
```
assert_json_string "$ENTRYWAY_AUTHZ_BODY" authorization_endpoint "https://entryway.divine.video/api/atproto/oauth/authorize" "entryway.divine.video authorization-server metadata"
```

- [ ] **Step 3: Edit line 231 — `pushed_authorization_request_endpoint`.** Replace:
```
assert_json_string "$ENTRYWAY_AUTHZ_BODY" pushed_authorization_request_endpoint "https://entryway.divine.video/api/oauth/par" "entryway.divine.video authorization-server metadata"
```
with:
```
assert_json_string "$ENTRYWAY_AUTHZ_BODY" pushed_authorization_request_endpoint "https://entryway.divine.video/api/atproto/oauth/par" "entryway.divine.video authorization-server metadata"
```

- [ ] **Step 4: Verify only those two lines changed and they now read `/api/atproto/oauth/`.**

```bash
git diff scripts/smoke-divine-atproto-login.sh
grep -n "api/oauth/" scripts/smoke-divine-atproto-login.sh || echo "no bare api/oauth/ in script"
grep -n "api/atproto/oauth/" scripts/smoke-divine-atproto-login.sh
```
Expected: the `git diff` shows exactly two `-`/`+` line pairs; `grep` for `api/oauth/` prints `no bare api/oauth/ in script`; `grep` for `api/atproto/oauth/` prints lines 230 and 231.

- [ ] **Step 5: Syntax-check the script (no execution against prod needed for this check).**

```bash
bash -n scripts/smoke-divine-atproto-login.sh && echo "syntax ok"
```
Expected: `syntax ok`.

- [ ] **Step 6 (optional, needs egress): run the metadata leg end-to-end.**

```bash
bash scripts/smoke-divine-atproto-login.sh
```
Expected: the entryway authorization-server metadata assertions pass (they would have failed before this fix on the path mismatch). If egress is blocked in the sandbox, defer to Chunk I.

- [ ] **Step 7: Commit.**

```bash
git add scripts/smoke-divine-atproto-login.sh
git commit -m "fix(smoke): assert real /api/atproto/oauth entryway endpoints"
```

---

## Task A2 — Sweep the one stale ATProto spec (GATED on keycast-owner confirmation)

**File to modify:** `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md` (lines 152–154, inside "Chunk 2 / Task 3: Add failing tests for the metadata document").

- [ ] **Step 0 (GATE): get keycast-owner confirmation** that `/api/atproto/oauth/*` is the canonical, intended ATProto OAuth namespace (see "Direction-confirmation caveat" above). Record the confirmation (Slack link / name / date) in the commit body. **Do not proceed if keycast signals an intended move to `/api/oauth/*`.**

- [ ] **Step 1: Re-list the exact ATProto candidates** (this command deliberately filters to ATProto markers so it does NOT match the legitimate generic refs at `auth-server-design.md:64` or `auth-server.md:183`):

```bash
grep -rn "api/oauth/" docs/ | grep -iE 'atproto|entryway|authorization_endpoint|pushed_auth|token_endpoint'
```
Expected hits (besides the master-plan prose, which you leave alone): `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md:152`, `:153`, `:154`.

- [ ] **Step 2: Edit boundary.md line 152.** Replace:
```
- `"authorization_endpoint": "https://login.divine.video/api/oauth/authorize"`
```
with:
```
- `"authorization_endpoint": "https://login.divine.video/api/atproto/oauth/authorize"`
```

- [ ] **Step 3: Edit boundary.md line 153.** Replace:
```
- `"token_endpoint": "https://login.divine.video/api/oauth/token"`
```
with:
```
- `"token_endpoint": "https://login.divine.video/api/atproto/oauth/token"`
```

- [ ] **Step 4: Edit boundary.md line 154.** Replace:
```
- `"pushed_authorization_request_endpoint": "https://login.divine.video/api/oauth/par"`
```
with:
```
- `"pushed_authorization_request_endpoint": "https://login.divine.video/api/atproto/oauth/par"`
```

> Note on host: these three lines describe the **metadata document's** advertised endpoints. The spec uses `login.divine.video` (keycast's own `APP_URL` / issuer in that test). Keep `login.divine.video` here — only the **path** changes. The live entryway metadata uses host `entryway.divine.video`; that host/issuer distinction is intentional and out of scope for this path-only sweep.

- [ ] **Step 5: Add the namespace-disambiguation note** to the boundary spec so future readers don't re-flatten the path. In `docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md`, near the metadata-test section (Chunk 2), add one line:
```
> keycast namespaces ATProto OAuth under `/api/atproto/oauth/*`, distinct from the pre-existing generic Nostr/UCAN `/api/oauth/*`. Never collapse the two.
```

- [ ] **Step 6: Verify the generic refs were NOT touched and no stray ATProto `/api/oauth/` remains.**

```bash
# Generic refs must still be present and unchanged:
grep -n "api/oauth/" docs/superpowers/specs/2026-03-28-login-divine-video-atproto-auth-server-design.md
grep -n "api/oauth/" docs/superpowers/plans/2026-03-28-login-divine-video-atproto-auth-server.md
# No ATProto-flavored /api/oauth/ left anywhere in docs+scripts (master-plan prose is the only allowed hit):
grep -rn "api/oauth/" docs/ scripts/ | grep -iE 'atproto|entryway' | grep -v '2026-05-30-atproto-production-rollout.md' || echo "clean"
```
Expected: the first two `grep`s still print their generic lines (design.md:64, auth-server.md:183); the final command prints `clean`.

- [ ] **Step 7: Commit.**

```bash
git add docs/superpowers/plans/2026-03-28-login-divine-video-atproto-boundary.md
git commit -m "docs: align atproto oauth metadata path with keycast /api/atproto/oauth contract"
```

---

## Done-condition for Chunk A

- [ ] Smoke script asserts `/api/atproto/oauth/{authorize,par}` for entryway (Task A1).
- [ ] The one stale ATProto spec (`...atproto-boundary.md`) advertises `/api/atproto/oauth/{authorize,token,par}` (Task A2, gated).
- [ ] Legitimate generic Nostr/UCAN `/api/oauth/*` refs (design.md:64, auth-server.md:183) are **unchanged**.
- [ ] Final guard passes:
```bash
grep -rn "api/oauth/" docs/ scripts/ | grep -iE 'atproto|entryway' | grep -v '2026-05-30-atproto-production-rollout.md' || echo "clean"
```
Expected: `clean`.
- [ ] If the keycast-owner gate (A2 Step 0) was not obtained, A1 may still ship (it asserts live reality); A2 stays parked until confirmed.

## Out of scope (do not do here)

- Editing keycast (`/Users/rabble/code/divine/keycast`) — this chunk is in-repo-divine-sky; the keycast server already serves `/api/atproto/oauth/*`.
- Adding a new `token_endpoint` assertion to the smoke script (master plan wording references one that does not exist).
- Touching the master rollout plan's own `/api/oauth/*` prose (it intentionally contrasts both paths).
- Rewriting `login.divine.video` → `entryway.divine.video` host references (host vs path is a separate concern; this chunk is path-only).
