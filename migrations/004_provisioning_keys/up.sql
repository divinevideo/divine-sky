CREATE TABLE provisioning_keys (
    key_ref             TEXT PRIMARY KEY,
    key_purpose         TEXT NOT NULL,
    public_key_hex      TEXT NOT NULL,
    encrypted_secret    BYTEA NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
