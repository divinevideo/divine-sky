DROP INDEX IF EXISTS idx_publish_jobs_lease_expiry;
DROP INDEX IF EXISTS idx_publish_jobs_backfill_claim;
DROP INDEX IF EXISTS idx_publish_jobs_live_claim;
DROP INDEX IF EXISTS idx_account_links_backfill_scan;

ALTER TABLE publish_jobs
    DROP COLUMN IF EXISTS completed_at,
    DROP COLUMN IF EXISTS lease_expires_at,
    DROP COLUMN IF EXISTS lease_owner,
    DROP COLUMN IF EXISTS job_source,
    DROP COLUMN IF EXISTS event_payload,
    DROP COLUMN IF EXISTS event_created_at,
    DROP COLUMN IF EXISTS nostr_pubkey;

ALTER TABLE account_links
    DROP COLUMN IF EXISTS publish_backfill_error,
    DROP COLUMN IF EXISTS publish_backfill_completed_at,
    DROP COLUMN IF EXISTS publish_backfill_started_at,
    DROP COLUMN IF EXISTS publish_backfill_state;
