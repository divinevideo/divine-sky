CREATE TABLE appview_repos (
  did TEXT PRIMARY KEY,
  handle TEXT,
  head TEXT,
  rev TEXT,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  last_backfilled_at TIMESTAMPTZ,
  last_seen_seq BIGINT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE appview_profiles (
  did TEXT PRIMARY KEY,
  handle TEXT,
  display_name TEXT,
  description TEXT,
  website TEXT,
  avatar_cid TEXT,
  banner_cid TEXT,
  created_at TIMESTAMPTZ,
  raw_json TEXT,
  indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE appview_posts (
  uri TEXT PRIMARY KEY,
  did TEXT NOT NULL,
  rkey TEXT NOT NULL,
  record_cid TEXT,
  created_at TIMESTAMPTZ NOT NULL,
  text TEXT NOT NULL,
  langs_json TEXT,
  embed_blob_cid TEXT,
  embed_alt TEXT,
  aspect_ratio_width INTEGER,
  aspect_ratio_height INTEGER,
  raw_json TEXT,
  search_text TEXT NOT NULL DEFAULT '',
  indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at TIMESTAMPTZ
);

CREATE TABLE appview_media_views (
  did TEXT NOT NULL,
  blob_cid TEXT NOT NULL,
  playlist_url TEXT NOT NULL,
  thumbnail_url TEXT,
  mime_type TEXT NOT NULL,
  bytes BIGINT NOT NULL,
  ready BOOLEAN NOT NULL DEFAULT FALSE,
  last_derived_at TIMESTAMPTZ,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (did, blob_cid)
);

CREATE TABLE appview_service_state (
  state_key TEXT PRIMARY KEY,
  state_value TEXT,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX appview_repos_handle_idx ON appview_repos (handle);
CREATE INDEX appview_profiles_handle_idx ON appview_profiles (handle);
CREATE INDEX appview_posts_author_created_idx ON appview_posts (did, created_at DESC);
CREATE INDEX appview_posts_created_idx ON appview_posts (created_at DESC);
CREATE INDEX appview_posts_blob_idx ON appview_posts (embed_blob_cid);
CREATE INDEX appview_posts_search_idx
  ON appview_posts
  USING GIN (to_tsvector('simple', COALESCE(search_text, '')));
