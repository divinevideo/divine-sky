# rsky-native Provisioning (Option B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use `- [ ]`. TDD throughout. Build env: libpq paths + `TEST_DATABASE_URL` (see AGENTS.md / prior runbooks).

**Goal:** Provision ATProto accounts the rsky-native way — let rsky-pds author the did:plc (with its own keys + a configured offline recovery key) so accounts are born ACTIVE, instead of the bridge pre-minting DIDs (which rsky's activateAccount rejects).

**Architecture:** Replace the bridge's `create_did` (PLC mint) + `activate` flow with a single `createAccount` call that supplies NO `did` and DOES supply `{handle, email, password, inviteCode, recoveryKey}`. rsky mints the PLC op with its rotation+signing keys (listing the recovery key first), returns `{did, accessJwt, refreshJwt}`, account active. Bridge stores DID + session (Wall 4 columns already exist). The bridge stops generating PLC/signing keys entirely.

**Tech stack:** Rust, divine-atbridge (provisioner/pds_accounts/config), reqwest, rsky-pds (`com.atproto.server.createAccount` + `createInviteCode`), Postgres.

**Verified contract (rsky `87151fc`):** no-DID createAccount path → `format_did_and_plc_op` (uses rsky's `PDS_PLC_ROTATION_KEY` + `PDS_REPO_SIGNING_KEY`, lists `input.recovery_key` first), `deactivated=false`, returns `CreateAccountOutput{did, accessJwt, refreshJwt}`. Requires `password` (non-admin), and `inviteCode` when `PDS_INVITE_REQUIRED=true` (it is, staging). Email optional-ish (validated if present; use a placeholder noreply address). `createInviteCode` (admin auth) mints a code.

---

## Task 1: Config — recovery key, email domain, invite toggle

**Files:** `crates/divine-atbridge/src/config.rs`

- [ ] **Step 1 (RED):** test that `from_env` reads `PLC_RECOVERY_ROTATION_DID_KEYS` (comma-sep), `ACCOUNT_EMAIL_DOMAIN` (default `divine.video`), and `PDS_INVITE_REQUIRED` (default true). Reuse the wip branch's `parse_recovery_rotation_did_keys` (validation: each starts `did:key:`, dedup, max 4) — cherry-pick its logic.
```rust
#[test]
fn parses_recovery_keys_and_email_domain() {
    std::env::set_var("PLC_RECOVERY_ROTATION_DID_KEYS","did:key:zRec1,did:key:zRec2");
    // ... set required vars ...
    let c = BridgeConfig::from_env().unwrap();
    assert_eq!(c.plc_recovery_rotation_did_keys, vec!["did:key:zRec1","did:key:zRec2"]);
    assert_eq!(c.account_email_domain, "divine.video");
}
```
- [ ] **Step 2:** add fields `plc_recovery_rotation_did_keys: Vec<String>`, `account_email_domain: String` to `BridgeConfig`; wire env reads + the `parse_recovery_rotation_did_keys` helper (port from `wip/found-recovery-rotation-key`). Update the 3 test struct-literals in config.rs/provision_api.rs.
- [ ] **Step 3:** `cargo test -p divine-atbridge --lib config` green. Commit.

## Task 2: PdsAccountCreator trait → rsky-native shape

**Files:** `crates/divine-atbridge/src/provisioner.rs` (trait + struct), `pds_accounts.rs` (impl), mocks.

- [ ] **Step 1 (RED):** new trait method returning the rsky-minted identity:
```rust
pub struct CreatedAccount { pub did: String, pub session: Option<PdsSession> }
#[async_trait]
pub trait PdsAccountCreator: Send + Sync {
    async fn create_account(&self, handle: &str, recovery_keys: &[String]) -> Result<CreatedAccount>;
}
```
Test (in pds_accounts.rs, mockito): createAccount called with NO `did`, WITH `handle`, a `password`, an `inviteCode`, `email`, and `recoveryKey`; mock returns `{did, accessJwt, refreshJwt}`; assert returned `CreatedAccount.did` + session.
- [ ] **Step 2 (GREEN):** implement `PdsAccountsClient::create_account`:
  1. `create_invite()` → POST `com.atproto.server.createInviteCode` `{useCount:1}` with Basic admin auth → code (skip if invites not required — but default require).
  2. POST `com.atproto.server.createAccount` (Basic admin auth) body `{handle, email: noreply+<handle>@<domain>, password: <generated 32-byte hex>, inviteCode, recoveryKey: recovery_keys.first()}` — NO `did`.
  3. parse `{did, accessJwt, refreshJwt}` (empty-body tolerant per existing helper), return `CreatedAccount{did, session: Some(...)}`.
  Add `create_invite_endpoint()`. Keep `confirm_existing_repo` for the 409 path (return its did, session None).
- [ ] **Step 3:** update both mocks (`provisioner.rs` MockPdsCreator, `provisioning_lifecycle.rs`) to the new signature, returning a `CreatedAccount` with a fake did+session.
- [ ] **Step 4:** `cargo test -p divine-atbridge --lib pds_accounts` green. Commit.

## Task 3: Provisioner orchestration — drop PLC mint + activate

**Files:** `crates/divine-atbridge/src/provisioner.rs`

- [ ] **Step 1 (RED):** rewrite `successful_provisioning_*` tests to assert: NO key generation, NO plc_client call (remove PlcClient from provisioner or leave unused for delete-path), `create_account(handle, recovery_keys)` called, DID + session stored, state `ready`. Add: recovery keys from config are passed through.
- [ ] **Step 2 (GREEN):** rewrite `create_new_link`:
  - drop `key_store.generate_keypair` calls and the whole PlcOperation/sign/`create_did` block.
  - `save_pending_link` (did None) — keep for lifecycle visibility, but signing_key_id/rotation_ref become `""` or a sentinel (rsky owns keys now). (Schema columns are NOT NULL — pass `"rsky-managed"`.)
  - `let created = self.pds_creator.create_account(handle, &self.recovery_rotation_did_keys).await?;`
  - persist session (`store_pds_session`) if present, `mark_link_ready(nostr_pubkey, &created.did)`.
  - on error → `mark_link_failed` with `{err:#}`.
  - add `recovery_rotation_did_keys: Vec<String>` to `AccountProvisioner`.
- [ ] **Step 3:** DO NOT DELETE the PLC code. Verified: `create_did` is called only by `create_new_link` (provision path), but `PlcClient`/`update_did`/`derive_did_plc`/`sign_plc_operation` are still referenced by `pds_host_backfill.rs` and their own unit tests. After the rewrite, `create_did` itself may become call-free in non-test code → guard with `#[cfg_attr(not(test), allow(dead_code))]` (or keep it `pub`) so `clippy -D warnings` stays green. Keep `plc_directory.rs`, `KeyStore`/`GeneratedKeyStore`, and the `PlcClient` generic on `AccountProvisioner` intact (don't churn the struct's type params). The delete/tombstone path uses `delete_record` (a repo write), NOT PLC — unaffected.
- [ ] **Step 4:** `cargo test -p divine-atbridge` green. Commit.

## Task 4: Wire config → provisioner construction

**Files:** `crates/divine-atbridge/src/health.rs` (both AccountProvisioner builds)

- [ ] **Step 1:** pass `recovery_rotation_did_keys: config.plc_recovery_rotation_did_keys.clone()` and `account_email_domain` into the `AccountProvisioner{...}` literals; drop `pds_endpoint` if no longer used by the provision path (keep if delete path needs it).
- [ ] **Step 2:** `cargo build -p divine-atbridge` clean; full `cargo test -p divine-atbridge -p divine-bridge-db` green; `clippy -D warnings`; `fmt`. Commit.

## Task 5: Verify live on staging

- [ ] **Step 1:** build image via Cloud Build from this branch's merge SHA; bump staging overlay; ArgoCD sync (see `docs/runbooks/deploy-atbridge-plc-fix.md`).
- [ ] **Step 2:** set staging `PLC_RECOVERY_ROTATION_DID_KEYS` secret to a real cold recovery `did:key` (private half offline — generated by infra owner). Reset the test account row, opt in fresh, provision.
- [ ] **Step 3:** confirm `provisioning_state=ready`, account `active:true` on the PDS (`listRepos`), DID doc lists the recovery key FIRST in `rotationKeys` and rsky's key second (`https://plc.directory/<did>/data`). Post a Nostr video → confirm it appears AND plays on Bluesky.

---

## Self-Review
- Recovery key (decision): Task 1 (config) + Task 2 (passed to createAccount) + Task 5 step 3 (verified in DID doc). ✓
- rsky-native (no bridge mint): Task 3 drops the mint/activate. ✓
- Invite/password/email contract: Task 2 step 2. ✓
- Session storage (Wall 4): reused via `store_pds_session`. ✓
- Cost/scale: no per-user secrets introduced (keys are rsky's shared + 1 recovery). ✓

## Risks
- **Delete/tombstone path** may still depend on `PlcClient`/`create_did`/keys — Task 3 step 3 must check `runtime.rs` before deleting code, or it breaks deletes. Verify, don't assume.
- **Schema NOT NULL** on `signing_key_id`/`plc_rotation_key_ref` — use a sentinel (`"rsky-managed"`), don't loosen the schema in this change.
- **Recovery key private half** must be cold/offline (not in the cluster's SM) or it's not a real recovery path — infra-owner task, Task 5 step 2.
- **Few test users / resettable** (per user): safe to wipe `account_links` + the 33 PDS repos in staging while iterating.
