ALTER TABLE account_links
    ADD COLUMN IF NOT EXISTS publish_backfill_state TEXT NOT NULL DEFAULT 'not_started',
    ADD COLUMN IF NOT EXISTS publish_backfill_started_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS publish_backfill_completed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS publish_backfill_error TEXT;

CREATE INDEX IF NOT EXISTS idx_account_links_backfill_scan
    ON account_links (publish_backfill_state, created_at ASC)
    WHERE crosspost_enabled = TRUE AND provisioning_state = 'ready';

ALTER TABLE publish_jobs
    ADD COLUMN IF NOT EXISTS nostr_pubkey TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS event_created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN IF NOT EXISTS event_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS job_source TEXT NOT NULL DEFAULT 'live',
    ADD COLUMN IF NOT EXISTS lease_owner TEXT,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS completed_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_publish_jobs_live_claim
    ON publish_jobs (state, created_at ASC)
    WHERE job_source = 'live';

CREATE INDEX IF NOT EXISTS idx_publish_jobs_backfill_claim
    ON publish_jobs (state, event_created_at ASC)
    WHERE job_source = 'backfill';

CREATE INDEX IF NOT EXISTS idx_publish_jobs_lease_expiry
    ON publish_jobs (lease_expires_at);
