UPDATE publish_jobs
SET state = 'published'
WHERE state = 'ineligible';
