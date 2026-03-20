# ATProto ↔ Nostr Bidirectional Moderation & Labeling Bridge

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable moderation labels and enforcement actions to flow bidirectionally between ATProto and Nostr so that content flagged on either network is handled on both.

**Architecture:** Two independent data flows share a common vocabulary and mapping layer. **Outbound:** DiVine's existing Hive AI classifications and human moderator decisions are emitted as signed ATProto labels via a labeler service at `labeler.divine.video`. **Inbound:** A subscriber consumes `com.atproto.label.subscribeLabels` from Bluesky's Ozone and other trusted labelers, maps labels back through `record_mappings` to Nostr event IDs, and publishes NIP-32 (kind 1985) label events and/or NIP-09 deletions. The bridge PostgreSQL `moderation_actions` table is the shared audit log for both directions.

**Tech Stack:**
- Rust (divine-sky workspace: Diesel 2.2, Tokio, Serde, rsky crates)
- JavaScript/Cloudflare Workers (divine-moderation-service: nostr-tools, Hive AI)
- PostgreSQL (bridge DB) + ClickHouse (moderation_labels) + Cloudflare D1/KV
- ATProto: `com.atproto.label.*` lexicons, Ozone moderation API

---

## Existing Infrastructure (Reference)

### What already exists

| Component | Location | What it does |
|---|---|---|
| **Moderation vocabulary** | `divine-moderation-service/src/moderation/vocabulary.mjs` | Canonical label set (`nudity`, `sexual`, `porn`, `violence`, etc.) with alias normalization |
| **Label writer (ClickHouse)** | `divine-moderation-service/src/moderation/label-writer.mjs` | Writes normalized labels to `moderation_labels` table with source metadata |
| **NIP-56 publisher** | `divine-moderation-service/src/nostr/publisher.mjs` | Publishes kind 1984 reports to faro + content relay |
| **NIP-32 publisher** | `divine-moderation-service/src/nostr/publisher.mjs:publishLabelEvent()` | Publishes kind 1985 labels for human-verified content |
| **Translator self-labels** | `divine-sky/crates/divine-atbridge/src/translator.rs` | Maps Nostr `content-warning` tags → ATProto `com.atproto.label.defs#selfLabels` |
| **Bridge types** | `divine-sky/crates/divine-bridge-types/src/lib.rs` | `ModerationAction`, `ModerationOrigin`, `RecordStatus` enums |
| **Bridge DB** | `divine-sky/migrations/001_bridge_tables/up.sql` | `moderation_actions`, `record_mappings`, `account_links` tables |
| **Moderation adapter stub** | `divine-sky/crates/divine-moderation-adapter/src/labels.rs` | Maps `nsfw`→`divine-adult`, `spam`→`divine-spam`, etc. + inbound queue |
| **DM moderation workflow** | `divine-moderation-service/src/nostr/dm-sender.mjs` | Sends NIP-17 DMs to creators for enforcement actions |

### Key mapping tables

- `record_mappings`: `nostr_event_id` ↔ `at_uri` (e.g., `at://did:plc:user/app.bsky.feed.post/rkey`)
- `account_links`: `nostr_pubkey` ↔ `did` + `handle`
- `asset_manifest`: `source_sha256` → `at_blob_cid`
- `moderation_actions`: audit log with `origin` (nostr/atproto/manual) and `subject_id`

---

## Chunk 1: Shared Vocabulary & ATProto Label Types

### Task 1: ATProto ↔ Divine label vocabulary mapping

The existing `vocabulary.mjs` defines canonical labels for the JS side. The Rust `labels.rs` has a separate mapping. We need a single source of truth that maps between:
- DiVine canonical labels (used in ClickHouse `moderation_labels`)
- ATProto label values (used in `com.atproto.label` records)
- Nostr NIP-32 label values (used in kind 1985 events)

> **IMPORTANT: Module restructure required.** The current `src/labels.rs` is a flat file.
> To support submodules (`vocabulary`, `outbound`, `inbound`, `labeler_service`), we must
> convert it to a directory module: `src/labels/mod.rs`. The existing content moves into
> `mod.rs`, and new submodules go into `src/labels/vocabulary.rs`, etc.
> The `main.rs` declaration `pub mod labels;` works for both structures.

**Files:**
- Move: `crates/divine-moderation-adapter/src/labels.rs` → `crates/divine-moderation-adapter/src/labels/mod.rs`
- Create: `crates/divine-moderation-adapter/src/labels/vocabulary.rs`
- Modify: `crates/divine-moderation-adapter/tests/label_mapping.rs`

- [ ] **Step 1: Write failing tests for the vocabulary mapping**

In `crates/divine-moderation-adapter/tests/label_mapping.rs`, add tests for the new vocabulary:

```rust
use divine_moderation_adapter::labels::vocabulary::{atproto_to_divine, divine_to_atproto, divine_to_nip32, VOCABULARY};

#[test]
fn vocabulary_covers_all_atproto_content_labels() {
    // ATProto's standard content labels per the label spec
    let atproto_labels = ["porn", "sexual", "nudity", "gore", "graphic-media", "self-harm"];
    for label in atproto_labels {
        assert!(
            atproto_to_divine(label).is_some(),
            "ATProto label '{}' should map to a divine label",
            label
        );
    }
}

#[test]
fn vocabulary_covers_all_atproto_system_labels() {
    // System labels that require enforcement, not just display
    assert_eq!(atproto_to_divine("!takedown"), Some("takedown"));
    assert_eq!(atproto_to_divine("!suspend"), Some("suspend"));
    assert_eq!(atproto_to_divine("!warn"), Some("content-warning"));
}

#[test]
fn divine_to_atproto_roundtrips_for_content_labels() {
    let divine_labels = ["nudity", "sexual", "porn", "graphic-media", "violence", "self-harm"];
    for label in divine_labels {
        let at_label = divine_to_atproto(label);
        assert!(at_label.is_some(), "Divine label '{}' should map to ATProto", label);
        let back = atproto_to_divine(at_label.unwrap());
        assert!(back.is_some(), "ATProto label should map back");
    }
}

#[test]
fn divine_to_nip32_maps_to_content_warning_namespace() {
    let (namespace, value) = divine_to_nip32("nudity").unwrap();
    assert_eq!(namespace, "content-warning");
    assert_eq!(value, "nudity");
}

#[test]
fn takedown_maps_to_nip09_not_nip32() {
    // Takedowns produce NIP-09 deletions, not NIP-32 labels
    assert!(divine_to_nip32("takedown").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p divine-moderation-adapter -- --nocapture`
Expected: FAIL — `vocabulary` module not found

- [ ] **Step 3: Implement the vocabulary module**

Create `crates/divine-moderation-adapter/src/labels/vocabulary.rs`:

```rust
//! Bidirectional vocabulary mapping between DiVine, ATProto, and Nostr label systems.

/// A single vocabulary entry mapping labels across all three systems.
#[derive(Debug, Clone)]
pub struct VocabEntry {
    /// DiVine canonical label (matches ClickHouse moderation_labels.label)
    pub divine: &'static str,
    /// ATProto label value (com.atproto.label val field)
    pub atproto: &'static str,
    /// NIP-32 (kind 1985) label value, or None if this label uses a different NIP
    pub nip32: Option<&'static str>,
    /// NIP-32 namespace (L tag)
    pub nip32_namespace: &'static str,
    /// Whether this label requires enforcement action (not just display)
    pub requires_enforcement: bool,
}

/// Primary vocabulary: one DiVine label → one ATProto label (outbound direction).
/// For inbound, `atproto_to_divine()` also consults `INBOUND_ALIASES`.
pub const VOCABULARY: &[VocabEntry] = &[
    // Content classification labels (display-only)
    VocabEntry { divine: "nudity",        atproto: "nudity",        nip32: Some("nudity"),        nip32_namespace: "content-warning", requires_enforcement: false },
    VocabEntry { divine: "sexual",        atproto: "sexual",        nip32: Some("sexual"),        nip32_namespace: "content-warning", requires_enforcement: false },
    VocabEntry { divine: "porn",          atproto: "porn",          nip32: Some("porn"),          nip32_namespace: "content-warning", requires_enforcement: false },
    VocabEntry { divine: "graphic-media", atproto: "graphic-media", nip32: Some("graphic-media"), nip32_namespace: "content-warning", requires_enforcement: false },
    VocabEntry { divine: "violence",      atproto: "violence",      nip32: Some("violence"),      nip32_namespace: "content-warning", requires_enforcement: false },
    VocabEntry { divine: "self-harm",     atproto: "self-harm",     nip32: Some("self-harm"),     nip32_namespace: "content-warning", requires_enforcement: true  },

    // AI detection labels
    VocabEntry { divine: "ai-generated",  atproto: "ai-generated",  nip32: Some("ai-generated"),  nip32_namespace: "content-warning", requires_enforcement: false },
    VocabEntry { divine: "deepfake",      atproto: "deepfake",      nip32: Some("deepfake"),      nip32_namespace: "content-warning", requires_enforcement: true  },

    // Behavioral labels
    VocabEntry { divine: "spam",          atproto: "spam",          nip32: Some("spam"),          nip32_namespace: "content-warning", requires_enforcement: true  },
    VocabEntry { divine: "hate",          atproto: "hate",          nip32: Some("hate"),          nip32_namespace: "content-warning", requires_enforcement: true  },
    VocabEntry { divine: "harassment",    atproto: "harassment",    nip32: Some("harassment"),    nip32_namespace: "content-warning", requires_enforcement: true  },

    // System/enforcement labels (no NIP-32 — these produce actions, not labels)
    VocabEntry { divine: "takedown",         atproto: "!takedown",  nip32: None, nip32_namespace: "", requires_enforcement: true },
    VocabEntry { divine: "suspend",          atproto: "!suspend",   nip32: None, nip32_namespace: "", requires_enforcement: true },
    VocabEntry { divine: "content-warning",  atproto: "!warn",      nip32: None, nip32_namespace: "", requires_enforcement: false },
];

/// Inbound-only aliases: ATProto labels that map to DiVine labels
/// but are NOT the primary outbound label for that DiVine category.
/// `gore` on ATProto maps to DiVine `graphic-media` (same as `graphic-media`).
const INBOUND_ALIASES: &[(&str, &str)] = &[
    ("gore", "graphic-media"),
];

/// Map an ATProto label value to the DiVine canonical label.
/// Checks primary vocabulary first, then inbound aliases.
pub fn atproto_to_divine(atproto_val: &str) -> Option<&'static str> {
    VOCABULARY
        .iter()
        .find(|e| e.atproto == atproto_val)
        .map(|e| e.divine)
        .or_else(|| {
            INBOUND_ALIASES
                .iter()
                .find(|(at, _)| *at == atproto_val)
                .map(|(_, divine)| *divine)
        })
}

/// Map a DiVine canonical label to the ATProto label value.
pub fn divine_to_atproto(divine_label: &str) -> Option<&'static str> {
    VOCABULARY.iter().find(|e| e.divine == divine_label).map(|e| e.atproto)
}

/// Map a DiVine canonical label to a NIP-32 (namespace, value) pair.
/// Returns None for labels that use NIP-09 deletion or other mechanisms.
pub fn divine_to_nip32(divine_label: &str) -> Option<(&'static str, &'static str)> {
    VOCABULARY
        .iter()
        .find(|e| e.divine == divine_label)
        .and_then(|e| e.nip32.map(|v| (e.nip32_namespace, v)))
}

/// Look up a vocabulary entry by ATProto label value.
pub fn get_entry_by_atproto(atproto_val: &str) -> Option<&'static VocabEntry> {
    VOCABULARY.iter().find(|e| e.atproto == atproto_val)
}

/// Check if an ATProto label requires enforcement action.
pub fn requires_enforcement(atproto_val: &str) -> bool {
    get_entry_by_atproto(atproto_val).map_or(false, |e| e.requires_enforcement)
}
```

- [ ] **Step 4: Restructure labels module to a directory**

Convert the flat file to a directory module:

```bash
mkdir -p crates/divine-moderation-adapter/src/labels
mv crates/divine-moderation-adapter/src/labels.rs crates/divine-moderation-adapter/src/labels/mod.rs
```

Add `pub mod vocabulary;` at the top of `crates/divine-moderation-adapter/src/labels/mod.rs`.

Update `crates/divine-moderation-adapter/tests/label_mapping.rs` to use crate imports instead of the `#[path]` hack:

```rust
// Remove old: #[path = "../src/labels.rs"]
// Use crate imports instead:
use divine_moderation_adapter::labels::vocabulary::{atproto_to_divine, divine_to_atproto, divine_to_nip32};
use divine_moderation_adapter::labels::{map_action_to_label, queue_inbound_moderation, ModerationAction, SubjectKind};
```

Also update `crates/divine-moderation-adapter/Cargo.toml` to expose a lib target alongside the bin:

```toml
[lib]
name = "divine_moderation_adapter"
path = "src/main.rs"
```

And change `src/main.rs` to re-export the module:

```rust
pub mod labels;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    tracing::info!("divine moderation adapter ready");
    Ok(())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p divine-moderation-adapter -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-moderation-adapter/src/labels/ crates/divine-moderation-adapter/tests/label_mapping.rs crates/divine-moderation-adapter/Cargo.toml
git commit -m "feat: add bidirectional ATProto/Nostr/DiVine label vocabulary"
```

---

### Task 2: ATProto label types for the Rust bridge

Add ATProto label record types to `divine-bridge-types` so both the labeler service and inbound subscriber can serialize/deserialize labels.

> **Note:** The `subscribeLabels` WebSocket uses DAG-CBOR encoding, not JSON. These types
> are directly useful for the HTTP `queryLabels` endpoint and for internal processing.
> The actual WebSocket subscriber (deferred to Phase 3b) will need CBOR decoding via `rsky`
> crates before deserializing into these types.

**Files:**
- Create: `crates/divine-bridge-types/src/atproto_labels.rs`
- Modify: `crates/divine-bridge-types/src/lib.rs`

- [ ] **Step 1: Write failing tests for ATProto label serialization**

Add to `crates/divine-bridge-types/src/atproto_labels.rs` (at the bottom, in a `#[cfg(test)]` block):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_serializes_to_atproto_format() {
        let label = AtprotoLabel {
            ver: Some(1),
            src: "did:plc:divine-labeler".to_string(),
            uri: "at://did:plc:user123/app.bsky.feed.post/abc123".to_string(),
            cid: None,
            val: "sexual".to_string(),
            neg: false,
            cts: "2026-03-20T12:00:00.000Z".to_string(),
            exp: None,
            sig: None,
        };
        let json = serde_json::to_value(&label).unwrap();
        assert_eq!(json["ver"], 1);
        assert_eq!(json["src"], "did:plc:divine-labeler");
        assert_eq!(json["val"], "sexual");
        assert_eq!(json["neg"], false);
        // cid, exp, sig should be absent (skip_serializing_if)
        assert!(json.get("cid").is_none());
    }

    #[test]
    fn negation_label_round_trips() {
        let label = AtprotoLabel {
            ver: Some(1),
            src: "did:plc:divine-labeler".to_string(),
            uri: "at://did:plc:user123/app.bsky.feed.post/abc123".to_string(),
            cid: None,
            val: "nudity".to_string(),
            neg: true,
            cts: "2026-03-20T12:00:00.000Z".to_string(),
            exp: None,
            sig: None,
        };
        let json_str = serde_json::to_string(&label).unwrap();
        let back: AtprotoLabel = serde_json::from_str(&json_str).unwrap();
        assert!(back.neg);
        assert_eq!(back.val, "nudity");
    }

    #[test]
    fn subscribe_labels_message_parses_labels_variant() {
        let json = r#"{"seq":42,"labels":[{"ver":1,"src":"did:plc:ozone","uri":"at://did:plc:u/app.bsky.feed.post/x","val":"porn","neg":false,"cts":"2026-03-20T00:00:00Z"}]}"#;
        let msg: SubscribeLabelsMessage = serde_json::from_str(json).unwrap();
        match msg {
            SubscribeLabelsMessage::Labels { seq, labels } => {
                assert_eq!(seq, 42);
                assert_eq!(labels.len(), 1);
                assert_eq!(labels[0].val, "porn");
            }
            _ => panic!("Expected Labels variant"),
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p divine-bridge-types -- --nocapture`
Expected: FAIL — module not found

- [ ] **Step 3: Implement the ATProto label types**

Create `crates/divine-bridge-types/src/atproto_labels.rs`:

```rust
//! ATProto label types matching the com.atproto.label lexicon.
//!
//! Reference: https://atproto.com/specs/label

use serde::{Deserialize, Serialize};

/// A single ATProto label as defined by com.atproto.label.defs#label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtprotoLabel {
    /// Label version (currently 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ver: Option<u32>,

    /// DID of the labeler that created this label.
    pub src: String,

    /// AT URI of the subject (post, account, blob).
    pub uri: String,

    /// CID of the specific version labeled (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,

    /// Label value (e.g., "porn", "!takedown", "nudity").
    pub val: String,

    /// If true, this is a negation (removal) of a previous label.
    #[serde(default)]
    pub neg: bool,

    /// Creation timestamp (ISO 8601).
    pub cts: String,

    /// Expiration timestamp (ISO 8601, optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<String>,

    /// Signature bytes (base64, optional — present in signed label events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

/// Messages received from com.atproto.label.subscribeLabels WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SubscribeLabelsMessage {
    /// A batch of labels.
    Labels {
        seq: i64,
        labels: Vec<AtprotoLabel>,
    },
    /// An info message (e.g., OutdatedCursor).
    Info {
        name: String,
        message: Option<String>,
    },
}

impl AtprotoLabel {
    /// Check if this is a system/enforcement label (starts with !).
    pub fn is_system_label(&self) -> bool {
        self.val.starts_with('!')
    }

    /// Check if this label targets a specific post (vs account or blob).
    pub fn targets_post(&self) -> bool {
        self.uri.contains("/app.bsky.feed.post/")
    }

    /// Check if this label targets an account (DID with no collection).
    pub fn targets_account(&self) -> bool {
        self.uri.starts_with("did:") && !self.uri.contains('/')
    }

    /// Extract the DID from the label's URI.
    pub fn subject_did(&self) -> Option<&str> {
        if self.uri.starts_with("at://") {
            self.uri.strip_prefix("at://").and_then(|s| s.split('/').next())
        } else if self.uri.starts_with("did:") {
            Some(self.uri.split('/').next().unwrap_or(&self.uri))
        } else {
            None
        }
    }
}
```

- [ ] **Step 4: Wire into lib.rs**

Add to `crates/divine-bridge-types/src/lib.rs`:

```rust
pub mod atproto_labels;
pub use atproto_labels::*;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p divine-bridge-types -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-bridge-types/src/atproto_labels.rs crates/divine-bridge-types/src/lib.rs
git commit -m "feat: add ATProto label types for labeler and subscriber"
```

---

## Chunk 2: Outbound — DiVine AI Labels → ATProto Labeler Service

### Task 3: Database migration for label tracking

Add a table to track which labels have been emitted to ATProto, with sequence numbers for `subscribeLabels`.

**Files:**
- Create: `migrations/002_label_tracking/up.sql`
- Create: `migrations/002_label_tracking/down.sql`
- Modify: `crates/divine-bridge-db/src/schema.rs`
- Modify: `crates/divine-bridge-db/src/models.rs`
- Modify: `crates/divine-bridge-db/src/queries.rs`

- [ ] **Step 1: Write the migration SQL**

Create `migrations/002_label_tracking/up.sql`:

```sql
-- Labels emitted by DiVine's ATProto labeler service.
-- Each row is one signed label; seq is the labeler's sequence number
-- (used by subscribeLabels consumers to resume).
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
-- Stored separately for audit; actions are in moderation_actions.
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
```

Create `migrations/002_label_tracking/down.sql`:

```sql
DROP TABLE IF EXISTS inbound_labels;
DROP TABLE IF EXISTS labeler_events;
```

- [ ] **Step 2: Add Diesel schema declarations**

Append to `crates/divine-bridge-db/src/schema.rs`:

```rust
diesel::table! {
    labeler_events (seq) {
        seq -> Int8,
        src_did -> Text,
        subject_uri -> Text,
        subject_cid -> Nullable<Text>,
        val -> Text,
        neg -> Bool,
        nostr_event_id -> Nullable<Text>,
        sha256 -> Nullable<Text>,
        origin -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    inbound_labels (id) {
        id -> Int8,
        labeler_did -> Text,
        subject_uri -> Text,
        val -> Text,
        neg -> Bool,
        nostr_event_id -> Nullable<Text>,
        sha256 -> Nullable<Text>,
        divine_label -> Nullable<Text>,
        review_state -> Text,
        reviewed_by -> Nullable<Text>,
        reviewed_at -> Nullable<Timestamptz>,
        raw_json -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}
```

Update the `allow_tables_to_appear_in_same_query!` macro to include the new tables.

- [ ] **Step 3: Add Diesel models**

Append to `crates/divine-bridge-db/src/models.rs`:

```rust
// ---------------------------------------------------------------------------
// labeler_events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = labeler_events)]
#[diesel(primary_key(seq))]
pub struct LabelerEvent {
    pub seq: i64,
    pub src_did: String,
    pub subject_uri: String,
    pub subject_cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub nostr_event_id: Option<String>,
    pub sha256: Option<String>,
    pub origin: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = labeler_events)]
pub struct NewLabelerEvent<'a> {
    pub src_did: &'a str,
    pub subject_uri: &'a str,
    pub subject_cid: Option<&'a str>,
    pub val: &'a str,
    pub neg: bool,
    pub nostr_event_id: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub origin: &'a str,
}

// ---------------------------------------------------------------------------
// inbound_labels
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = inbound_labels)]
pub struct InboundLabel {
    pub id: i64,
    pub labeler_did: String,
    pub subject_uri: String,
    pub val: String,
    pub neg: bool,
    pub nostr_event_id: Option<String>,
    pub sha256: Option<String>,
    pub divine_label: Option<String>,
    pub review_state: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub raw_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = inbound_labels)]
pub struct NewInboundLabel<'a> {
    pub labeler_did: &'a str,
    pub subject_uri: &'a str,
    pub val: &'a str,
    pub neg: bool,
    pub nostr_event_id: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub divine_label: Option<&'a str>,
    pub review_state: &'a str,
    pub raw_json: Option<&'a str>,
}
```

- [ ] **Step 4: Add queries**

Append to `crates/divine-bridge-db/src/queries.rs`:

```rust
// ---------------------------------------------------------------------------
// labeler_events queries
// ---------------------------------------------------------------------------

/// Insert a new outbound label event and return it with its assigned sequence number.
pub fn insert_labeler_event(
    conn: &mut PgConnection,
    event: &NewLabelerEvent,
) -> Result<LabelerEvent> {
    let result = diesel::insert_into(labeler_events::table)
        .values(event)
        .get_result::<LabelerEvent>(conn)?;
    Ok(result)
}

/// Get labeler events after a given sequence number (for subscribeLabels).
pub fn get_labeler_events_after(
    conn: &mut PgConnection,
    after_seq: i64,
    limit: i64,
) -> Result<Vec<LabelerEvent>> {
    let results = labeler_events::table
        .filter(labeler_events::seq.gt(after_seq))
        .order(labeler_events::seq.asc())
        .limit(limit)
        .load::<LabelerEvent>(conn)?;
    Ok(results)
}

/// Get the latest sequence number.
pub fn get_latest_labeler_seq(conn: &mut PgConnection) -> Result<Option<i64>> {
    use diesel::dsl::max;
    let result = labeler_events::table
        .select(max(labeler_events::seq))
        .first::<Option<i64>>(conn)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// inbound_labels queries
// ---------------------------------------------------------------------------

/// Insert an inbound label from an external ATProto labeler.
pub fn insert_inbound_label(
    conn: &mut PgConnection,
    label: &NewInboundLabel,
) -> Result<InboundLabel> {
    let result = diesel::insert_into(inbound_labels::table)
        .values(label)
        .get_result::<InboundLabel>(conn)?;
    Ok(result)
}

/// Get pending inbound labels for human review.
pub fn get_pending_inbound_labels(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<InboundLabel>> {
    let results = inbound_labels::table
        .filter(inbound_labels::review_state.eq("pending"))
        .order(inbound_labels::created_at.asc())
        .limit(limit)
        .load::<InboundLabel>(conn)?;
    Ok(results)
}

/// Update inbound label review state.
pub fn update_inbound_label_review(
    conn: &mut PgConnection,
    label_id: i64,
    new_state: &str,
    reviewer: &str,
) -> Result<InboundLabel> {
    let result = diesel::update(inbound_labels::table.find(label_id))
        .set((
            inbound_labels::review_state.eq(new_state),
            inbound_labels::reviewed_by.eq(Some(reviewer)),
            inbound_labels::reviewed_at.eq(Some(diesel::dsl::now)),
        ))
        .get_result::<InboundLabel>(conn)?;
    Ok(result)
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p divine-bridge-db`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add migrations/002_label_tracking/ crates/divine-bridge-db/src/
git commit -m "feat: add label tracking tables for ATProto labeler + inbound labels"
```

---

### Task 4: Outbound label emitter — DiVine moderation → ATProto labels

When DiVine's moderation service classifies a video (or a human moderator acts), emit an ATProto label by inserting into `labeler_events`. This is the write side — the labeler HTTP/WebSocket endpoints (Task 5) serve these labels.

**Files:**
- Create: `crates/divine-moderation-adapter/src/labels/outbound.rs`
- Modify: `crates/divine-moderation-adapter/src/labels/mod.rs`
- Create: `crates/divine-moderation-adapter/tests/outbound.rs`

- [ ] **Step 1: Write failing test for outbound label emission**

Create `crates/divine-moderation-adapter/tests/outbound.rs`:

```rust
//! Tests for outbound label emission logic (no DB required — tests the mapping only).

use divine_moderation_adapter::labels::outbound::OutboundLabel;
use divine_moderation_adapter::labels::vocabulary::divine_to_atproto;

#[test]
fn quarantine_nudity_produces_atproto_label() {
    let result = OutboundLabel::from_moderation_result(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "QUARANTINE",
        &[("nudity", 0.91)],
        "did:plc:divine-labeler",
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].val, "nudity");
    assert_eq!(result[0].subject_uri, "at://did:plc:user1/app.bsky.feed.post/rkey1");
    assert!(!result[0].neg);
}

#[test]
fn safe_result_produces_no_labels() {
    let result = OutboundLabel::from_moderation_result(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "SAFE",
        &[("nudity", 0.1)],
        "did:plc:divine-labeler",
    );
    assert!(result.is_empty());
}

#[test]
fn permanent_ban_produces_takedown_label() {
    let result = OutboundLabel::from_moderation_result(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "PERMANENT_BAN",
        &[("violence", 0.95)],
        "did:plc:divine-labeler",
    );
    // Should produce both the content label AND the takedown system label
    let vals: Vec<&str> = result.iter().map(|l| l.val.as_str()).collect();
    assert!(vals.contains(&"violence"));
    assert!(vals.contains(&"!takedown"));
}

#[test]
fn negation_label_for_human_rejection() {
    let result = OutboundLabel::from_rejection(
        "abc123sha256",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "nudity",
        "did:plc:divine-labeler",
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].val, "nudity");
    assert!(result[0].neg);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p divine-moderation-adapter outbound -- --nocapture`
Expected: FAIL — `outbound` module not found

- [ ] **Step 3: Implement the outbound label mapper**

Create `crates/divine-moderation-adapter/src/labels/outbound.rs`:

```rust
//! Outbound label emission: DiVine moderation results → ATProto label records.

use crate::vocabulary::divine_to_atproto;

/// An outbound label ready to be inserted into labeler_events.
#[derive(Debug, Clone)]
pub struct OutboundLabel {
    pub subject_uri: String,
    pub sha256: String,
    pub val: String,
    pub neg: bool,
    pub src_did: String,
}

/// Score threshold for emitting a content label.
const LABEL_CONFIDENCE_THRESHOLD: f64 = 0.5;

impl OutboundLabel {
    /// Generate ATProto labels from a moderation result.
    ///
    /// `scores` is a slice of (divine_category, score) pairs.
    /// Only scores above threshold produce labels.
    /// PERMANENT_BAN also produces a `!takedown` system label.
    pub fn from_moderation_result(
        sha256: &str,
        at_uri: &str,
        action: &str,
        scores: &[(&str, f64)],
        labeler_did: &str,
    ) -> Vec<Self> {
        if action == "SAFE" {
            return vec![];
        }

        let mut labels = Vec::new();

        // Content classification labels for scores above threshold
        for (category, score) in scores {
            if *score < LABEL_CONFIDENCE_THRESHOLD {
                continue;
            }
            if let Some(at_val) = divine_to_atproto(category) {
                labels.push(Self {
                    subject_uri: at_uri.to_string(),
                    sha256: sha256.to_string(),
                    val: at_val.to_string(),
                    neg: false,
                    src_did: labeler_did.to_string(),
                });
            }
        }

        // System labels based on action
        if action == "PERMANENT_BAN" {
            labels.push(Self {
                subject_uri: at_uri.to_string(),
                sha256: sha256.to_string(),
                val: "!takedown".to_string(),
                neg: false,
                src_did: labeler_did.to_string(),
            });
        }

        labels
    }

    /// Generate a negation label (human moderator rejected a category).
    pub fn from_rejection(
        sha256: &str,
        at_uri: &str,
        divine_category: &str,
        labeler_did: &str,
    ) -> Vec<Self> {
        if let Some(at_val) = divine_to_atproto(divine_category) {
            vec![Self {
                subject_uri: at_uri.to_string(),
                sha256: sha256.to_string(),
                val: at_val.to_string(),
                neg: true,
                src_did: labeler_did.to_string(),
            }]
        } else {
            vec![]
        }
    }
}
```

- [ ] **Step 4: Wire into labels.rs**

Add `pub mod outbound;` to `crates/divine-moderation-adapter/src/labels/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p divine-moderation-adapter -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-moderation-adapter/src/labels/outbound.rs crates/divine-moderation-adapter/src/labels/mod.rs crates/divine-moderation-adapter/tests/outbound.rs
git commit -m "feat: add outbound label emission for ATProto labeler"
```

---

### Task 5: ATProto labeler HTTP endpoints

Implement the two required ATProto labeler XRPC endpoints:
- `com.atproto.label.queryLabels` — HTTP GET to query labels
- `com.atproto.label.subscribeLabels` — WebSocket for streaming labels

These serve labels from the `labeler_events` table.

**Files:**
- Create: `crates/divine-moderation-adapter/src/labels/labeler_service.rs`
- Create: `crates/divine-moderation-adapter/tests/labeler_service.rs`

- [ ] **Step 1: Write failing test for queryLabels response format**

Create `crates/divine-moderation-adapter/tests/labeler_service.rs`:

```rust
//! Tests for the labeler service response formatting.

use divine_moderation_adapter::labels::labeler_service::{format_query_labels_response, QueryLabelsParams};

#[test]
fn query_labels_formats_response_correctly() {
    let events = vec![
        labels::labeler_service::StoredLabel {
            seq: 1,
            src_did: "did:plc:divine-labeler".to_string(),
            subject_uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
            subject_cid: None,
            val: "nudity".to_string(),
            neg: false,
            created_at: "2026-03-20T12:00:00Z".to_string(),
        },
    ];
    let response = format_query_labels_response(&events, None);
    let json: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert!(json["labels"].is_array());
    assert_eq!(json["labels"][0]["val"], "nudity");
    assert_eq!(json["labels"][0]["src"], "did:plc:divine-labeler");
}

#[test]
fn query_labels_filters_by_uri_patterns() {
    let params = QueryLabelsParams {
        uri_patterns: Some(vec!["at://did:plc:user1/*".to_string()]),
        sources: None,
        limit: 50,
        cursor: None,
    };
    assert!(params.matches_uri("at://did:plc:user1/app.bsky.feed.post/rkey1"));
    assert!(!params.matches_uri("at://did:plc:user2/app.bsky.feed.post/rkey1"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p divine-moderation-adapter labeler_service -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement the labeler service formatting**

Create `crates/divine-moderation-adapter/src/labels/labeler_service.rs`:

```rust
//! ATProto labeler service endpoint formatters.
//!
//! The actual HTTP/WebSocket server depends on the runtime (Axum/Rocket).
//! This module provides the query logic and response formatting.

use serde::{Deserialize, Serialize};

/// A label as stored in labeler_events, ready to be served.
#[derive(Debug, Clone, Serialize)]
pub struct StoredLabel {
    pub seq: i64,
    pub src_did: String,
    pub subject_uri: String,
    pub subject_cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub created_at: String,
}

/// Parameters for com.atproto.label.queryLabels.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryLabelsParams {
    /// URI patterns to match (with optional * wildcard suffix).
    #[serde(rename = "uriPatterns")]
    pub uri_patterns: Option<Vec<String>>,
    /// Source DIDs to filter by.
    pub sources: Option<Vec<String>>,
    /// Max results (default 50, max 250).
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Cursor for pagination (seq number).
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    50
}

impl QueryLabelsParams {
    /// Check if a URI matches any of the configured patterns.
    pub fn matches_uri(&self, uri: &str) -> bool {
        match &self.uri_patterns {
            None => true,
            Some(patterns) => patterns.iter().any(|p| {
                if let Some(prefix) = p.strip_suffix('*') {
                    uri.starts_with(prefix)
                } else {
                    uri == p
                }
            }),
        }
    }

    /// Check if a source DID matches the filter.
    pub fn matches_source(&self, src: &str) -> bool {
        match &self.sources {
            None => true,
            Some(sources) => sources.iter().any(|s| s == src),
        }
    }
}

/// Format labels into a com.atproto.label.queryLabels response body.
pub fn format_query_labels_response(labels: &[StoredLabel], cursor: Option<&str>) -> String {
    #[derive(Serialize)]
    struct LabelOutput {
        ver: u32,
        src: String,
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cid: Option<String>,
        val: String,
        neg: bool,
        cts: String,
    }

    #[derive(Serialize)]
    struct Response {
        labels: Vec<LabelOutput>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
    }

    let output = Response {
        labels: labels
            .iter()
            .map(|l| LabelOutput {
                ver: 1,
                src: l.src_did.clone(),
                uri: l.subject_uri.clone(),
                cid: l.subject_cid.clone(),
                val: l.val.clone(),
                neg: l.neg,
                cts: l.created_at.clone(),
            })
            .collect(),
        cursor: cursor.map(String::from),
    };

    serde_json::to_string(&output).unwrap_or_else(|_| r#"{"labels":[]}"#.to_string())
}
```

- [ ] **Step 4: Wire into labels.rs**

Add `pub mod labeler_service;` to `crates/divine-moderation-adapter/src/labels/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p divine-moderation-adapter -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-moderation-adapter/src/labels/labeler_service.rs crates/divine-moderation-adapter/src/labels/mod.rs crates/divine-moderation-adapter/tests/labeler_service.rs
git commit -m "feat: add ATProto labeler queryLabels endpoint formatter"
```

---

## Chunk 3: Inbound — ATProto Labels → Nostr

### Task 6: ATProto label subscriber (inbound pipeline)

Subscribe to `com.atproto.label.subscribeLabels` from trusted labelers (Bluesky Ozone at minimum). When a label targets a DiVine content URI:

1. Look up the Nostr event via `record_mappings`
2. Map the ATProto label to DiVine vocabulary
3. Store in `inbound_labels` for audit
4. For auto-approved labels: publish to Nostr immediately
5. For enforcement labels (`!takedown`, `!suspend`): queue for human review

**Files:**
- Create: `crates/divine-moderation-adapter/src/labels/inbound.rs`
- Create: `crates/divine-moderation-adapter/tests/inbound.rs`

- [ ] **Step 1: Write failing test for inbound label processing**

Create `crates/divine-moderation-adapter/tests/inbound.rs`:

```rust
use divine_moderation_adapter::labels::inbound::{InboundAction, process_inbound_label};
use divine_moderation_adapter::labels::vocabulary::atproto_to_divine;

#[test]
fn content_label_auto_approved_for_trusted_labeler() {
    let action = process_inbound_label(
        "did:plc:ozone-mod",      // trusted labeler
        "sexual",                 // content label
        false,                    // not negation
        &["did:plc:ozone-mod"],   // trusted labelers list
    );
    assert_eq!(action, InboundAction::AutoApprove);
}

#[test]
fn takedown_always_requires_review() {
    let action = process_inbound_label(
        "did:plc:ozone-mod",
        "!takedown",
        false,
        &["did:plc:ozone-mod"],
    );
    assert_eq!(action, InboundAction::RequiresReview);
}

#[test]
fn untrusted_labeler_requires_review() {
    let action = process_inbound_label(
        "did:plc:random-labeler",
        "nudity",
        false,
        &["did:plc:ozone-mod"],  // random-labeler not in trusted list
    );
    assert_eq!(action, InboundAction::RequiresReview);
}

#[test]
fn negation_from_trusted_labeler_auto_approved() {
    let action = process_inbound_label(
        "did:plc:ozone-mod",
        "nudity",
        true,  // negation
        &["did:plc:ozone-mod"],
    );
    assert_eq!(action, InboundAction::AutoApprove);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p divine-moderation-adapter inbound -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement the inbound label processor**

Create `crates/divine-moderation-adapter/src/labels/inbound.rs`:

```rust
//! Inbound label processing: ATProto labels → DiVine moderation queue.

use crate::vocabulary::{atproto_to_divine, requires_enforcement};

/// What to do with an inbound label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundAction {
    /// Label is from a trusted source and non-enforcement; apply automatically.
    AutoApprove,
    /// Label needs human review before acting on it.
    RequiresReview,
    /// Label doesn't map to anything DiVine cares about; ignore.
    Ignore,
}

/// Determine the action for an inbound ATProto label.
///
/// IMPORTANT: Callers must check `labeler_did != own_labeler_did` before
/// calling this function, to avoid re-processing labels DiVine itself emitted.
pub fn process_inbound_label(
    labeler_did: &str,
    atproto_val: &str,
    neg: bool,
    trusted_labelers: &[&str],
) -> InboundAction {
    // If we don't recognize this label, ignore it
    if atproto_to_divine(atproto_val).is_none() {
        return InboundAction::Ignore;
    }

    let is_trusted = trusted_labelers.contains(&labeler_did);

    // Enforcement labels (takedown, suspend) ALWAYS require human review
    if requires_enforcement(atproto_val) && !neg {
        return InboundAction::RequiresReview;
    }

    // Content labels from trusted labelers are auto-approved
    if is_trusted {
        return InboundAction::AutoApprove;
    }

    // Everything else needs review
    InboundAction::RequiresReview
}

/// Determine what Nostr action(s) to take for an approved inbound label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NostrAction {
    /// Publish a NIP-32 (kind 1985) label event.
    PublishLabel {
        namespace: String,
        value: String,
        nostr_event_id: String,
    },
    /// Publish a NIP-09 (kind 5) deletion event.
    PublishDeletion {
        nostr_event_id: String,
        reason: String,
    },
    /// Ban pubkey at relay level.
    RelayBan {
        nostr_pubkey: String,
        reason: String,
    },
    /// No Nostr action needed (e.g., label only applies to ATProto side).
    None,
}

/// Map an approved inbound label to Nostr action(s).
pub fn map_to_nostr_actions(
    atproto_val: &str,
    neg: bool,
    nostr_event_id: &str,
    nostr_pubkey: &str,
) -> Vec<NostrAction> {
    use crate::vocabulary::divine_to_nip32;

    let divine_label = match atproto_to_divine(atproto_val) {
        Some(l) => l,
        None => return vec![NostrAction::None],
    };

    match atproto_val {
        "!takedown" if !neg => vec![
            NostrAction::PublishDeletion {
                nostr_event_id: nostr_event_id.to_string(),
                reason: format!("ATProto takedown label from labeler"),
            },
        ],
        "!suspend" if !neg => vec![
            NostrAction::RelayBan {
                nostr_pubkey: nostr_pubkey.to_string(),
                reason: "ATProto account suspension".to_string(),
            },
        ],
        _ => {
            if let Some((namespace, value)) = divine_to_nip32(divine_label) {
                vec![NostrAction::PublishLabel {
                    namespace: namespace.to_string(),
                    value: if neg {
                        format!("not-{}", value)
                    } else {
                        value.to_string()
                    },
                    nostr_event_id: nostr_event_id.to_string(),
                }]
            } else {
                vec![NostrAction::None]
            }
        }
    }
}
```

- [ ] **Step 4: Wire into labels.rs**

Add `pub mod inbound;` to `crates/divine-moderation-adapter/src/labels/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p divine-moderation-adapter -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-moderation-adapter/src/labels/inbound.rs crates/divine-moderation-adapter/src/labels/mod.rs crates/divine-moderation-adapter/tests/inbound.rs
git commit -m "feat: add inbound ATProto label processor with trust + review gating"
```

---

### Task 7: Moderation API webhook for outbound label emission

When the JS moderation service classifies a video or a human moderator acts, it needs to notify the Rust labeler. Add a webhook endpoint to the moderation API that the JS service calls after each moderation result.

**Files:**
- Create: `divine-moderation-service/src/atproto/label-webhook.mjs`
- Create: `divine-moderation-service/src/atproto/label-webhook.test.mjs`
- Modify: `divine-moderation-service/src/index.mjs` (add route)

- [ ] **Step 1: Write failing test for the webhook payload builder**

Create `divine-moderation-service/src/atproto/label-webhook.test.mjs`:

```javascript
import { describe, it, expect } from 'vitest';
import { buildLabelWebhookPayload } from './label-webhook.mjs';

describe('buildLabelWebhookPayload', () => {
  it('builds payload from quarantine result with scores', () => {
    const result = {
      sha256: 'abc123',
      action: 'QUARANTINE',
      scores: { nudity: 0.91, violence: 0.1 },
    };
    const payload = buildLabelWebhookPayload(result);
    expect(payload.sha256).toBe('abc123');
    expect(payload.action).toBe('QUARANTINE');
    expect(payload.labels).toEqual([{ category: 'nudity', score: 0.91 }]);
  });

  it('omits scores below threshold', () => {
    const result = {
      sha256: 'abc123',
      action: 'REVIEW',
      scores: { nudity: 0.3, violence: 0.1 },
    };
    const payload = buildLabelWebhookPayload(result);
    expect(payload.labels).toEqual([]);
  });

  it('includes multiple labels when multiple scores qualify', () => {
    const result = {
      sha256: 'abc123',
      action: 'QUARANTINE',
      scores: { nudity: 0.8, violence: 0.7, ai_generated: 0.9 },
    };
    const payload = buildLabelWebhookPayload(result);
    expect(payload.labels.length).toBe(3);
  });

  it('returns null for SAFE results', () => {
    const result = {
      sha256: 'abc123',
      action: 'SAFE',
      scores: { nudity: 0.1 },
    };
    const payload = buildLabelWebhookPayload(result);
    expect(payload).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test src/atproto/label-webhook.test.mjs`
Expected: FAIL — module not found

- [ ] **Step 3: Implement the webhook payload builder**

Create `divine-moderation-service/src/atproto/label-webhook.mjs`:

```javascript
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// ABOUTME: Builds webhook payloads for ATProto label emission
// ABOUTME: Called after moderation results to notify the Rust labeler service

const LABEL_THRESHOLD = 0.5;

/**
 * Build a webhook payload to send to the ATProto labeler service.
 * Returns null if no labels should be emitted (SAFE result).
 *
 * @param {Object} result - Moderation result
 * @param {string} result.sha256 - Content hash
 * @param {string} result.action - SAFE, REVIEW, QUARANTINE, AGE_RESTRICTED, PERMANENT_BAN
 * @param {Object} result.scores - Category scores { nudity: 0.9, violence: 0.1, ... }
 * @param {string} [result.reviewed_by] - Human reviewer if manually acted
 * @returns {Object|null} Webhook payload or null
 */
export function buildLabelWebhookPayload(result) {
  if (result.action === 'SAFE') return null;

  const labels = [];
  for (const [category, score] of Object.entries(result.scores || {})) {
    if (score >= LABEL_THRESHOLD) {
      labels.push({ category, score });
    }
  }

  return {
    sha256: result.sha256,
    action: result.action,
    labels,
    reviewed_by: result.reviewed_by || null,
    timestamp: new Date().toISOString(),
  };
}

/**
 * Send moderation result to the ATProto labeler service webhook.
 * Fire-and-forget: logs errors but doesn't throw.
 *
 * @param {Object} result - Moderation result
 * @param {Object} env - Environment with ATPROTO_LABELER_WEBHOOK_URL
 */
export async function notifyAtprotoLabeler(result, env) {
  if (!env.ATPROTO_LABELER_WEBHOOK_URL) return;

  const payload = buildLabelWebhookPayload(result);
  if (!payload) return;

  try {
    const resp = await fetch(env.ATPROTO_LABELER_WEBHOOK_URL, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${env.ATPROTO_LABELER_TOKEN || ''}`,
      },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      console.error(`[ATPROTO] Labeler webhook failed: ${resp.status}`);
    }
  } catch (err) {
    console.error(`[ATPROTO] Labeler webhook error: ${err.message}`);
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test src/atproto/label-webhook.test.mjs`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/rabble/code/divine/divine-moderation-service
git add src/atproto/label-webhook.mjs src/atproto/label-webhook.test.mjs
git commit -m "feat: add ATProto labeler webhook payload builder"
```

---

### Task 8: Wire webhook into moderation pipeline

Call `notifyAtprotoLabeler()` from the existing `handleModerationResult()` in `index.mjs` after the moderation result is finalized.

**Files:**
- Modify: `divine-moderation-service/src/index.mjs`

- [ ] **Step 1: Add import at top of index.mjs**

```javascript
import { notifyAtprotoLabeler } from './atproto/label-webhook.mjs';
```

- [ ] **Step 2: Add webhook call in handleModerationResult**

Find the `handleModerationResult` function in `index.mjs`. After the existing label writing and Nostr publishing calls, add:

```javascript
// Notify ATProto labeler service (fire-and-forget)
notifyAtprotoLabeler({ sha256, action: classification.action, scores: classification.scores, reviewed_by: classification.reviewed_by }, env).catch(err => {
  console.error('[QUEUE] ATProto labeler notification failed:', err.message);
});
```

- [ ] **Step 3: Also wire into the admin moderate endpoint**

Find the `POST /admin/api/moderate/:sha256` handler. After the existing moderation update logic, add:

```javascript
// Notify ATProto labeler of manual override
notifyAtprotoLabeler({ sha256, action: newAction, scores: existingResult?.scores || {}, reviewed_by: 'admin' }, env).catch(err => {
  console.error('[ADMIN] ATProto labeler notification failed:', err.message);
});
```

- [ ] **Step 4: Run existing tests to verify nothing broke**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test`
Expected: All existing tests PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/rabble/code/divine/divine-moderation-service
git add src/index.mjs
git commit -m "feat: wire ATProto labeler webhook into moderation pipeline"
```

---

## Chunk 4: Inbound Subscriber & Nostr Publisher

### Task 9: ClickHouse label writer for inbound ATProto labels

Extend the existing `label-writer.mjs` to also write inbound ATProto labels to ClickHouse, using the same `moderation_labels` table with `source_type: 'external-labeler'`.

**Files:**
- Modify: `divine-moderation-service/src/moderation/label-writer.mjs`
- Create: `divine-moderation-service/src/moderation/label-writer-inbound.test.mjs`

- [ ] **Step 1: Write failing test**

Create `divine-moderation-service/src/moderation/label-writer-inbound.test.mjs`:

```javascript
import { describe, it, expect, vi } from 'vitest';
import { writeInboundAtprotoLabel } from './label-writer.mjs';

describe('writeInboundAtprotoLabel', () => {
  it('writes to ClickHouse with external-labeler source type', async () => {
    const mockFetch = vi.fn().mockResolvedValue({ ok: true });
    globalThis.fetch = mockFetch;

    const env = {
      CLICKHOUSE_URL: 'http://clickhouse:8123',
      CLICKHOUSE_PASSWORD: 'test',
    };

    await writeInboundAtprotoLabel('abc123sha256', {
      labeler_did: 'did:plc:ozone-mod',
      val: 'nudity',
      neg: false,
    }, env);

    expect(mockFetch).toHaveBeenCalledOnce();
    const body = mockFetch.mock.calls[0][1].body;
    const row = JSON.parse(body);
    expect(row.sha256).toBe('abc123sha256');
    expect(row.label).toBe('nudity');
    expect(row.source_id).toBe('did:plc:ozone-mod');
    expect(row.source_type).toBe('external-labeler');
    expect(row.transport).toBe('atproto-firehose');
  });

  it('skips if no ClickHouse config', async () => {
    const mockFetch = vi.fn();
    globalThis.fetch = mockFetch;
    await writeInboundAtprotoLabel('abc', { labeler_did: 'x', val: 'y', neg: false }, {});
    expect(mockFetch).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test src/moderation/label-writer-inbound.test.mjs`
Expected: FAIL

- [ ] **Step 3: Add the inbound writer function**

Append to `divine-moderation-service/src/moderation/label-writer.mjs`:

```javascript
/**
 * Write an inbound ATProto label to ClickHouse moderation_labels table.
 *
 * @param {string} sha256 - Content hash
 * @param {Object} labelData
 * @param {string} labelData.labeler_did - Source labeler DID
 * @param {string} labelData.val - ATProto label value
 * @param {boolean} labelData.neg - Is this a negation
 * @param {Object} env - Worker environment
 */
export async function writeInboundAtprotoLabel(sha256, labelData, env) {
  if (!env.CLICKHOUSE_URL || !env.CLICKHOUSE_PASSWORD) return;

  const row = {
    sha256,
    label: normalizeLabel(labelData.val),
    source_id: labelData.labeler_did,
    source_owner: 'atproto',
    source_type: 'external-labeler',
    transport: 'atproto-firehose',
    confidence: 1.0,
    operation: labelData.neg ? 'clear' : 'apply',
    review_state: 'external',
    action: '',
    updated_at: new Date().toISOString(),
  };

  const query = 'INSERT INTO moderation_labels FORMAT JSONEachRow';

  try {
    const resp = await fetch(`${env.CLICKHOUSE_URL}/?database=default&query=${encodeURIComponent(query)}`, {
      method: 'POST',
      headers: {
        'X-ClickHouse-User': env.CLICKHOUSE_USER || 'default',
        'X-ClickHouse-Key': env.CLICKHOUSE_PASSWORD,
        'Content-Type': 'application/x-ndjson',
      },
      body: JSON.stringify(row),
    });
    if (!resp.ok) {
      console.error('[LABELS] ClickHouse inbound write failed:', resp.status, await resp.text());
    }
  } catch (err) {
    console.error('[LABELS] ClickHouse inbound write error:', err.message);
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test src/moderation/label-writer-inbound.test.mjs`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/rabble/code/divine/divine-moderation-service
git add src/moderation/label-writer.mjs src/moderation/label-writer-inbound.test.mjs
git commit -m "feat: add ClickHouse writer for inbound ATProto labels"
```

---

### Task 10: Nostr label publisher for inbound ATProto labels

When an inbound ATProto label is approved (auto or human), publish the corresponding NIP-32 label event to the Nostr relay. This reuses the existing `publishLabelEvent()` in `publisher.mjs`.

**Files:**
- Create: `divine-moderation-service/src/atproto/inbound-publisher.mjs`
- Create: `divine-moderation-service/src/atproto/inbound-publisher.test.mjs`

- [ ] **Step 1: Write failing test**

Create `divine-moderation-service/src/atproto/inbound-publisher.test.mjs`:

```javascript
import { describe, it, expect, vi } from 'vitest';
import { buildNostrLabelFromAtproto } from './inbound-publisher.mjs';

describe('buildNostrLabelFromAtproto', () => {
  it('maps nudity label to NIP-32 publish params', () => {
    const result = buildNostrLabelFromAtproto({
      val: 'nudity',
      neg: false,
      sha256: 'abc123',
      nostrEventId: 'nostr-event-123',
    });

    expect(result).not.toBeNull();
    expect(result.category).toBe('nudity');
    expect(result.status).toBe('confirmed');
    expect(result.nostrEventId).toBe('nostr-event-123');
  });

  it('maps negation to rejected status', () => {
    const result = buildNostrLabelFromAtproto({
      val: 'nudity',
      neg: true,
      sha256: 'abc123',
      nostrEventId: 'nostr-event-123',
    });

    expect(result.status).toBe('rejected');
  });

  it('maps takedown to deletion action', () => {
    const result = buildNostrLabelFromAtproto({
      val: '!takedown',
      neg: false,
      sha256: 'abc123',
      nostrEventId: 'nostr-event-123',
    });

    expect(result.action).toBe('delete');
    expect(result.category).toBeNull();
  });

  it('returns null for unknown labels', () => {
    const result = buildNostrLabelFromAtproto({
      val: 'custom-unknown-label',
      neg: false,
      sha256: 'abc123',
    });

    expect(result).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test src/atproto/inbound-publisher.test.mjs`
Expected: FAIL

- [ ] **Step 3: Implement the inbound publisher mapper**

Create `divine-moderation-service/src/atproto/inbound-publisher.mjs`:

```javascript
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// ABOUTME: Maps inbound ATProto labels to Nostr NIP-32 / NIP-09 publish actions
// ABOUTME: Used when ATProto labels are approved for cross-network propagation

/**
 * ATProto label → DiVine/Nostr category mapping.
 * Must stay in sync with divine-moderation-adapter/src/vocabulary.rs
 */
/**
 * MUST stay in sync with divine-moderation-adapter/src/labels/vocabulary.rs
 * DiVine canonical label → the key used in publisher.mjs CATEGORY_LABELS
 */
const ATPROTO_TO_NOSTR = {
  'porn':          { category: 'nudity',       namespace: 'content-warning' },  // ATProto porn → divine nudity category
  'sexual':        { category: 'nudity',       namespace: 'content-warning' },  // ATProto sexual → divine nudity (closest)
  'nudity':        { category: 'nudity',       namespace: 'content-warning' },
  'gore':          { category: 'violence',     namespace: 'content-warning' },  // inbound alias: gore → graphic-media → violence publisher category
  'graphic-media': { category: 'violence',     namespace: 'content-warning' },  // graphic-media → violence publisher category
  'violence':      { category: 'violence',     namespace: 'content-warning' },
  'self-harm':     { category: 'self_harm',    namespace: 'content-warning' },  // kebab → snake for publisher.mjs CATEGORY_LABELS key
  'spam':          { category: 'offensive',    namespace: 'content-warning' },  // maps to publisher 'profanity' label via CATEGORY_LABELS
  'ai-generated':  { category: 'ai_generated', namespace: 'content-warning' },
  'deepfake':      { category: 'deepfake',     namespace: 'content-warning' },
};

/**
 * Build Nostr publish parameters from an inbound ATProto label.
 *
 * @param {Object} opts
 * @param {string} opts.val - ATProto label value
 * @param {boolean} opts.neg - Is negation
 * @param {string} opts.sha256 - Content hash
 * @param {string} [opts.nostrEventId] - Mapped Nostr event ID
 * @returns {Object|null} Publish params or null if unmapped
 */
export function buildNostrLabelFromAtproto({ val, neg, sha256, nostrEventId }) {
  // System labels → special actions
  if (val === '!takedown' && !neg) {
    return {
      action: 'delete',
      category: null,
      sha256,
      nostrEventId: nostrEventId || null,
    };
  }

  if (val === '!suspend' && !neg) {
    return {
      action: 'ban',
      category: null,
      sha256,
      nostrEventId: nostrEventId || null,
    };
  }

  // Content labels → NIP-32 label events
  const mapping = ATPROTO_TO_NOSTR[val];
  if (!mapping) return null;

  return {
    action: 'label',
    category: mapping.category,
    namespace: mapping.namespace,
    status: neg ? 'rejected' : 'confirmed',
    score: 1.0,
    sha256,
    nostrEventId: nostrEventId || null,
  };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test src/atproto/inbound-publisher.test.mjs`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/rabble/code/divine/divine-moderation-service
git add src/atproto/inbound-publisher.mjs src/atproto/inbound-publisher.test.mjs
git commit -m "feat: add inbound ATProto label to Nostr publisher mapper"
```

---

## Chunk 5: Integration & Admin UI

### Task 11: Admin review endpoint for inbound ATProto labels

Add an endpoint to the admin dashboard where human moderators can review inbound ATProto labels that require approval before propagating to Nostr.

**Files:**
- Modify: `divine-moderation-service/src/index.mjs` (add routes)

- [ ] **Step 1: Add GET /admin/api/inbound-labels route**

In `index.mjs`, add inside the admin routes section:

```javascript
// GET /admin/api/inbound-labels — list pending inbound ATProto labels
if (path === '/admin/api/inbound-labels' && request.method === 'GET') {
  // Query pending inbound labels from bridge DB via REST
  // For MVP: query the bridge DB's inbound_labels table via its API
  // This will be wired once the Rust labeler has an HTTP API
  return new Response(JSON.stringify({ labels: [], message: 'Pending bridge DB integration' }), {
    headers: { 'Content-Type': 'application/json' },
  });
}
```

- [ ] **Step 2: Add POST /admin/api/inbound-labels/:id/approve route**

```javascript
// POST /admin/api/inbound-labels/:id/approve — approve an inbound label for Nostr propagation
if (path.match(/^\/admin\/api\/inbound-labels\/\d+\/approve$/) && request.method === 'POST') {
  // For MVP: stub - will call bridge DB API to update review_state
  // Then publish NIP-32 label to Nostr via existing publishLabelEvent()
  return new Response(JSON.stringify({ status: 'approved', message: 'Pending bridge DB integration' }), {
    headers: { 'Content-Type': 'application/json' },
  });
}
```

- [ ] **Step 3: Run existing tests to verify nothing broke**

Run: `cd /Users/rabble/code/divine/divine-moderation-service && npm test`
Expected: All existing tests PASS

- [ ] **Step 4: Commit**

```bash
cd /Users/rabble/code/divine/divine-moderation-service
git add src/index.mjs
git commit -m "feat: add stub admin endpoints for inbound ATProto label review"
```

---

### Task 12: End-to-end integration test

Write an integration test that exercises the full bidirectional flow using the mapped types.

**Files:**
- Create: `crates/divine-moderation-adapter/tests/integration_flow.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! End-to-end bidirectional moderation flow test (no DB, no network).

use divine_moderation_adapter::labels::outbound::OutboundLabel;
use divine_moderation_adapter::labels::inbound::{InboundAction, NostrAction, process_inbound_label, map_to_nostr_actions};
use divine_moderation_adapter::labels::vocabulary::{atproto_to_divine, divine_to_atproto};

#[test]
fn outbound_divine_label_roundtrips_through_atproto_and_back() {
    // Step 1: DiVine classifies a video as nudity
    let outbound = OutboundLabel::from_moderation_result(
        "sha256abc",
        "at://did:plc:user1/app.bsky.feed.post/rkey1",
        "QUARANTINE",
        &[("nudity", 0.91)],
        "did:plc:divine-labeler",
    );
    assert_eq!(outbound.len(), 1);
    let emitted_val = &outbound[0].val;

    // Step 2: That label comes back via subscribeLabels (simulated inbound)
    let divine_label = atproto_to_divine(emitted_val).unwrap();
    assert_eq!(divine_label, "nudity");

    // Step 3: Inbound processing decides what to do
    let action = process_inbound_label(
        "did:plc:divine-labeler",  // It's our own label coming back
        emitted_val,
        false,
        &["did:plc:divine-labeler"],
    );
    // Our own labels from ourselves are auto-approved (and we'd typically skip them)
    assert_eq!(action, InboundAction::AutoApprove);
}

#[test]
fn external_takedown_flows_to_nostr_deletion() {
    // External labeler issues takedown
    let action = process_inbound_label(
        "did:plc:ozone",
        "!takedown",
        false,
        &["did:plc:ozone"],
    );
    assert_eq!(action, InboundAction::RequiresReview);

    // After human approval, map to Nostr action
    let nostr_actions = map_to_nostr_actions(
        "!takedown",
        false,
        "nostr-event-abc",
        "nostr-pubkey-xyz",
    );
    assert_eq!(nostr_actions.len(), 1);
    match &nostr_actions[0] {
        NostrAction::PublishDeletion { nostr_event_id, .. } => {
            assert_eq!(nostr_event_id, "nostr-event-abc");
        }
        other => panic!("Expected PublishDeletion, got {:?}", other),
    }
}

#[test]
fn external_content_label_flows_to_nip32() {
    // External labeler flags as sexual
    let action = process_inbound_label(
        "did:plc:ozone",
        "sexual",
        false,
        &["did:plc:ozone"],
    );
    assert_eq!(action, InboundAction::AutoApprove);

    let nostr_actions = map_to_nostr_actions(
        "sexual",
        false,
        "nostr-event-def",
        "nostr-pubkey-xyz",
    );
    match &nostr_actions[0] {
        NostrAction::PublishLabel { namespace, value, nostr_event_id } => {
            assert_eq!(namespace, "content-warning");
            assert_eq!(value, "sexual");
            assert_eq!(nostr_event_id, "nostr-event-def");
        }
        other => panic!("Expected PublishLabel, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p divine-moderation-adapter integration_flow -- --nocapture`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add crates/divine-moderation-adapter/tests/integration_flow.rs
git commit -m "test: add end-to-end bidirectional moderation flow tests"
```

---

## Summary: What Gets Built

| Direction | Component | Repo | Runtime |
|---|---|---|---|
| **Shared** | Vocabulary mapping (Rust) | divine-sky | Compile-time |
| **Shared** | ATProto label types | divine-sky | Compile-time |
| **Shared** | DB migration (labeler_events, inbound_labels) | divine-sky | PostgreSQL |
| **Outbound** | Label emitter (moderation → ATProto labels) | divine-sky | Rust binary |
| **Outbound** | queryLabels + subscribeLabels endpoints | divine-sky | Rust HTTP/WS |
| **Outbound** | Webhook from JS moderation service | divine-moderation-service | CF Worker |
| **Inbound** | Label subscriber (ATProto → DiVine) | divine-sky | Rust async |
| **Inbound** | ClickHouse writer for inbound labels | divine-moderation-service | CF Worker |
| **Inbound** | Nostr NIP-32/NIP-09 publisher | divine-moderation-service | CF Worker |
| **Inbound** | Admin review UI for inbound labels | divine-moderation-service | CF Worker |

## What's Deferred (Phase 3b / Phase 4)

- **Ozone dashboard integration** — Running a full Ozone instance; these tasks build the primitive APIs that Ozone would consume
- **subscribeLabels WebSocket server** — The DB and formatter are ready; the actual Tokio WebSocket server depends on how divine-moderation-adapter is deployed (standalone binary vs. integrated into divine-atbridge)
- **ATProto firehose subscriber** — The inbound processor logic is ready; the actual WebSocket client to `com.atproto.label.subscribeLabels` needs network code + reconnection
- **DM notifications for inbound ATProto reports** — Reuse existing `dm-sender.mjs` with new templates
- **Creator dashboard for cross-protocol moderation status** — Needs frontend work
