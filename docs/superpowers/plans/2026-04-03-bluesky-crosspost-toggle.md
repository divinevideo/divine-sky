# Bluesky Crosspost Toggle Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow users to toggle Bluesky crossposting on/off, defaulting to on for new registrations.

**Architecture:** Three-repo change. divine-sky adds an `/enable` endpoint and changes the DB default. keycast (login.divine.video) adds a proxy endpoint and settings UI. divine-mobile (Flutter/BLoC) adds a settings toggle. This plan covers the divine-sky changes only; Tasks 4 and 5 provide prompts for the other repos.

**Tech Stack:** Rust, Axum, Diesel (raw SQL), PostgreSQL, mockito (tests)

**Spec:** `docs/superpowers/specs/2026-04-03-bluesky-crosspost-toggle-design.md`

---

## Chunk 1: divine-sky Backend Changes

### Task 1: Database Migration — Change Default

**Files:**
- Create: `migrations/005_crosspost_default_true/up.sql`
- Create: `migrations/005_crosspost_default_true/down.sql`

- [ ] **Step 1: Create migration directory and files**

```bash
mkdir -p migrations/005_crosspost_default_true
```

`migrations/005_crosspost_default_true/up.sql`:
```sql
ALTER TABLE account_links ALTER COLUMN crosspost_enabled SET DEFAULT TRUE;
```

`migrations/005_crosspost_default_true/down.sql`:
```sql
ALTER TABLE account_links ALTER COLUMN crosspost_enabled SET DEFAULT FALSE;
```

Note: The test infrastructure only runs migration 001, and all code paths pass `crosspost_enabled` explicitly via `upsert_pending_account_link`. This migration only affects direct SQL inserts that omit the column.

- [ ] **Step 2: Commit**

```bash
git add migrations/005_crosspost_default_true/
git commit -m "feat: change crosspost_enabled default to TRUE for new accounts"
```

---

### Task 2: Add `enable_account_link` Query and Store Method

**Files:**
- Modify: `crates/divine-bridge-db/src/queries.rs` (add after `disable_account_link` at line 211)
- Modify: `crates/divine-handle-gateway/src/store.rs` (add import + method after `disable` at line 84)

- [ ] **Step 1: Add `enable_account_link` query**

Add to `crates/divine-bridge-db/src/queries.rs` after the `disable_account_link` function (after line 211):

```rust
/// Re-enable a previously disabled account-link record.
///
/// Restores `provisioning_state` to `ready` if the account has a DID
/// (was previously provisioned), or `pending` if not.
pub fn enable_account_link(
    conn: &mut PgConnection,
    nostr_pubkey: &str,
) -> Result<AccountLinkLifecycleRow> {
    let result = sql_query(
        "UPDATE account_links
         SET crosspost_enabled = TRUE,
             provisioning_state = CASE
                 WHEN did IS NOT NULL THEN 'ready'
                 ELSE 'pending'
             END,
             disabled_at = NULL,
             updated_at = NOW()
         WHERE nostr_pubkey = $1
         RETURNING nostr_pubkey, did, handle, crosspost_enabled, signing_key_id,
                   plc_rotation_key_ref, provisioning_state, provisioning_error, disabled_at,
                   created_at, updated_at",
    )
    .bind::<Text, _>(nostr_pubkey)
    .get_result::<AccountLinkLifecycleRow>(conn)?;
    Ok(result)
}
```

- [ ] **Step 2: Add import and `enable` method to `DbStore`**

In `crates/divine-handle-gateway/src/store.rs`, update the import on line 7:

```rust
use divine_bridge_db::{
    disable_account_link, enable_account_link, get_account_link_lifecycle,
    get_account_link_lifecycle_by_handle, mark_account_link_failed, mark_account_link_ready,
    upsert_pending_account_link,
};
```

Add after the `disable` method (after line 84):

```rust
    pub fn enable(&self, nostr_pubkey: &str) -> Result<Option<AccountLinkRecord>> {
        if self.get_by_pubkey(nostr_pubkey)?.is_none() {
            return Ok(None);
        }
        let mut connection = self.connection.lock().unwrap();
        let row = enable_account_link(&mut connection, nostr_pubkey)?;
        Ok(Some(AccountLinkRecord::from(row)))
    }
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo check -p divine-handle-gateway
```
Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add crates/divine-bridge-db/src/queries.rs crates/divine-handle-gateway/src/store.rs
git commit -m "feat: add enable_account_link query and store method"
```

---

### Task 3: Add `/enable` Route, `sync_enabled_state`, and Tests

**Files:**
- Create: `crates/divine-handle-gateway/src/routes/enable.rs`
- Modify: `crates/divine-handle-gateway/src/routes/mod.rs` (add `pub mod enable;`)
- Modify: `crates/divine-handle-gateway/src/lib.rs` (add route + `enable_by_pubkey_result` + `sync_enabled_state`)
- Modify: `crates/divine-handle-gateway/tests/control_plane.rs` (add tests)

- [ ] **Step 1: Write the failing test**

Add to `crates/divine-handle-gateway/tests/control_plane.rs`:

```rust
#[tokio::test]
#[serial]
async fn control_plane_enable_re_enables_disabled_account() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    // Stubs for provision and disable steps
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .expect_at_least(1)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .expect_at_least(1)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    // 1. Provision an account
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({
                        "nostr_pubkey": "npub1enable",
                        "handle": "enabletest.divine.video",
                        "did": "did:plc:enable123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // 2. Disable it
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/npub1enable/disable")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 3. Re-enable it
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/npub1enable/enable")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["crosspost_enabled"], true);
    assert_eq!(payload["provisioning_state"], "ready");
    assert!(payload["disabled_at"].is_null());
}

#[tokio::test]
#[serial]
async fn control_plane_enable_returns_404_for_unknown_pubkey() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/nonexistent/enable")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn control_plane_enable_is_idempotent() {
    let database_url = test_database_url();
    reset_database(&database_url);

    let mut name_server = mockito::Server::new_async().await;
    let _sync_stub = name_server
        .mock("POST", "/api/internal/username/set-atproto")
        .with_status(200)
        .expect_at_least(1)
        .create_async()
        .await;
    let _keycast_sync_stub = name_server
        .mock("POST", "/api/internal/atproto/state")
        .with_status(200)
        .expect_at_least(1)
        .create_async()
        .await;

    let app = build_app(database_url, name_server.url());

    // Provision an account (already crosspost_enabled = true)
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/provision")
                .header("content-type", "application/json")
                .header("authorization", AUTH_HEADER)
                .body(Body::from(
                    json!({
                        "nostr_pubkey": "npub1idem",
                        "handle": "idemtest.divine.video",
                        "did": "did:plc:idem123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Enable when already enabled — should return 200 (idempotent)
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/account-links/npub1idem/enable")
                .header("authorization", AUTH_HEADER)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["crosspost_enabled"], true);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p divine-handle-gateway control_plane_enable -- --nocapture
```
Expected: Compilation error — route/handler doesn't exist yet.

- [ ] **Step 3: Create the enable route handler**

Create `crates/divine-handle-gateway/src/routes/enable.rs`:

```rust
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use super::super::{AccountLinkRecord, AppState};

pub async fn handler(
    State(state): State<AppState>,
    Path(nostr_pubkey): Path<String>,
) -> Result<Json<AccountLinkRecord>, StatusCode> {
    let record = state
        .enable_by_pubkey_result(&nostr_pubkey)
        .map_err(|error| {
            tracing::error!(error = %error, "failed to enable account link");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let record = record.ok_or(StatusCode::NOT_FOUND)?;

    state
        .sync_enabled_state(&record)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "failed to sync enabled state");
            StatusCode::BAD_GATEWAY
        })?;

    Ok(Json(record))
}
```

Note: Error handling mirrors `disable.rs` — returns `BAD_GATEWAY` on downstream sync failure for consistency.

- [ ] **Step 4: Register the route module**

Add to `crates/divine-handle-gateway/src/routes/mod.rs`:
```rust
pub mod enable;
```

- [ ] **Step 5: Add `enable_by_pubkey_result` and `sync_enabled_state` to AppState**

In `crates/divine-handle-gateway/src/lib.rs`, add after `sync_disabled_state` (after line 204):

```rust
    pub(crate) fn enable_by_pubkey_result(
        &self,
        nostr_pubkey: &str,
    ) -> anyhow::Result<Option<AccountLinkRecord>> {
        self.store.enable(nostr_pubkey)
    }

    pub(crate) async fn sync_enabled_state(
        &self,
        record: &AccountLinkRecord,
    ) -> anyhow::Result<()> {
        // Only sync "ready" to keycast if the account has a DID.
        // If provisioning_state is "pending" (no DID), skip keycast sync
        // since there's nothing to activate yet.
        if let (Some(did), divine_bridge_db::models::ProvisioningState::Ready) =
            (&record.did, &record.provisioning_state)
        {
            self.keycast_client
                .sync_ready(&record.nostr_pubkey, did)
                .await?;
            self.name_server_client
                .sync_state_for_handle(&record.handle, Some(did.as_str()), "ready")
                .await?;
        }
        Ok(())
    }
```

This correctly handles both cases:
- Account has DID (was provisioned) → sync "ready" to keycast and name server
- Account has no DID (never provisioned) → skip sync, provisioning will pick it up

- [ ] **Step 6: Register the route in `app_with_state`**

In `crates/divine-handle-gateway/src/lib.rs`, add after the disable route (after line 231):

```rust
        .route(
            "/api/account-links/:nostr_pubkey/enable",
            post(routes::enable::handler),
        )
```

- [ ] **Step 7: Run all tests**

```bash
cargo test -p divine-handle-gateway -- --nocapture
```
Expected: All tests pass including the three new enable tests.

- [ ] **Step 8: Commit**

```bash
git add crates/divine-handle-gateway/src/routes/enable.rs \
       crates/divine-handle-gateway/src/routes/mod.rs \
       crates/divine-handle-gateway/src/lib.rs \
       crates/divine-handle-gateway/tests/control_plane.rs
git commit -m "feat: add POST /enable endpoint to re-enable crossposting"
```

---

### Task 3b: Accept Optional `crosspost_enabled` in Opt-In Request

This prevents a race window during registration when the user unchecks "Publish to Bluesky" — the account is created atomically with the correct `crosspost_enabled` value rather than creating with `true` then immediately calling `/disable`.

**Files:**
- Modify: `crates/divine-handle-gateway/src/routes/opt_in.rs`
- Modify: `crates/divine-handle-gateway/src/store.rs` (update `upsert_pending_opt_in` signature)
- Modify: `crates/divine-handle-gateway/src/lib.rs` (update `upsert_pending_result` call)

- [ ] **Step 1: Update OptInRequest to accept optional crosspost_enabled**

Replace the contents of `crates/divine-handle-gateway/src/routes/opt_in.rs`:

```rust
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use super::super::{AccountLinkRecord, AppState};

#[derive(Debug, Deserialize)]
pub struct OptInRequest {
    pub nostr_pubkey: String,
    pub handle: String,
    #[serde(default = "default_crosspost_enabled")]
    pub crosspost_enabled: bool,
}

fn default_crosspost_enabled() -> bool {
    true
}

pub async fn handler(
    State(state): State<AppState>,
    Json(payload): Json<OptInRequest>,
) -> Result<(StatusCode, Json<AccountLinkRecord>), StatusCode> {
    let record = state
        .upsert_pending_result(payload.nostr_pubkey, payload.handle, payload.crosspost_enabled)
        .map_err(|error| {
            tracing::error!(error = %error, "failed to persist pending opt-in");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    state.enqueue_provisioning(&record.nostr_pubkey, &record.handle);
    Ok((StatusCode::ACCEPTED, Json(record)))
}
```

- [ ] **Step 2: Update `upsert_pending_opt_in` in store.rs**

In `crates/divine-handle-gateway/src/store.rs`, update the method signature to accept `crosspost_enabled`:

```rust
    pub fn upsert_pending_opt_in(
        &self,
        nostr_pubkey: &str,
        handle: &str,
        crosspost_enabled: bool,
    ) -> Result<AccountLinkRecord> {
        let signing_key_id = format!("pending-signing:{nostr_pubkey}");
        let plc_rotation_key_ref = format!("pending-rotation:{nostr_pubkey}");
        let mut connection = self.connection.lock().unwrap();
        let row = upsert_pending_account_link(
            &mut connection,
            nostr_pubkey,
            handle,
            &signing_key_id,
            &plc_rotation_key_ref,
            crosspost_enabled,
        )?;
        Ok(AccountLinkRecord::from(row))
    }
```

- [ ] **Step 3: Update `upsert_pending_result` in lib.rs**

Find the `upsert_pending_result` method in `crates/divine-handle-gateway/src/lib.rs` and update it to pass through `crosspost_enabled`. It currently calls `self.store.upsert_pending_opt_in(nostr_pubkey, handle)` — add the third parameter.

Also find any other call sites of `upsert_pending_opt_in` (e.g., in `upsert_ready` around line 155-162 which calls it as a fallback) and pass `true` as the default for those paths.

- [ ] **Step 4: Run all tests**

```bash
cargo test -p divine-handle-gateway -- --nocapture
```
Expected: All existing tests still pass. The existing opt-in test sends `{ "nostr_pubkey", "handle" }` without `crosspost_enabled`, so the serde default `true` kicks in — same behavior as before.

- [ ] **Step 5: Commit**

```bash
git add crates/divine-handle-gateway/src/routes/opt_in.rs \
       crates/divine-handle-gateway/src/store.rs \
       crates/divine-handle-gateway/src/lib.rs
git commit -m "feat: accept optional crosspost_enabled in opt-in request"
```

---

## Chunk 2: Prompts for Other Repos

### Task 4: Keycast (login.divine.video) — Prompt for Coding Agent

This task is NOT implemented in divine-sky. Provide this prompt to a coding agent working in `/Users/rabble/code/divine/keycast`:

---

**PROMPT FOR KEYCAST AGENT:**

You are working in the keycast repo (`/Users/rabble/code/divine/keycast`), a Rust/Axum backend + SvelteKit frontend that powers login.divine.video.

**Goal:** Add a crosspost toggle so users can enable/disable Bluesky publishing for their DiVine videos.

**Context:**
- The handle-gateway backend (divine-sky repo) has these endpoints, authenticated with bearer token from env `KEYCAST_ATPROTO_TOKEN`:
  - `POST /api/account-links/:pubkey/enable` — re-enables crossposting, returns updated record
  - `POST /api/account-links/:pubkey/disable` — disables crossposting, returns updated record
  - `GET /api/account-links/:pubkey/status` — returns current state including `crosspost_enabled`, `provisioning_state`, `handle`, `did`
  - `POST /api/account-links/opt-in` — creates account link, now accepts optional `crosspost_enabled` field (defaults to `true`)
- Keycast already syncs state with handle-gateway. Check `api/src/api/http/atproto.rs` for existing ATProto account management patterns. The keycast client uses `POST /api/internal/atproto/state` for state sync.
- The keycast API authenticates users via OAuth sessions. Check existing patterns for how handlers validate that the authenticated user owns a given pubkey.

**Changes needed:**

1. **New API endpoint — `PUT /api/account/:pubkey/crosspost`**
   - Body: `{ "enabled": true }` or `{ "enabled": false }`
   - Authenticated via existing keycast OAuth session
   - Validate the authenticated user owns the pubkey (follow existing auth patterns)
   - If `enabled: true` → call handle-gateway `POST /api/account-links/:pubkey/enable`
   - If `enabled: false` → call handle-gateway `POST /api/account-links/:pubkey/disable`
   - Return the updated status from handle-gateway

2. **Registration flow update**
   - Find the handle claim / account creation flow in the SvelteKit frontend (`/keycast/keycast/`)
   - Add a checkbox: "Publish your videos to Bluesky" (checked by default)
   - When the form submits, include `crosspost_enabled` in the request body
   - The backend should pass `crosspost_enabled` through to handle-gateway's opt-in endpoint

3. **Settings page**
   - Find the existing user settings page in the SvelteKit frontend
   - Add a "Bluesky Publishing" section with:
     - Toggle switch for enable/disable
     - Display of current handle (`username.divine.video`)
     - Current status text (enabled/disabled/provisioning in progress)
   - Toggle calls `PUT /api/account/:pubkey/crosspost`

**Testing:** Follow existing test patterns in the keycast repo. Test the proxy endpoint with mocked handle-gateway responses.

---

### Task 5: divine-mobile (Flutter/BLoC) — Prompt for Coding Agent

This task is NOT implemented in divine-sky. Provide this prompt to a coding agent working on divine-mobile:

---

**PROMPT FOR DIVINE-MOBILE AGENT:**

You are working on divine-mobile, a Flutter app using BLoC for state management.

**Goal:** Add a settings toggle for Bluesky crossposting.

**Context:**
- The keycast backend (login.divine.video) exposes:
  - `PUT /api/account/:pubkey/crosspost` with body `{ "enabled": true|false }`, authenticated via OAuth session
  - The status endpoint returns `{ crosspost_enabled, provisioning_state, handle, did, ... }`
- The app already uses `@divinevideo/login` for OAuth authentication.
- The app uses BLoC (not Riverpod) for state management.

**Changes needed:**

1. **New Cubit — `CrosspostSettingsCubit`**
   - State class: `CrosspostSettingsState` with fields:
     - `enabled` (bool) — crosspost toggle state
     - `handle` (String?) — username.divine.video handle
     - `provisioningState` (String?) — pending/ready/failed/disabled
     - `isLoading` (bool)
     - `error` (String?)
   - Methods:
     - `loadStatus()` — fetch current status from keycast API on cubit creation
     - `toggleCrosspost(bool enabled)` — optimistic update: immediately emit new state with toggled value, call API, revert and show error on failure
   - Use the existing HTTP client / API service patterns in the app

2. **Settings UI**
   - Find the existing settings screen
   - Add a "Bluesky Publishing" section/card:
     - `SwitchListTile` or equivalent toggle: "Publish videos to Bluesky"
     - Subtitle: shows `username.divine.video` handle when provisioned
     - Disabled (greyed out) if provisioning not complete
     - On toggle → call `cubit.toggleCrosspost(!currentValue)`
     - Show error SnackBar on failure
   - Wrap with `BlocBuilder<CrosspostSettingsCubit, CrosspostSettingsState>`

3. **Registration flow** (if divine-mobile has its own handle claim UI)
   - Add checkbox "Publish your videos to Bluesky" (checked by default)
   - Pass the value through to keycast's registration endpoint

**Testing:**
- Unit test for `CrosspostSettingsCubit`: verify state transitions for load, toggle success, toggle failure with revert
- Widget test for settings toggle: verify it renders, responds to taps, shows loading/error states
- Use `bloc_test` package for cubit tests
