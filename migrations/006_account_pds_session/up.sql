-- Per-account PDS session tokens for repo writes (Wall 4).
-- rsky-pds requires repo writes to be authed as the account's own DID, so the
-- bridge must store the accessJwt/refreshJwt it receives at provisioning time
-- and use/refresh them when publishing. Idempotent (safe to re-run).
ALTER TABLE account_links
    ADD COLUMN IF NOT EXISTS pds_access_jwt TEXT,
    ADD COLUMN IF NOT EXISTS pds_refresh_jwt TEXT,
    ADD COLUMN IF NOT EXISTS pds_session_updated_at TIMESTAMPTZ;
