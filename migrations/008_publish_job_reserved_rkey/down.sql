DROP TRIGGER IF EXISTS publish_job_writer_epoch_fence ON publish_jobs;
DROP FUNCTION IF EXISTS enforce_publish_job_writer_epoch();
ALTER TABLE publish_jobs DROP COLUMN IF EXISTS writer_epoch;
ALTER TABLE publish_jobs DROP COLUMN IF EXISTS prepared_record;
ALTER TABLE publish_jobs DROP COLUMN IF EXISTS reserved_rkey;
