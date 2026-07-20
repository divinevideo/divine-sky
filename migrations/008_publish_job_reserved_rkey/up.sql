ALTER TABLE publish_jobs
    ADD COLUMN IF NOT EXISTS reserved_rkey TEXT,
    ADD COLUMN IF NOT EXISTS prepared_record JSONB,
    ADD COLUMN IF NOT EXISTS writer_epoch INTEGER NOT NULL DEFAULT 1;

-- Epoch 2 is the first create-only writer.  A legacy worker does not set this
-- transaction-local marker, so it cannot claim epoch-2 work even if it overlaps
-- the rollout.  Existing epoch-1 jobs remain quarantined for explicit audit.
CREATE OR REPLACE FUNCTION enforce_publish_job_writer_epoch()
RETURNS trigger AS $$
BEGIN
    IF NEW.state = 'in_progress'
       AND (OLD.state IS DISTINCT FROM 'in_progress'
            OR NEW.lease_owner IS DISTINCT FROM OLD.lease_owner)
       AND current_setting('divine_sky.writer_epoch', true)
           IS DISTINCT FROM NEW.writer_epoch::TEXT THEN
        RAISE EXCEPTION 'publish job writer epoch % is not active', NEW.writer_epoch;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS publish_job_writer_epoch_fence ON publish_jobs;
CREATE TRIGGER publish_job_writer_epoch_fence
    BEFORE UPDATE ON publish_jobs
    FOR EACH ROW EXECUTE FUNCTION enforce_publish_job_writer_epoch();
