ALTER TABLE account_links
    ADD COLUMN publish_backfill_state TEXT NOT NULL DEFAULT 'not_started',
    ADD COLUMN publish_backfill_started_at TIMESTAMPTZ,
    ADD COLUMN publish_backfill_completed_at TIMESTAMPTZ,
    ADD COLUMN publish_backfill_error TEXT;

CREATE INDEX idx_account_links_backfill_scan
    ON account_links (publish_backfill_state, created_at ASC)
    WHERE crosspost_enabled = TRUE AND provisioning_state = 'ready';

ALTER TABLE publish_jobs
    ADD COLUMN nostr_pubkey TEXT NOT NULL DEFAULT '',
    ADD COLUMN event_created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN event_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN job_source TEXT NOT NULL DEFAULT 'live',
    ADD COLUMN lease_owner TEXT,
    ADD COLUMN lease_expires_at TIMESTAMPTZ,
    ADD COLUMN completed_at TIMESTAMPTZ;

CREATE INDEX idx_publish_jobs_live_claim
    ON publish_jobs (state, created_at ASC)
    WHERE job_source = 'live';

CREATE INDEX idx_publish_jobs_backfill_claim
    ON publish_jobs (state, event_created_at ASC)
    WHERE job_source = 'backfill';

CREATE INDEX idx_publish_jobs_lease_expiry
    ON publish_jobs (lease_expires_at);
