UPDATE publish_jobs
SET state = 'published',
    error = NULL
WHERE state = 'ineligible';
