CREATE TABLE IF NOT EXISTS operator_actions (
    operation_id TEXT PRIMARY KEY,
    action_type TEXT NOT NULL,
    actor TEXT NOT NULL,
    scope JSONB NOT NULL,
    dry_run BOOLEAN NOT NULL,
    confirmation_digest TEXT NOT NULL,
    before_images JSONB NOT NULL DEFAULT '[]'::jsonb,
    matched_count BIGINT NOT NULL DEFAULT 0,
    changed_count BIGINT NOT NULL DEFAULT 0,
    applied_count BIGINT NOT NULL DEFAULT 0,
    applied_at TIMESTAMPTZ,
    rollback_restored_count BIGINT NOT NULL DEFAULT 0,
    rollback_skipped_count BIGINT NOT NULL DEFAULT 0,
    rollback_at TIMESTAMPTZ,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT operator_actions_type_check
        CHECK (action_type IN ('repair_legacy_badjwt')),
    CONSTRAINT operator_actions_status_check
        CHECK (status IN ('previewed', 'applied', 'rolled_back', 'rollback_partial', 'failed')),
    CONSTRAINT operator_actions_operation_id_check
        CHECK (operation_id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$')
);

ALTER TABLE operator_actions
    ADD COLUMN IF NOT EXISTS applied_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS applied_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS rollback_restored_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS rollback_skipped_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS rollback_at TIMESTAMPTZ;

UPDATE operator_actions
SET applied_count = matched_count,
    applied_at = CASE WHEN status = 'applied' THEN updated_at ELSE applied_at END
WHERE status IN ('applied', 'rolled_back', 'rollback_partial')
  AND applied_count = 0;

CREATE INDEX IF NOT EXISTS idx_operator_actions_type_created
    ON operator_actions (action_type, created_at DESC);
