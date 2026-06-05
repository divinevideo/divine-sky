# Bluesky Crosspost Toggle — Design Spec

**Date:** 2026-04-03
**Status:** Approved

## Goal

Users can control whether their DiVine videos are published to Bluesky. The toggle is **on by default** for new registrations and can be disabled in settings (divine-mobile or login.divine.video). Existing accounts are unaffected — they can opt in through settings.

## Architecture

Three repos, one data path:

```
divine-mobile (Flutter/BLoC)  ──┐
                                 ├──▶  keycast (login.divine.video)  ──▶  handle-gateway  ──▶  account_links DB
login.divine.video (SvelteKit) ─┘          (auth + proxy)                (internal API)
```

Clients authenticate to keycast via OAuth session. Keycast validates user ownership of the pubkey, then calls handle-gateway with the internal bearer token (`KEYCAST_ATPROTO_TOKEN`). The bridge (`divine-atbridge`) reads `crosspost_enabled` from the DB to gate publishing.

## Changes by Repo

### 1. divine-sky (handle-gateway)

**Database migration (005):**
```sql
ALTER TABLE account_links ALTER COLUMN crosspost_enabled SET DEFAULT TRUE;
```
Only affects new rows. Existing accounts stay as-is. Note: `NewAccountLink` struct passes `crosspost_enabled` explicitly, so the column default only matters for direct SQL inserts.

**New DB query — `enable_account_link` in `divine-bridge-db/src/queries.rs`:**
- Inverse of `disable_account_link`
- Sets `crosspost_enabled = TRUE`, clears `disabled_at`
- Restores `provisioning_state`: to `'ready'` if DID exists, to `'pending'` if DID is NULL (needs provisioning)
- Returns the updated row

**New store method — `enable` in `store.rs`:**
- Calls `enable_account_link` query
- Calls `sync_enabled_state` to notify keycast and name server (symmetric with `sync_disabled_state` on disable)

**New endpoint — `POST /api/account-links/:pubkey/enable`:**
- Re-enables crossposting for an existing account link
- Delegates to the `enable` store method
- Idempotent: returns 200 with current state if already enabled (matches `/disable` behavior)
- Returns 404 if no account link exists
- Protected by existing `require_internal_auth` middleware

**Modified endpoint — `POST /api/account-links/opt-in`:**
- Accept optional `crosspost_enabled` field in request body (defaults to `true`)
- Allows atomic creation with crosspost disabled, avoiding a race window where the bridge could publish before a follow-up `/disable` call lands

**Existing endpoints (no changes needed):**
- `POST /api/account-links/:pubkey/disable` — disables crossposting
- `GET /api/account-links/:pubkey/status` — returns current state

**No bridge changes** — `divine-atbridge` already gates on `crosspost_enabled && provisioning_state == "ready"`. The enable endpoint must clear `disabled_at` or the bridge's `row.disabled_at.is_some()` check will still block publishing.

### 2. keycast (login.divine.video)

**New proxy endpoint — `PUT /api/account/:pubkey/crosspost`:**
- Body: `{ "enabled": true | false }`
- Authenticated via existing keycast OAuth session
- Validates the authenticated user owns the pubkey
- Forwards to handle-gateway `/enable` or `/disable` with bearer token
- Returns the updated status

**Registration flow update:**
- Add checkbox to SvelteKit handle claim form: "Publish your videos to Bluesky" (checked by default)
- Pass the `crosspost_enabled` value in the opt-in request body (atomic, no race window)
- If checked (default): `crosspost_enabled: true`
- If unchecked: `crosspost_enabled: false`

**Settings page addition:**
- New "Bluesky Publishing" section in user settings
- Shows: current status (enabled/disabled), handle (`username.divine.video`), provisioning state
- Toggle switch to enable/disable
- Calls `PUT /api/account/:pubkey/crosspost`

### 3. divine-mobile (Flutter/BLoC)

**New BLoC — `CrosspostSettingsCubit`:**
- State: `CrosspostSettingsState { enabled: bool, handle: String?, loading: bool, error: String? }`
- Fetches current status on load via keycast API
- `toggleCrosspost(bool enabled)` — calls keycast `PUT /api/account/:pubkey/crosspost`
- Optimistic update: flips UI immediately, reverts on error with toast

**Settings screen:**
- "Bluesky Publishing" section with toggle switch
- Subtitle shows `username.divine.video` handle when enabled
- Disabled state if account not yet provisioned

**Registration flow (if divine-mobile has its own handle claim UI):**
- Checkbox "Publish your videos to Bluesky" (checked by default)
- Passes preference through keycast registration call

## Edge Cases

- **Mid-pipeline toggle off**: Video already in bridge pipeline may still publish. Best-effort, not real-time kill switch.
- **Provisioning not ready**: Bridge already gates on `provisioning_state = ready`. Events skip until provisioning completes.
- **Re-enable after disable**: `/enable` flips `crosspost_enabled`, clears `disabled_at`, restores `provisioning_state` to `ready` (if DID exists) or `pending` (if not yet provisioned). No re-provisioning for already-provisioned accounts.
- **Network failure in mobile**: Optimistic toggle reverts on error, shows toast.
- **No backfill**: Enabling crossposting only publishes new videos going forward, not retroactive.
- **Existing accounts**: Unaffected by default change. Must explicitly enable via settings.

## Testing

- **handle-gateway**: Unit test for `/enable` endpoint; update existing `bridge_opt_in_gate` tests to verify new default
- **keycast**: Integration test for proxy endpoint with session auth
- **divine-mobile**: Widget test for settings toggle; BLoC unit test for state transitions
