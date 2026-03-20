CREATE TABLE account_links (
    nostr_pubkey    TEXT PRIMARY KEY,
    did             TEXT UNIQUE NOT NULL,
    handle          TEXT UNIQUE NOT NULL,
    crosspost_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    signing_key_id  TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE ingest_offsets (
    source_name     TEXT PRIMARY KEY,
    last_event_id   TEXT NOT NULL,
    last_created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE asset_manifest (
    source_sha256   TEXT PRIMARY KEY,
    blossom_url     TEXT,
    at_blob_cid     TEXT NOT NULL,
    mime            TEXT NOT NULL,
    bytes           BIGINT NOT NULL,
    is_derivative   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE record_mappings (
    nostr_event_id  TEXT PRIMARY KEY,
    did             TEXT NOT NULL REFERENCES account_links(did),
    collection      TEXT NOT NULL,
    rkey            TEXT NOT NULL,
    at_uri          TEXT NOT NULL,
    cid             TEXT,
    status          TEXT NOT NULL DEFAULT 'published',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_record_mappings_at_uri ON record_mappings(at_uri);

CREATE TABLE moderation_actions (
    id              BIGSERIAL PRIMARY KEY,
    subject_type    TEXT NOT NULL,
    subject_id      TEXT NOT NULL,
    action          TEXT NOT NULL,
    origin          TEXT NOT NULL,
    reason          TEXT,
    state           TEXT NOT NULL DEFAULT 'pending',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE publish_jobs (
    nostr_event_id  TEXT PRIMARY KEY,
    attempt         INT NOT NULL DEFAULT 0,
    state           TEXT NOT NULL DEFAULT 'pending',
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_publish_jobs_state ON publish_jobs(state);
