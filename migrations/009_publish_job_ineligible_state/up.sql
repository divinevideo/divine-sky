-- Separate pipeline no-ops from jobs that wrote an ATProto record.
--
-- Historical `published` rows without a record mapping include unsupported,
-- invalid, pre-opt-in, or otherwise ineligible publish events. They also include
-- kind-5 delete-execution rows, which never produce a crosspost record for
-- their own event id. The mobile client queries video event ids, so treating
-- both groups as ineligible preserves the existing public status.
UPDATE publish_jobs
SET state = 'ineligible'
WHERE state = 'published'
  AND NOT EXISTS (
    SELECT 1
    FROM record_mappings
    WHERE record_mappings.nostr_event_id = publish_jobs.nostr_event_id
  );
