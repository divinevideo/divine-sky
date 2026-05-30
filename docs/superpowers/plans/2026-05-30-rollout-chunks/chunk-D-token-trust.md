# Chunk D — Verify rsky-pds Entryway Token Trust (TDD sub-plan)

> **Editability: cross-repo-spec-only.** The test described here lands in the **sibling repo `rsky`** (path `/Users/rabble/code/divine/rsky`). This document is the divine-sky-side spec for that change. **Do NOT edit `rsky` while drafting this plan.** Execution of the steps below (writing the test, running `cargo test`) happens in a later, separate session that has the rsky repo as its working tree.
>
> **Parent plan:** `/Users/rabble/code/divine/divine-sky/docs/superpowers/plans/2026-05-30-atproto-production-rollout.md` (Chunk D, lines 147–182). Read it first.
>
> **Deployed reference (pin everything to this — never local HEAD or a worktree):**
> - rsky branch/commit: `divinevideo/main` = `413fa351e6ffd3c332edafc6ddce34e9b52ffe9d` (2026-04-03). Read source only via `git show divinevideo/main:<path>`.
> - IAC: `/Users/rabble/code/divine/divine-iac-coreconfig` `k8s/applications/rsky-pds/`.

---

## 0. Ground truth from the deployed source (read before writing any test)

These facts were verified by reading `git show divinevideo/main:rsky-pds/src/auth_verifier.rs` (1052 lines) and the IAC overlays. **The parent plan's Task D framing assumes a "static entryway signing public key in PDS env" model. That model is wrong for this codebase.** The corrections below are load-bearing — the test must target the path that actually exists, or it proves nothing.

### 0.1 There are TWO unrelated token-verification paths in `auth_verifier.rs`

**Path 1 — session/access bearer tokens (`validate_bearer_token`, lines ~750–818).**
- Verifies the JWT against `PDS_JWT_KEY_K256_PRIVATE_KEY_HEX` (the PDS's own session-signing key); on failure falls back to `PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX`.
- `allowed_audiences` is hard-pinned to `PDS_SERVICE_DID` (see `Refresh::from_request` line ~157, `validate_bearer_access_token` line ~723, `validate_access_token` line ~835).
- This path has **nothing to do with entryway**. It cannot, by construction, accept an entryway-signed token: entryway does not hold the PDS's `PDS_JWT_KEY` or repo key. **Do not write the entryway test against this path.**
- Directly above `validate_access_token` (line 823) is the comment `// @TODO: Implement DPop/OAuth`. **The OAuth/DPoP resource-server path is unimplemented on `divinevideo/main`.** An entryway-issued *OAuth access token* (DPoP-bound, the kind keycast's `/api/atproto/oauth/token` mints) is **not** verified by any guard here yet. Flag this in §5.

**Path 2 — inter-service JWTs (`verify_service_jwt`, lines ~907–960, delegating to `xrpc_server::auth::verify_jwt`).**
- This is the **only** path where an external service's signature is trusted. Trust model:
  1. Read `iss` from the JWT payload.
  2. Resolve the issuer's **DID document** via `id_resolver` (`PDS_DID_PLC_URL=https://plc.directory`, `PDS_ID_RESOLVER_TIMEOUT=30000`).
  3. Pull the `atproto` verification key (`#atproto`, or `#atproto_label` for labelers) out of that DID doc via `get_verification_material` + `get_did_key_from_multibase`.
  4. Verify the signature against that **resolved** key (`rsky_crypto::verify::verify_signature`, with a force-refresh retry for key rotation — see `xrpc_server/auth.rs` lines ~88–112).
  5. Audience is checked by the caller. The **only guard that admits `PDS_ENTRYWAY_DID` as a valid audience is `ModService`** (lines 525–587): it accepts when `payload.aud == PDS_SERVICE_DID` OR (`PDS_ENTRYWAY_DID` is set AND `payload.aud == PDS_ENTRYWAY_DID`).
- **Consequence:** "entryway token trust" in this codebase = "the PDS resolves entryway's DID, finds entryway's `#atproto` public key in the DID doc, and verifies the signature." Trust is **keyed on DID-document resolution, not on a static env public key.** There is no `PDS_ENTRYWAY_SIGNING_PUBLIC_KEY` env var to check.

### 0.2 `PDS_ENTRYWAY_DID` is NOT set in the deployed IAC

`grep -rni ENTRYWAY /Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/rsky-pds/` returns **nothing**. The base deployment env (`base/deployment.yaml`) sets `PDS_SERVICE_DID=did:web:pds.ENVIRONMENT.dvines.org` (patched to `pds.divine.video` in prod), `PDS_JWT_KEY_K256_PRIVATE_KEY_HEX`, `PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX`, `PDS_PLC_ROTATION_KEY_...`, `PDS_DID_PLC_URL=https://plc.directory`, `PDS_INVITE_REQUIRED=true` — but **no `PDS_ENTRYWAY_DID` and no entryway public key**.

So today, the `PDS_ENTRYWAY_DID` audience branch in `ModService` is **dead** in prod: `env_str("PDS_ENTRYWAY_DID").is_none()` is true, so only `PDS_SERVICE_DID`-audience service JWTs are admitted. This is a real finding for §4/§5, not a blocker for the test itself.

### 0.3 What the test can and cannot prove (honest scope)

| Claim from parent plan | Provable as a Rust unit test? | How |
|---|---|---|
| A token signed by the configured signing key, with the right `aud`, is ACCEPTED | **Yes** | Drive `xrpc_server::auth::verify_jwt` with a `get_signing_key` closure returning the matching key. |
| A wrong-key token (same `aud`) is REJECTED with a signature error | **Yes** | Same harness, closure returns a different key. |
| The audience check admits `PDS_ENTRYWAY_DID` | **Yes (guard-level)** | Assert `ModService`'s audience predicate; or unit-test the predicate logic. |
| The DEPLOYED PDS trusts the REAL entryway DID's REAL key end-to-end | **No — not a unit test.** Requires live DID resolution + the running PDS. | §4 live probe (manual / Chunk I). |

The Rust test proves **"trust is keyed on signature, not on audience alone"** (the security property). The end-to-end "does prod trust the real entryway" is a §4 live check, because it depends on (a) entryway publishing a DID with an `#atproto` key and (b) `PDS_ENTRYWAY_DID` being set — neither of which a unit test can stand in for.

---

## 1. Files

- **Read (rsky, deployed branch only):**
  - `git show divinevideo/main:rsky-pds/src/xrpc_server/auth.rs` — `verify_jwt` (the signature-verifying core, lines ~44–123).
  - `git show divinevideo/main:rsky-pds/src/account_manager/helpers/auth.rs` — `create_service_jwt` (lines 161–204) + `ServiceJwtParams` (lines 57–66). This is the token-minting helper the test reuses.
  - `git show divinevideo/main:rsky-pds/src/auth_verifier.rs` — `verify_service_jwt` (~907–960), `ModService` (525–587), audience pins.
- **Create (rsky — DONE IN A LATER rsky-session, not now):** `rsky-pds/tests/entryway_token_trust.rs`
- **Verify (IAC, read-only):** `/Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/rsky-pds/{base/deployment.yaml,overlays/production/kustomization.yaml,base/external-secret.yaml}`

---

## 2. TDD: the test file (`rsky-pds/tests/entryway_token_trust.rs`)

The test exercises **Path 2** (`xrpc_server::auth::verify_jwt`) directly, because that is the real entryway-trust path and it is a pure async function taking a `get_signing_key` closure — no Postgres container, no Rocket, no network. Mint tokens with `create_service_jwt` (the same code path entryway-style service JWTs use), then verify with the matching vs. a mismatched key.

### Test design (what each test asserts)

`verify_jwt(jwt_str, own_did, get_signing_key)` returns `Ok(ServiceJwtPayload)` on a good signature+audience and `Err` otherwise. The `get_signing_key` closure stands in for DID resolution: in prod it resolves `iss`'s DID doc and returns the `did:key`-encoded `#atproto` key. In the test we hand it the key we want trust anchored to.

`rsky_crypto::verify::verify_signature` expects the signing key as a **`did:key` multibase string** (that is what `get_did_key_from_multibase` produces in prod). Mint the token with a `secp256k1` keypair, then encode that keypair's **public** key to `did:key` for the closure. (Confirm the exact encoder while writing — `rsky_crypto`/`rsky_identity` expose a `did:key` formatter for secp256k1 public keys; if the helper name differs, grep `rsky-crypto/src` and `rsky-identity/src` for `did:key` and `multibase` on `divinevideo/main`.)

### Step 2.1 — Write the failing test: entryway-signed token is ACCEPTED

- [ ] Create `rsky-pds/tests/entryway_token_trust.rs` with a test `entryway_signed_service_jwt_is_accepted`:
  - Generate a `secp256k1::Keypair` (the "entryway" signing key) via a fixed secret hex so the test is deterministic, e.g. reuse the test repo-key hex from `tests/oauth_metadata_test.rs` (`4f3edf983ac636a65a842ce7c78d9aa706d3b113bce036f4aeb4f7f7a5c5f3cf`) **as the entryway key** for the happy path.
  - Mint a service JWT with `create_service_jwt(ServiceJwtParams { iss: "did:web:entryway.divine.video".into(), aud: "did:web:pds.divine.video".into(), keypair: <entryway_secret_key>, exp: None, lxm: None })`.
  - Build `get_signing_key = |iss, _force_refresh| Ok(did_key_for(&entryway_pubkey))` returning the `did:key` of the **same** keypair's public half.
  - Call `verify_jwt(jwt, Some("did:web:pds.divine.video".into()), get_signing_key).await`.
  - Assert `Ok`, and `payload.iss == "did:web:entryway.divine.video"`, `payload.aud == "did:web:pds.divine.video"`.
- [ ] It "fails" first only in the sense of not-yet-compiling/not-yet-existing. Run it (Step 2.4) to confirm the harness wiring before adding the rejection case.

### Step 2.1b — Write the test: ENTRYWAY-AUDIENCE token is ACCEPTED (the named case)

This is the literal headline of Chunk D and must be proven explicitly. **Do not just swap the `aud` string in 2.1** — that would *fail* with `BadJwtAudience`, because `verify_jwt(jwt, Some("did:web:pds.divine.video"), …)` rejects any `aud != pds`. Entryway-audience admission lives **only** in `ModService` (lines 525–587), and **only** when `PDS_ENTRYWAY_DID` is set. Reproduce that decomposition at the unit level (no Rocket/Postgres):

- [ ] Add `entryway_audience_service_jwt_is_accepted`:
  - Mint with the **entryway** key (the accepted-case key from 2.1) and `aud: "did:web:entryway.divine.video"`.
  - Call `verify_jwt(jwt, None, get_signing_key).await` — **`own_did = None`** is exactly how `ModService::from_request` invokes it (`ServiceJwtOpts { aud: None, … }`, which makes the inner audience check a no-op so the *guard* can apply its own pds-OR-entryway rule).
  - Assert `Ok`, and `payload.aud == "did:web:entryway.divine.video"`. This proves the signature path accepts an entryway-aud token once the key resolves.
- [ ] Add `entryway_audience_admitted_by_modservice_predicate`: replicate `ModService`'s exact admission boolean (lines 549–552) as a small helper and assert both arms:
  - With `PDS_ENTRYWAY_DID = Some("did:web:entryway.divine.video")` and `payload.aud = "did:web:entryway.divine.video"` → **admitted** (predicate returns "accept").
  - With `PDS_ENTRYWAY_DID = None` (today's prod reality, §4.1) and the same `payload.aud` → **rejected** (`BadJwtAudience`).
  - Use `EnvVarGuard` (copy the struct from `tests/oauth_metadata_test.rs`, lines ~15–37) so the env mutation is scoped and restored.
  - This is §4.1's finding turned into a regression test: **the prod-faithful "entryway token ACCEPTED" outcome requires `PDS_ENTRYWAY_DID` to be set, and the test asserting rejection-when-unset is the standing proof that prod is currently missing it.** When Chunk B adds the env var, flip nothing — the test already documents the contract.

### Step 2.2 — Write the test: WRONG-KEY token is REJECTED

- [ ] Add `wrong_key_service_jwt_is_rejected`:
  - Mint the JWT with **`forged_keypair`** (a *different* secret hex, e.g. `8f2a55949068468ad5d670dfd0c0a33d5b9e7e1a2c0d2059f0f8f8779d4d078d`), keeping `iss`/`aud` identical to the accepted case.
  - Build `get_signing_key` returning the **entryway** public key's `did:key` (i.e. the trust anchor is entryway, but the token was signed by the forger).
  - Call `verify_jwt(jwt, Some("did:web:pds.divine.video".into()), get_signing_key).await`.
  - Assert `Err`, and that the error string contains `BadJwtSignature` (verify_jwt bails with `"BadJwtSignature: jwt signature does not match jwt issuer"` — `xrpc_server/auth.rs` line ~113).

### Step 2.3 — (audience guard sanity) wrong-audience token is REJECTED

- [ ] Add `wrong_audience_service_jwt_is_rejected`:
  - Mint with the correct **entryway** key but `aud: "did:web:someone-else.example"`.
  - Call `verify_jwt(jwt, Some("did:web:pds.divine.video".into()), get_signing_key)` (own_did = PDS).
  - Assert `Err` containing `BadJwtAudience` (line ~70). This locks the contract that PDS rejects tokens minted for a different audience even with a valid signature.

### Step 2.4 — Run

- [ ] From the rsky repo root:

```bash
cd /Users/rabble/code/divine/rsky && cargo test -p rsky-pds --test entryway_token_trust
```

Expected output (all five pass):

```
running 5 tests
test entryway_signed_service_jwt_is_accepted ... ok
test entryway_audience_service_jwt_is_accepted ... ok
test entryway_audience_admitted_by_modservice_predicate ... ok
test wrong_key_service_jwt_is_rejected ... ok
test wrong_audience_service_jwt_is_rejected ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; ...
```

- [ ] **SECURITY STOP (gate, not a nicety):** If `wrong_key_service_jwt_is_rejected` shows `FAILED` because `verify_jwt` returned `Ok` on the forged token — **halt the entire rollout.** A wrong-key-accepted result means PDS trust is keyed on audience alone and any party can forge a token with `aud=did:web:pds.divine.video`. Do not pin images, do not onboard creators. File a security finding referencing `xrpc_server/auth.rs` `verify_jwt` and stop. (This is the explicit gate from parent plan line 171 and Risks "Token-trust false-confidence.")

### Step 2.5 — Commit (in rsky, later session only)

```bash
git -C /Users/rabble/code/divine/rsky checkout -b test/entryway-token-trust divinevideo/main
git -C /Users/rabble/code/divine/rsky add rsky-pds/tests/entryway_token_trust.rs
git -C /Users/rabble/code/divine/rsky commit -m "test: prove pds rejects wrong-key service jwt (entryway trust)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

> Branch from `divinevideo/main`, never local `main`. Do NOT push or merge without the rsky maintainers — this is the cross-repo guardrail.

---

## 3. If `verify_jwt`'s closure signature makes a pure-unit test awkward

`verify_jwt` is `pub` in `xrpc_server::auth` and takes a plain `Fn(String, bool) -> Result<String>`, so the closure approach above needs no DB and no DID resolver — it is the cleanest target. **Prefer it.** Only if a future rsky refactor hides `verify_jwt` should you fall back to the heavier harness below.

- [ ] **Fallback (heavier):** model the test on `rsky-pds/tests/oauth_metadata_test.rs` — spin a Postgres `testcontainer`, set the env (`PDS_SERVICE_DID`, `PDS_ENTRYWAY_DID=did:web:entryway.divine.video`, the three key hexes), build a Rocket client via `build_rocket`, and drive a write XRPC route guarded by `ModService` with a stubbed `id_resolver`. This requires injecting a fake DID document for `did:web:entryway.divine.video` — heavier and slower. Avoid unless forced.

---

## 4. Confirm the DEPLOYED PDS env carries entryway's trust anchor (live / IAC check)

The Rust test proves the *property*; this confirms the *deployment* actually wires entryway into the trust set. **All read-only — no IAC edits in this chunk** (image pinning + any env additions are Chunk B).

### Step 4.1 — IAC: is `PDS_ENTRYWAY_DID` set?

- [ ] Run:

```bash
grep -rni 'ENTRYWAY' /Users/rabble/code/divine/divine-iac-coreconfig/k8s/applications/rsky-pds/
```

Expected **today**: no output. **Finding:** `PDS_ENTRYWAY_DID` is unset in prod, so the `ModService` entryway-audience branch is dead and no service JWT minted with `aud=did:web:entryway.divine.video` would be admitted. Record this in the launch checklist.

- [ ] Decide with the rsky/keycast owners whether the production OAuth/service flow actually requires PDS to accept `aud=entryway`. Two outcomes:
  - **It does** → a follow-up (Chunk B-adjacent) must add `PDS_ENTRYWAY_DID=did:web:entryway.divine.video` to `base/deployment.yaml` (or the prod overlay). Note it as a dependency; do not edit here.
  - **It does not** (PDS only ever sees `aud=did:web:pds.divine.video` service JWTs, and OAuth access tokens are out of scope until the `@TODO: Implement DPop/OAuth` lands) → record that the entryway-audience branch is intentionally inert and the relevant trust is PDS-audience service-JWT trust only.

### Step 4.2 — Live: does entryway publish a resolvable DID with an `#atproto` key?

- [ ] From a host with real egress (NOT the planning sandbox):

```bash
curl -fsS https://entryway.divine.video/.well-known/did.json | jq '{id, verificationMethod: [.verificationMethod[].id]}'
```

Expected: an `id` (the entryway DID) and a `verificationMethod` array containing an entry ending in `#atproto`. If entryway's DID is a `did:plc:...`, resolve via `curl -fsS https://plc.directory/<did> | jq` instead. **If there is no `#atproto` key, `get_verification_material` returns `None` and the PDS rejects every entryway token** — record as a blocker.

### Step 4.3 — Live: does the running PDS expose the protected-resource metadata pointing at entryway?

- [ ] (Cross-check with Chunk 0 / Chunk I; confirms the resource-server half of the contract is live.)

```bash
curl -fsS https://pds.divine.video/.well-known/oauth-protected-resource | jq '{resource, authorization_servers}'
```

Expected: `{"resource":"https://pds.divine.video","authorization_servers":["https://entryway.divine.video"]}`. A 404 means the deployed `latest` rsky-pds predates the endpoint → escalate to Chunk B (pin + redeploy) before relying on any of this.

### Step 4.4 — Record results

- [ ] Append the §4 outcomes (env-set y/n, entryway `#atproto` key y/n, protected-resource live y/n) to `/Users/rabble/code/divine/divine-sky/docs/runbooks/launch-checklist.md` so the team stops re-inferring.

---

## 5. Findings to surface back to the parent plan (do not silently "fix")

1. **The parent plan's "static entryway signing public key in PDS env" model does not match the code.** Trust flows through DID-document resolution of `iss` (`verify_service_jwt` → `xrpc_server::auth::verify_jwt`). There is no `PDS_ENTRYWAY_SIGNING_PUBLIC_KEY` env to verify; the right check is "entryway's DID doc carries an `#atproto` key" (§4.2) plus "`PDS_ENTRYWAY_DID` is set" (§4.1).
2. **`PDS_ENTRYWAY_DID` is unset in deployed IAC** → the `ModService` entryway-audience branch is currently dead in prod (§4.1).
3. **The OAuth/DPoP resource-server path is a `@TODO` on `divinevideo/main`** (`auth_verifier.rs:823`). The DPoP-bound OAuth *access tokens* keycast's `/api/atproto/oauth/token` issues are **not** verified by any PDS guard yet. The §2 test proves *service-JWT* trust, which is the real protocol link that exists; it does **not** prove OAuth access-token verification, because that code isn't written. The parent plan's Chunk I "token works against pds.divine.video" step must account for this gap (it likely exercises app-password/service-JWT auth, not OAuth access tokens, until the `@TODO` lands).
4. **Security gate restated:** §2.4 wrong-key-accepted = full rollout halt.

---

## 6. Done criteria

- [ ] `rsky-pds/tests/entryway_token_trust.rs` exists on a branch off `divinevideo/main` with the five tests in §2 (incl. 2.1b, the named `aud=entryway` accepted case + the `PDS_ENTRYWAY_DID`-gated admission predicate).
- [ ] `cargo test -p rsky-pds --test entryway_token_trust` → `5 passed; 0 failed`.
- [ ] Wrong-key case **rejects** (security gate green).
- [ ] §4 live/IAC results recorded in `launch-checklist.md`.
- [ ] §5 findings reported to the parent-plan owner (model mismatch, unset `PDS_ENTRYWAY_DID`, OAuth `@TODO`) so Chunk B / Chunk I can absorb them.
- [ ] No edits made to the `rsky` repo during *planning*; the test commit happens only in the dedicated rsky execution session.
