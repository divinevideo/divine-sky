# Bridge per-account publish auth (Wall 4) — Design Spec

**Date:** 2026-06-05
**Status:** DECIDED 2026-06-05 → **Option A (per-account session).** Option B (service JWT) is **impossible**, verified against rsky source: `create_record` uses `AccessStandardIncludeChecks` → `access_check` → `validate_access_token` → `validate_bearer_token`, which only accepts a **user session** token (`credentials.did = Some`). The service-JWT path (`UserDidAuth`/`verify_service_jwt`) sets `credentials.did = None` and is wired only to specific endpoints (mod/labeler) with `aud` checks — it is never consulted by `createRecord`, and the `did != credentials.did.unwrap()` gate (`create_record.rs:50`) requires a real `did`. So repo writes MUST use a per-account session. Implementing Option A.

## Problem (evidence-backed)

The bridge publishes every account's records (`com.atproto.repo.createRecord`, `uploadBlob`, `putRecord`, `deleteRecord`) using a **single shared Bearer token** — `config.pds_auth_token` (`PDS_AUTH_TOKEN`), passed once to `PdsClient::new(pds_url, pds_auth_token)` in `runtime.rs:478/487/495`.

This cannot work. rsky-pds enforces **per-DID auth** on repo writes:

- `rsky-pds/src/apis/com/atproto/repo/create_record.rs:50-51`:
  ```rust
  let did = account.did;
  if did != auth.access.credentials.unwrap().did.unwrap() {
      bail!("AuthRequiredError")
  }
  ```
  The authenticated DID **must equal** the repo being written to. One shared token can write to at most one DID's repo (its own), not every crossposting account's.
- Same file, line 47: `if account.deactivated_at.is_some() { bail!("Account is deactivated") }` — independently re-confirms Wall 3 (the account must be activated first; handled by PR #13).

Compounding: staging `PDS_AUTH_TOKEN` is literally `placeholder-token` — not a valid credential at all.

**Consequence:** even with provisioning fixed (PRs #12, #13), the publish step will fail `AuthRequiredError` for every account. No crosspost can land until the bridge authenticates **as each account**.

## What we already have

The Wall 3 fix (PR #13) makes `createAccount` return — and the bridge receive — an `accessJwt` + `refreshJwt` **for the account** at provisioning time. That session is exactly the credential repo writes require. Today it's used once (to activate) and discarded.

## Options

### Option A — Persist and use the per-account session (recommended)
- At provisioning, store the account's `accessJwt`/`refreshJwt` (e.g. a `pds_sessions` table keyed by DID, or columns on `account_links`).
- The publisher selects the session for the target DID and sends `Authorization: Bearer <that account's accessJwt>`.
- Refresh via `com.atproto.server.refreshSession` when the access token expires (401 → refresh → retry once); persist the rotated tokens.
- For accounts provisioned before this lands (the existing repos), mint a session via admin (`com.atproto.admin`/service auth) or `createSession` — needs the account password, which the bridge does not currently set/store (see open questions).

**Pros:** matches the ATProto model exactly; each write is correctly scoped. **Cons:** session storage + refresh lifecycle; backfill for existing accounts.

### Option B — Service-auth (inter-service JWT) signed by the bridge
- If rsky-pds trusts a service JWT (the `entryway`/service-DID mechanism the video path uses — cf. `memory: rsky-pds-video-upload-blocker`, where getServiceAuth with `aud=PDS service DID`, `lxm=...` worked), the bridge could mint a short-lived service JWT scoped per request instead of storing sessions.
- **Unknown:** whether rsky's `AccessStandardIncludeChecks` verifier accepts a service JWT as the repo DID for `createRecord`. Needs verification against `auth_verifier.rs`.

**Pros:** no session storage. **Cons:** depends on rsky accepting service JWTs for repo writes — must be confirmed, not assumed.

### Option C — Admin/superuser write bypass
- Some PDS impls allow an admin token to write any repo. rsky's `create_record` uses `AccessStandardIncludeChecks` (not admin), and the explicit `did != credentials.did` check suggests **no** such bypass exists. Likely a dead end; record and move on unless the verifier says otherwise.

## Recommendation

**Option A**, unless verifying `auth_verifier.rs` shows Option B (service JWT) is accepted for repo writes — in which case B is less state to manage. The decision hinges on one question answerable from rsky source: *what credential types does `AccessStandardIncludeChecks` accept, and does any satisfy `credentials.did == <arbitrary repo did>`?*

## Open questions for the maintainer
1. Does the bridge set an account **password** at `createAccount`? (Today it sends only `{did, handle}` — no password. `createSession`/refresh may need one. If not, sessions must come from the `createAccount` response only, and there's no recovery path if they're lost → leans toward storing refreshJwt durably.)
2. Is there an existing service-auth path (entryway/service DID) rsky-pds will accept for third-party repo writes? (If yes → Option B.)
3. How should the ~33 already-provisioned staging repos (and any prod ones) be backfilled with sessions?

## Implementation progress (branch `fix/atbridge-per-account-publish-auth`)
- ✅ **Incr 1** — migration 006: `pds_access_jwt`/`pds_refresh_jwt`/`pds_session_updated_at` on `account_links` (idempotent, in startup runner).
- ✅ **Incr 2** — provisioner persists the session: `create_account -> Option<PdsSession>`, `AccountLinkStore::store_pds_session` + `store_account_pds_session` query. Test: `successful_provisioning_persists_pds_session`.
- ✅ **Incr 3** — publish path authenticates per-account: `SessionProvider` trait, `PdsClient::with_session_provider` + `auth_token_for(did)` (falls back to shared token), `DbSessionProvider` resolves the access JWT by DID, wired in runtime. Test: `create_record_uses_per_account_session_token`.
- ✅ **Incr 4** — refresh on 401: `post_repo_write_as` calls `com.atproto.server.refreshSession` with the stored refresh JWT, persists the rotation (`store_session`), and retries once. Test: `create_record_refreshes_session_and_retries_on_401`.
- ✅ **Blob path** — `uploadBlob` authenticates per-account (`upload_blob_for_did` + `BlobUploader::upload_blob_for_user`); the **video-service path** `getServiceAuth` calls `auth_token_for(user_did)`. Both blob client and publisher share one `DbSessionProvider`. Test: `upload_blob_for_user_uses_per_account_session_token`.
- ✅ **putRecord / deleteRecord** — routed through `post_repo_write_as`, so all repo writes (create/put/delete/uploadBlob) auth per-account with 401-refresh. Test: `delete_record_uses_per_account_session_token`.
- ⏳ **Incr 5 (only remaining)** — backfill sessions for the ~33 pre-existing repos (no stored session → can't publish; needs admin/createSession or re-provision). Shared-token fallback does NOT help them (rsky rejects per-DID). Best done AFTER a live deploy confirms the new-account path.

## Acceptance criteria
- A crosspost `createRecord` for account X authenticates as DID X and succeeds against rsky-pds (no `AuthRequiredError`).
- Access-token expiry is handled (refresh + retry) without dropping the publish.
- Verified end-to-end: a real Nostr video from a `ready`+`crosspost_enabled` account appears AND plays on Bluesky.

## Not doing yet / why
Not implementing blind: this changes the publish hot path and the data model (session storage), and the right shape depends on the maintainer answers + one more rsky `auth_verifier.rs` read. The bug and the constraint are now precisely characterized; implementation should follow the decision.
