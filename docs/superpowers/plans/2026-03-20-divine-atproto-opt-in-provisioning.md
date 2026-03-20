# DiVine ATProto Opt-In Provisioning Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep `username.divine.video` + NIP-05 automatic, make ATProto/Bluesky opt-in, and automatically provision `did:plc` + PDS + `/.well-known/atproto-did` after consent so mirroring starts only when the account is truly ready.

**Architecture:** `keycast` (`login.divine.video`) becomes the canonical ATProto consent and lifecycle owner. `divine-sky` provisions `did:plc`, creates the PDS account, persists durable bridge state, and pushes public handle-read-model updates to `divine-name-server`. `divine-router` remains read-only at the edge and serves `/.well-known/atproto-did` from Fastly KV only when the linked user is active, opted in, and `ready`.

**Tech Stack:** Rust (`axum`, `sqlx`, `reqwest`, `tokio`), PostgreSQL, Cloudflare Workers + D1 + Fastly KV, Fastly Compute, Flutter/Dart, existing `divine-atbridge` / `divine-handle-gateway` / `keycast_api` / `profile_repository`.

---

## Repo Boundaries

- `../keycast`
  - Owns authenticated user consent and ATProto lifecycle state.
  - Already claims `username.divine.video` from `update_profile()` in `api/src/api/http/auth.rs`.
- `../divine-name-server`
  - Owns canonical username registry and Fastly KV publication.
  - Must expose a service-authenticated way to update ATProto read-model fields.
- `../divine-router`
  - Owns wildcard `*.divine.video` edge routing.
  - Must remain read-only and serve `/.well-known/atproto-did` from Fastly KV.
- `/Users/rabble/code/divine/divine-sky`
  - Owns ATProto provisioning, bridge state, and mirroring.
  - Must provision `did:plc`, create PDS accounts, and gate publishing on consent + readiness.
- `../divine-mobile/mobile`
  - Owns the actual user opt-in UX for “Publish to Bluesky/ATProto”.
- `../divine-web`
  - Owns the web signup/settings surfaces.
  - Must keep ATProto UI behind a web feature flag until rollout is explicitly enabled.

## Canonical State Model

- Username claim:
  - NIP-05 goes live immediately.
  - `atproto_enabled = false`
  - `atproto_state = null`
- User opts in:
  - `atproto_enabled = true`
  - `atproto_state = "pending"`
- Provisioning succeeds:
  - `atproto_did = "did:plc:..."`
  - `atproto_state = "ready"`
- Provisioning fails:
  - `atproto_state = "failed"`
  - `atproto_error = "..."`
- User disables:
  - `atproto_enabled = false`
  - `atproto_state = "disabled"`
  - router stops resolving `/.well-known/atproto-did`
  - bridge stops mirroring new content

## Client Rollout Policy

- Backend support can ship before client exposure.
- `divine-mobile` must gate ATProto opt-in UX behind a dedicated mobile feature flag with a default of `false`.
- `divine-web` must gate ATProto opt-in UX behind a dedicated web feature flag with a default of `false`.
- Flipping the client flags should reveal already-built UI; it must not change backend semantics.
- QA and local debugging can use feature-flag overrides without enabling the feature globally.

## Chunk 1: Keycast Control Plane

### Task 1: Persist ATProto Opt-In State In Keycast

**Files:**
- Create: `../keycast/database/migrations/0009_add_atproto_link_state.sql`
- Modify: `../keycast/core/src/repositories/user.rs`
- Modify: `../keycast/api/src/api/http/auth.rs`
- Test: `../keycast/api/tests/atproto_opt_in_state_test.rs`

- [ ] **Step 1: Write the failing repository test**

```rust
#[tokio::test]
async fn user_atproto_state_round_trips() {
    let repo = test_user_repo().await;
    repo.set_atproto_state("pubkey1", 1, true, Some("pending"), None, None)
        .await
        .unwrap();

    let state = repo.get_atproto_state("pubkey1", 1).await.unwrap().unwrap();
    assert!(state.enabled);
    assert_eq!(state.state.as_deref(), Some("pending"));
    assert_eq!(state.did, None);
}
```

- [ ] **Step 2: Run the new test to confirm the schema/repository is missing**

Run: `cd ../keycast && cargo test -p keycast_api --test atproto_opt_in_state -- --nocapture`

Expected: FAIL with missing columns or missing repository methods.

- [ ] **Step 3: Add the migration and repository methods**

```sql
ALTER TABLE users
  ADD COLUMN atproto_enabled boolean NOT NULL DEFAULT false,
  ADD COLUMN atproto_state text DEFAULT NULL,
  ADD COLUMN atproto_did text DEFAULT NULL,
  ADD COLUMN atproto_error text DEFAULT NULL,
  ADD COLUMN atproto_updated_at timestamptz DEFAULT NULL;
```

```rust
pub async fn set_atproto_state(
    &self,
    pubkey: &str,
    tenant_id: i64,
    enabled: bool,
    state: Option<&str>,
    did: Option<&str>,
    error: Option<&str>,
) -> Result<(), RepositoryError> { /* sqlx update */ }
```

- [ ] **Step 4: Re-run the focused test**

Run: `cd ../keycast && cargo test -p keycast_api --test atproto_opt_in_state -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../keycast
git add database/migrations/0009_add_atproto_link_state.sql core/src/repositories/user.rs api/src/api/http/auth.rs api/tests/atproto_opt_in_state_test.rs
git commit -m "feat: persist atproto opt-in lifecycle in keycast"
```

### Task 2: Add Keycast User Endpoints For Enable / Status / Disable

**Files:**
- Create: `../keycast/api/src/api/http/atproto.rs`
- Modify: `../keycast/api/src/api/http/routes.rs`
- Modify: `../keycast/api/openapi.yaml`
- Create: `../keycast/api/src/atproto_provisioning.rs`
- Test: `../keycast/api/tests/atproto_http_test.rs`

- [ ] **Step 1: Write failing HTTP tests**

```rust
#[tokio::test]
async fn enable_sets_pending_and_returns_accepted() {
    let app = test_app().await;
    let response = app.post_json("/api/user/atproto/enable", json!({"username":"alice"})).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.json()["state"], "pending");
}
```

```rust
#[tokio::test]
async fn disable_marks_disabled() {
    let app = test_app_with_ready_atproto().await;
    let response = app.post_json("/api/user/atproto/disable", json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json()["state"], "disabled");
}
```

- [ ] **Step 2: Run the focused test binary**

Run: `cd ../keycast && cargo test -p keycast_api --test atproto_http_test -- --nocapture`

Expected: FAIL because the routes do not exist.

- [ ] **Step 3: Implement the user-facing ATProto endpoints**

```rust
POST /api/user/atproto/enable
GET  /api/user/atproto/status
POST /api/user/atproto/disable
```

Rules:
- `enable` requires the user already has a claimed username in Keycast.
- `enable` sets `enabled = true`, `state = pending`, clears stale error, then triggers provisioning.
- `status` returns `enabled`, `state`, `did`, `error`, `username`.
- `disable` sets `enabled = false`, `state = disabled`, and triggers downstream disable cleanup.

- [ ] **Step 4: Trigger divine-sky from the enable/disable handlers**

```rust
atproto_provisioning::request_enable(user_pubkey, username).await?;
atproto_provisioning::request_disable(user_pubkey).await?;
```

Use a dedicated service client module, not inline `reqwest` in handlers.

- [ ] **Step 5: Re-run the focused tests**

Run: `cd ../keycast && cargo test -p keycast_api --test atproto_http_test -- --nocapture`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd ../keycast
git add api/src/api/http/atproto.rs api/src/api/http/routes.rs api/src/atproto_provisioning.rs api/openapi.yaml api/tests/atproto_http_test.rs
git commit -m "feat: add keycast atproto opt-in control plane"
```

### Task 3: Hook Username Claim Flow To Pending ATProto State Only After Consent

**Files:**
- Modify: `../keycast/api/src/api/http/auth.rs`
- Modify: `../keycast/api/src/divine_names.rs`
- Test: `../keycast/api/tests/update_profile_divine_name_test.rs`

- [ ] **Step 1: Write a failing test showing username claim stays NIP-05-only by default**

```rust
#[tokio::test]
async fn update_profile_claims_name_without_enabling_atproto() {
    let app = test_app().await;
    let response = app.patch_json("/api/user/profile", json!({"username":"alice"})).await;
    assert_eq!(response.status(), StatusCode::OK);

    let state = load_user_atproto_state("alice_pubkey").await;
    assert_eq!(state.enabled, false);
    assert_eq!(state.state, None);
}
```

- [ ] **Step 2: Run the focused test**

Run: `cd ../keycast && cargo test -p keycast_api update_profile_claims_name_without_enabling_atproto -- --nocapture`

Expected: FAIL if the handler mutates ATProto state too early.

- [ ] **Step 3: Keep username claim and ATProto enable separate**

`update_profile()` in `api/src/api/http/auth.rs` should:
- claim/validate username with `divine-name-server`
- update local `users.username`
- not trigger ATProto enable
- return enough data for the client to know NIP-05 succeeded

`divine_names.rs` should stay responsible for NIP-05 claim / availability only.

- [ ] **Step 4: Re-run the focused test**

Run: `cd ../keycast && cargo test -p keycast_api update_profile_claims_name_without_enabling_atproto -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../keycast
git add api/src/api/http/auth.rs api/src/divine_names.rs api/tests/update_profile_divine_name_test.rs
git commit -m "refactor: keep username claim separate from atproto opt-in"
```

## Chunk 2: Public Handle Resolution Read Model

### Task 4: Add A Service-Authenticated ATProto Sync Endpoint To Divine Name Server

**Files:**
- Create: `../divine-name-server/src/routes/internal-atproto.ts`
- Modify: `../divine-name-server/src/index.ts`
- Modify: `../divine-name-server/src/utils/fastly-sync.ts`
- Test: `../divine-name-server/src/routes/internal-atproto.test.ts`
- Docs: `../divine-name-server/README.md`

- [ ] **Step 1: Write the failing service-auth tests**

```ts
it('updates atproto fields for an active username when bearer token is valid', async () => {
  const res = await app.request('/api/internal/username/set-atproto', {
    method: 'POST',
    headers: { Authorization: 'Bearer test-sync-token' },
    body: JSON.stringify({ name: 'alice', atproto_did: 'did:plc:abc', atproto_state: 'ready' }),
  })
  expect(res.status).toBe(200)
})
```

- [ ] **Step 2: Run the focused test file**

Run: `cd ../divine-name-server && npm run test:once -- src/routes/internal-atproto.test.ts`

Expected: FAIL because the route does not exist.

- [ ] **Step 3: Implement the internal sync route**

```ts
POST /api/internal/username/set-atproto
Authorization: Bearer ${ATPROTO_SYNC_TOKEN}
```

Rules:
- only trusted services call this route
- accepts `name`, `atproto_did`, `atproto_state`
- updates D1 row
- syncs Fastly KV immediately for active usernames
- keeps existing admin `set-atproto` as repair/backfill only

- [ ] **Step 4: Re-run the focused test**

Run: `cd ../divine-name-server && npm run test:once -- src/routes/internal-atproto.test.ts`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../divine-name-server
git add src/routes/internal-atproto.ts src/index.ts src/utils/fastly-sync.ts src/routes/internal-atproto.test.ts README.md
git commit -m "feat: add service-authenticated atproto sync endpoint"
```

### Task 5: Align Reserved Service Names Across Name Server And Router

**Files:**
- Create: `../divine-name-server/migrations/0008_reserve_atproto_service_subdomains.sql`
- Modify: `../divine-name-server/src/utils/subdomain.ts`
- Modify: `../divine-router/src/main.rs`
- Test: `../divine-name-server/src/utils/validation.test.ts`
- Test: `../divine-router/src/main.rs`

- [ ] **Step 1: Write the failing reservation test**

```ts
it('rejects login as a claimable username', async () => {
  const reserved = await isReservedWord(db, 'login')
  expect(reserved).toBe(true)
})
```

- [ ] **Step 2: Run the focused test**

Run: `cd ../divine-name-server && npm run test:once -- src/utils/validation.test.ts`

Expected: FAIL because `login` / `pds` / `feed` / `labeler` are not reserved everywhere.

- [ ] **Step 3: Add the missing reserved words and subdomain exclusions**

Reserve at least:
- `names`
- `www`
- `login`
- `pds`
- `feed`
- `labeler`
- `relay`
- `media`

Update both:
- D1 reserved words
- `SERVICE_SUBDOMAINS` in `src/utils/subdomain.ts`
- `SYSTEM_SUBDOMAINS` in `../divine-router/src/main.rs`

- [ ] **Step 4: Re-run both focused suites**

Run: `cd ../divine-name-server && npm run test:once -- src/utils/validation.test.ts`

Run: `cd ../divine-router && cargo test test_classify_host_new_system_subdomains -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../divine-name-server
git add migrations/0008_reserve_atproto_service_subdomains.sql src/utils/subdomain.ts src/utils/validation.test.ts
git commit -m "fix: reserve atproto service subdomains in name server"

cd ../divine-router
git add src/main.rs
git commit -m "fix: align router system subdomains with name server"
```

### Task 6: Keep Router Read-Only And Harden `/.well-known/atproto-did`

**Files:**
- Modify: `../divine-router/src/main.rs`
- Modify: `../divine-router/README.md`
- Test: `../divine-router/src/main.rs`

- [ ] **Step 1: Add failing route-level tests instead of field-only tests**

```rust
#[test]
fn atproto_did_returns_not_found_when_state_is_pending() {
    let user = make_atproto_user("abc123pubkey", "did:plc:abc123", "pending");
    let response = handle_atproto_did_for(Some(user));
    assert_eq!(response.get_status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run the focused router test**

Run: `cd ../divine-router && cargo test atproto_did_returns_not_found_when_state_is_pending -- --nocapture`

Expected: FAIL until the helper/route tests exercise actual response behavior.

- [ ] **Step 3: Refactor to pure helper + route wrapper**

```rust
fn build_atproto_did_response(user_data: Option<&UsernameData>) -> Response { /* ... */ }
```

Keep router behavior read-only:
- never invent DID state
- only read KV payload
- return `200 text/plain` bare DID when `active + ready + did present`
- return `404` otherwise

- [ ] **Step 4: Re-run the full router suite**

Run: `cd ../divine-router && cargo test`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../divine-router
git add src/main.rs README.md
git commit -m "test: harden router atproto did responses"
```

## Chunk 3: Divine-Sky Provisioning And Publish Gating

### Task 7: Replace The In-Memory Handle Gateway With A Real Provisioning API

**Files:**
- Modify: `crates/divine-handle-gateway/src/lib.rs`
- Create: `crates/divine-handle-gateway/src/store.rs`
- Create: `crates/divine-handle-gateway/src/provision_runner.rs`
- Create: `crates/divine-handle-gateway/src/name_server_client.rs`
- Modify: `crates/divine-handle-gateway/src/routes/opt_in.rs`
- Modify: `crates/divine-handle-gateway/src/routes/status.rs`
- Modify: `crates/divine-handle-gateway/src/routes/disable.rs`
- Test: `crates/divine-handle-gateway/tests/control_plane.rs`
- Test: `crates/divine-handle-gateway/tests/provision_flow.rs`

- [ ] **Step 1: Write the failing integration test against the real DB-backed API**

```rust
#[tokio::test]
async fn opt_in_persists_pending_link_and_spawns_provisioning() {
    let app = test_db_backed_gateway().await;
    let response = app.post_json("/api/account-links/opt-in", json!({
        "nostr_pubkey": "npub1alice",
        "handle": "alice.divine.video"
    })).await;

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(load_link("npub1alice").await.provisioning_state, "pending");
}
```

- [ ] **Step 2: Run the focused tests**

Run: `cargo test -p divine-handle-gateway provision_flow -- --nocapture`

Expected: FAIL because the crate is still in-memory.

- [ ] **Step 3: Replace `Arc<Mutex<HashMap<...>>>` with DB-backed storage**

Use `divine-bridge-db` lifecycle queries instead of in-memory records.

Add a signed/internal auth layer for trusted callers:
- `KEYCAST_ATPROTO_TOKEN`
- reject unauthenticated callers

- [ ] **Step 4: Trigger async provisioning from opt-in**

`opt-in` should:
- upsert pending link
- set `crosspost_enabled = true`
- spawn or enqueue provisioning runner
- return `202 accepted`

`disable` should:
- set `crosspost_enabled = false`
- mark `disabled`
- call name-server sync client with `disabled`

- [ ] **Step 5: Re-run the gateway tests**

Run: `cargo test -p divine-handle-gateway -- --nocapture`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/divine-handle-gateway
git commit -m "feat: make handle gateway db-backed and async"
```

### Task 8: Implement Real PLC Directory + PDS Account Clients

**Files:**
- Create: `crates/divine-atbridge/src/plc_directory.rs`
- Create: `crates/divine-atbridge/src/pds_accounts.rs`
- Modify: `crates/divine-atbridge/src/provisioner.rs`
- Modify: `crates/divine-atbridge/src/lib.rs`
- Test: `crates/divine-atbridge/tests/provisioning_lifecycle.rs`
- Docs: `docs/runbooks/login-divine-video.md`

- [ ] **Step 1: Write the failing provisioning integration test**

```rust
#[tokio::test]
async fn provision_account_creates_plc_did_and_pds_account() {
    let provisioner = test_provisioner_with_http_mocks();
    let result = provisioner.provision_account("npub1alice", "alice.divine.video").await.unwrap();
    assert!(result.did.starts_with("did:plc:"));
}
```

- [ ] **Step 2: Run the focused test**

Run: `cargo test -p divine-atbridge provisioning_lifecycle -- --nocapture`

Expected: FAIL because the production PLC/PDS clients do not exist yet.

- [ ] **Step 3: Implement the HTTP clients**

`plc_directory.rs`
- POST signed PLC operations to PLC directory

`pds_accounts.rs`
- create or recover PDS accounts for the minted DID + handle

Keep:
- retries idempotent
- failures persisted as `failed`
- signing key and rotation key distinct

- [ ] **Step 4: Re-run the provisioning suite**

Run: `cargo test -p divine-atbridge provisioning_lifecycle -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/src/plc_directory.rs crates/divine-atbridge/src/pds_accounts.rs crates/divine-atbridge/src/provisioner.rs crates/divine-atbridge/tests/provisioning_lifecycle.rs docs/runbooks/login-divine-video.md
git commit -m "feat: add real plc and pds provisioning clients"
```

### Task 9: Gate Mirroring On Explicit Opt-In And Ready State

**Files:**
- Modify: `crates/divine-atbridge/src/runtime.rs`
- Modify: `crates/divine-atbridge/src/pipeline.rs`
- Test: `crates/divine-atbridge/tests/bridge_opt_in_gate.rs`
- Test: `crates/divine-atbridge/tests/publish_path_integration.rs`

- [ ] **Step 1: Write the failing publish-gate test**

```rust
#[tokio::test]
async fn ready_but_not_crosspost_enabled_is_skipped() {
    let account = AccountLink {
        nostr_pubkey: "abc".into(),
        did: "did:plc:abc".into(),
        opted_in: false,
    };
    let result = process_video_for(account).await;
    assert!(matches!(result, ProcessResult::Skipped { .. }));
}
```

- [ ] **Step 2: Run the focused test**

Run: `cargo test -p divine-atbridge bridge_opt_in_gate -- --nocapture`

Expected: FAIL because `runtime.rs` currently maps `ready` to `opted_in = true`.

- [ ] **Step 3: Fix the gate**

Change:

```rust
opted_in: row.crosspost_enabled || row.provisioning_state == "ready",
```

to:

```rust
opted_in: row.crosspost_enabled && row.provisioning_state == "ready",
```

Also keep the existing `disabled_at.is_some()` guard.

- [ ] **Step 4: Re-run focused + integration tests**

Run: `cargo test -p divine-atbridge bridge_opt_in_gate -- --nocapture`

Run: `cargo test -p divine-atbridge publish_path_integration -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/divine-atbridge/src/runtime.rs crates/divine-atbridge/src/pipeline.rs crates/divine-atbridge/tests/bridge_opt_in_gate.rs crates/divine-atbridge/tests/publish_path_integration.rs
git commit -m "fix: gate bridge publishing on opt-in and ready state"
```

### Task 10: Push Public Read-Model Updates Back To Name Server

**Files:**
- Modify: `crates/divine-handle-gateway/src/name_server_client.rs`
- Modify: `crates/divine-handle-gateway/src/provision_runner.rs`
- Test: `crates/divine-handle-gateway/tests/provision_flow.rs`
- Docs: `docs/runbooks/launch-checklist.md`

- [ ] **Step 1: Write the failing sync callback test**

```rust
#[tokio::test]
async fn successful_provision_syncs_ready_state_to_name_server() {
    let client = test_name_server_client();
    let state = ProvisionResult { did: "did:plc:abc".into(), handle: "alice.divine.video".into(), signing_key_id: "k1".into() };
    client.sync_ready("alice.divine.video", &state.did).await.unwrap();
    assert_eq!(mock_name_server.last_state(), "ready");
}
```

- [ ] **Step 2: Run the focused test**

Run: `cargo test -p divine-handle-gateway provision_flow -- --nocapture`

Expected: FAIL before the callback client exists.

- [ ] **Step 3: Implement callback rules**

On success:
- sync `atproto_did`
- sync `atproto_state = ready`

On failure:
- sync `atproto_state = failed`
- sync `atproto_did = null`

On disable:
- sync `atproto_state = disabled`
- sync `atproto_did = null`

- [ ] **Step 4: Re-run the focused gateway tests**

Run: `cargo test -p divine-handle-gateway provision_flow -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/divine-handle-gateway/src/name_server_client.rs crates/divine-handle-gateway/src/provision_runner.rs crates/divine-handle-gateway/tests/provision_flow.rs docs/runbooks/launch-checklist.md
git commit -m "feat: sync atproto readiness back to name server"
```

## Chunk 4: Client Feature-Flagged Opt-In And Verification

### Task 11: Add The Mobile Opt-In Toggle And Status Polling Behind A Feature Flag

**Files:**
- Modify: `../divine-mobile/mobile/lib/features/feature_flags/models/feature_flag.dart`
- Modify: `../divine-mobile/mobile/lib/features/feature_flags/providers/feature_flag_providers.dart`
- Modify: `../divine-mobile/mobile/lib/blocs/profile_editor/profile_editor_state.dart`
- Modify: `../divine-mobile/mobile/lib/blocs/profile_editor/profile_editor_bloc.dart`
- Modify: `../divine-mobile/mobile/lib/screens/profile_setup_screen.dart`
- Modify: `../divine-mobile/mobile/packages/profile_repository/lib/src/profile_repository.dart`
- Create: `../divine-mobile/mobile/packages/profile_repository/lib/src/atproto_status.dart`
- Test: `../divine-mobile/mobile/test/core/feature_flag_test.dart`
- Test: `../divine-mobile/mobile/test/blocs/profile_editor/profile_editor_bloc_test.dart`
- Test: `../divine-mobile/mobile/test/screens/profile_setup_screen_test.dart`
- Test: `../divine-mobile/mobile/packages/profile_repository/test/src/profile_repository_test.dart`

- [ ] **Step 1: Write the failing client tests**

```dart
test('opt-in toggle calls enable endpoint after successful username claim', () async {
  // save profile -> claim username -> enable ATProto
});
```

```dart
testWidgets('shows pending and ready ATProto states in profile setup', (tester) async {
  // toggle on -> pending chip -> ready chip
});
```

```dart
testWidgets('hides ATProto controls when the mobile feature flag is disabled', (tester) async {
  // render profile setup with default flags and verify no Bluesky/ATProto toggle is shown
});
```

- [ ] **Step 2: Run the focused Flutter tests**

Run: `cd ../divine-mobile/mobile && flutter test test/blocs/profile_editor/profile_editor_bloc_test.dart`

Run: `cd ../divine-mobile/mobile && flutter test test/screens/profile_setup_screen_test.dart`

Expected: FAIL because no ATProto toggle/state exists.

- [ ] **Step 3: Extend repository + BLoC**

Add repository methods:

```dart
Future<void> enableAtproto({required String username});
Future<void> disableAtproto();
Future<AtprotoStatus> getAtprotoStatus();
```

Add BLoC/UI state:
- toggle value
- pending indicator
- ready indicator
- failure message with retry

Rules:
- add `FeatureFlag.atprotoPublishing` with default `false`
- hide the entire ATProto control surface unless the flag is enabled
- claim username first
- call opt-in endpoint second
- do not block username claim on provisioning completion
- show pending until backend reports `ready`

- [ ] **Step 4: Re-run focused tests**

Run: `cd ../divine-mobile/mobile && flutter test test/blocs/profile_editor/profile_editor_bloc_test.dart`

Run: `cd ../divine-mobile/mobile && flutter test test/screens/profile_setup_screen_test.dart`

Run: `cd ../divine-mobile/mobile && flutter test packages/profile_repository/test/src/profile_repository_test.dart`

Run: `cd ../divine-mobile/mobile && flutter test test/core/feature_flag_test.dart`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../divine-mobile/mobile
git add lib/features/feature_flags/models/feature_flag.dart lib/features/feature_flags/providers/feature_flag_providers.dart lib/blocs/profile_editor/profile_editor_state.dart lib/blocs/profile_editor/profile_editor_bloc.dart lib/screens/profile_setup_screen.dart packages/profile_repository/lib/src/profile_repository.dart packages/profile_repository/lib/src/atproto_status.dart test/core/feature_flag_test.dart test/blocs/profile_editor/profile_editor_bloc_test.dart test/screens/profile_setup_screen_test.dart packages/profile_repository/test/src/profile_repository_test.dart
git commit -m "feat: add mobile atproto opt-in feature flag"
```

### Task 12: Add Web ATProto Feature Flag Plumbing Before Exposing Any Web UX

**Files:**
- Modify: `../divine-web/src/config/api.ts`
- Modify: `../divine-web/src/components/auth/KeycastSignupDialog.tsx`
- Modify: `../divine-web/src/pages/LinkedAccountsSettingsPage.tsx`
- Create: `../divine-web/src/components/auth/KeycastSignupDialog.test.tsx`

- [ ] **Step 1: Write failing web tests**

```tsx
it('does not render ATProto CTA in signup when the feature flag is off', () => {
  // render signup dialog with default config and assert no Bluesky/ATProto copy is present
});
```

```tsx
it('renders ATProto entry points when the feature flag override is enabled', () => {
  // set localStorage override and verify the gated CTA appears
});
```

- [ ] **Step 2: Run the focused web tests**

Run: `cd ../divine-web && npm test -- --run src/components/auth/KeycastSignupDialog.test.tsx`

Expected: FAIL because no ATProto web flag/gating exists.

- [ ] **Step 3: Add web flag plumbing**

Add a web feature flag in `src/config/api.ts`:

```ts
features: {
  useFunnelcake: true,
  debugApi: false,
  useVerificationService: true,
  enableAtprotoPublishing: false,
}
```

Rules:
- use the existing `getFeatureFlag()` and localStorage override path
- hide any ATProto signup/settings UI unless `enableAtprotoPublishing` is true
- keep backend API calls unreachable from default web UI while the flag is off

- [ ] **Step 4: Re-run the focused web tests**

Run: `cd ../divine-web && npm test -- --run src/components/auth/KeycastSignupDialog.test.tsx`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ../divine-web
git add src/config/api.ts src/components/auth/KeycastSignupDialog.tsx src/pages/LinkedAccountsSettingsPage.tsx src/components/auth/KeycastSignupDialog.test.tsx
git commit -m "feat: gate web atproto opt-in behind feature flag"
```

### Task 13: Verify The Whole Flow End To End And Update Runbooks

**Files:**
- Modify: `docs/runbooks/dev-bootstrap.md`
- Modify: `docs/runbooks/login-divine-video.md`
- Modify: `docs/runbooks/launch-checklist.md`
- Create: `docs/runbooks/atproto-opt-in-smoke-test.md`

- [ ] **Step 1: Write the smoke test checklist**

Document this exact happy path:
1. create/login user in Keycast
2. claim `username.divine.video`
3. verify `/.well-known/nostr.json`
4. enable ATProto
5. verify `pending`
6. verify `did:plc` + `ready`
7. verify `https://username.divine.video/.well-known/atproto-did`
8. publish a Nostr video
9. verify mirrored ATProto post exists
10. disable and verify future mirroring stops

- [ ] **Step 2: Run repo-local verification suites**

Run: `cd ../keycast && cargo test -p keycast_api -- --nocapture`

Run: `cd ../divine-name-server && npm run test:once`

Run: `cd ../divine-router && cargo test`

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && bash scripts/test-workspace.sh`

Run: `cd ../divine-mobile/mobile && flutter test test/blocs/profile_editor/profile_editor_bloc_test.dart test/screens/profile_setup_screen_test.dart`

Run: `cd ../divine-web && npm test -- --run src/components/auth/KeycastSignupDialog.test.tsx`

Expected: PASS, excluding any pre-existing unrelated test failures that are documented and triaged immediately.

- [ ] **Step 3: Update the runbooks**

Capture:
- ATProto is opt-in, not automatic
- `did:plc` is the user identity
- `/.well-known/atproto-did` only resolves after `ready`
- router is read-only
- name-server is public read model
- keycast is consent/lifecycle owner
- bridge only publishes when `crosspost_enabled && ready`

- [ ] **Step 4: Commit**

```bash
git add docs/runbooks/dev-bootstrap.md docs/runbooks/login-divine-video.md docs/runbooks/launch-checklist.md docs/runbooks/atproto-opt-in-smoke-test.md
git commit -m "docs: add atproto opt-in rollout runbooks"
```

## Execution Order

1. `keycast` state model and endpoints
2. `divine-name-server` internal sync + reserved names
3. `divine-router` hardening
4. `divine-sky` handle-gateway + provisioning clients + publish gating
5. `divine-mobile` feature-flagged opt-in UI
6. `divine-web` feature-flagged web gating
7. end-to-end verification and rollout docs

## Non-Goals

- Do not switch users to `did:web`.
- Do not make ATProto auto-enable on username claim.
- Do not let `divine-router` or `divine-name-server` mint DIDs.
- Do not publish mirrored content before the account is `ready`.
- Do not expose ATProto UI in `divine-mobile` or `divine-web` unless the feature flag is enabled.

## Risks To Watch

- `keycast` and `divine-name-server` drifting into separate sources of truth.
- Publishing while `ready` but `crosspost_enabled = false`.
- Reserved-name mismatches causing handles that edge will never honor.
- Service-to-service auth for name-server sync being too weak or too ad hoc.
- PLC/PDS failures leaving users stuck in `pending` without retry surfaces.
- Mobile/web feature flags drifting from backend rollout readiness.

Plan complete and saved to `docs/superpowers/plans/2026-03-20-divine-atproto-opt-in-provisioning.md`. Ready to execute?
