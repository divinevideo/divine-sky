ALTER TABLE account_links
    DROP COLUMN IF EXISTS pds_access_jwt,
    DROP COLUMN IF EXISTS pds_refresh_jwt,
    DROP COLUMN IF EXISTS pds_session_updated_at;
