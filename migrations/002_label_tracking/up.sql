-- Labels emitted by DiVine's ATProto labeler service.
CREATE TABLE labeler_events (
    seq             BIGSERIAL PRIMARY KEY,
    src_did         TEXT NOT NULL,
    subject_uri     TEXT NOT NULL,
    subject_cid     TEXT,
    val             TEXT NOT NULL,
    neg             BOOLEAN NOT NULL DEFAULT FALSE,
    nostr_event_id  TEXT,
    sha256          TEXT,
    origin          TEXT NOT NULL DEFAULT 'divine',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_labeler_events_subject ON labeler_events(subject_uri);
CREATE INDEX idx_labeler_events_sha256 ON labeler_events(sha256);

-- Inbound labels from external ATProto labelers (Ozone, etc.)
CREATE TABLE inbound_labels (
    id              BIGSERIAL PRIMARY KEY,
    labeler_did     TEXT NOT NULL,
    subject_uri     TEXT NOT NULL,
    val             TEXT NOT NULL,
    neg             BOOLEAN NOT NULL DEFAULT FALSE,
    nostr_event_id  TEXT,
    sha256          TEXT,
    divine_label    TEXT,
    review_state    TEXT NOT NULL DEFAULT 'pending',
    reviewed_by     TEXT,
    reviewed_at     TIMESTAMPTZ,
    raw_json        TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_inbound_labels_review ON inbound_labels(review_state);
CREATE INDEX idx_inbound_labels_subject ON inbound_labels(subject_uri);
