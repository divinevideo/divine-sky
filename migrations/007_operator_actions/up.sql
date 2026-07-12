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

ALTER TABLE operator_actions DROP CONSTRAINT IF EXISTS operator_actions_status_check;
ALTER TABLE operator_actions ADD CONSTRAINT operator_actions_status_check
    CHECK (status IN ('previewed', 'applied', 'rolled_back', 'rollback_partial', 'failed'));

CREATE INDEX IF NOT EXISTS idx_operator_actions_type_created
    ON operator_actions (action_type, created_at DESC);
