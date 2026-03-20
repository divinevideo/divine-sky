# divine-labeler Service Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone ATProto labeler HTTP service that receives moderation results via webhook, signs them as ATProto labels, and serves them to ATProto consumers via the standard `queryLabels` XRPC endpoint.

**Architecture:** New `divine-labeler` crate with Axum HTTP server. Webhook endpoint receives moderation results from the JS moderation service, maps them to ATProto labels via the existing `OutboundLabel` mapper, signs with DAG-CBOR + ECDSA (ES256K), stores in `labeler_events` (PostgreSQL), and serves via `queryLabels`. Follows the same patterns as `divine-handle-gateway` (Axum + shared Mutex PgConnection + bearer token auth).

**Tech Stack:**
- Rust (divine-sky workspace: Axum 0.7, Diesel 2.2, Tokio, Serde)
- `secp256k1` 0.29 (already in workspace — ECDSA signing for ATProto labels)
- `serde_ipld_dagcbor` 0.6 (already in workspace — DAG-CBOR encoding for label signing)
- `sha2` (SHA-256 for signing digest)
- `base64` (sig field encoding)
- PostgreSQL (`labeler_events` table from migration 002)

---

## Existing Infrastructure (Reference)

| Component | Location | What it does |
|---|---|---|
| **Labeler event queries** | `crates/divine-bridge-db/src/queries.rs` | `insert_labeler_event()`, `get_labeler_events_after()`, `get_latest_labeler_seq()` |
| **Labeler event models** | `crates/divine-bridge-db/src/models.rs` | `LabelerEvent`, `NewLabelerEvent` |
| **Outbound label mapper** | `crates/divine-moderation-adapter/src/labels/outbound.rs` | `OutboundLabel::from_moderation_result()` |
| **queryLabels formatter** | `crates/divine-moderation-adapter/src/labels/labeler_service.rs` | `format_query_labels_response()`, `QueryLabelsParams`, `StoredLabel` |
| **ATProto label types** | `crates/divine-bridge-types/src/atproto_labels.rs` | `AtprotoLabel` (has `sig: Option<String>` field) |
| **Handle gateway pattern** | `crates/divine-handle-gateway/` | Reference for Axum app structure, `AppState`, `DbStore`, bearer auth middleware |
| **secp256k1 usage** | `crates/divine-atbridge/src/signature.rs` | Schnorr verification (we need ECDSA signing) |
| **DAG-CBOR dep** | `crates/divine-atbridge/Cargo.toml` | `serde_ipld_dagcbor = "0.6"` already available |
| **Login/signer service** | `login.divine.video` | External service — the labeler's signing key will be managed there |

---

## Chunk 1: Crate Scaffold & Config

### Task 1: Create divine-labeler crate with config

**Files:**
- Create: `crates/divine-labeler/Cargo.toml`
- Create: `crates/divine-labeler/src/main.rs`
- Create: `crates/divine-labeler/src/lib.rs`
- Create: `crates/divine-labeler/src/config.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Write failing test for config loading**

In `crates/divine-labeler/src/config.rs` (at bottom, `#[cfg(test)]` block):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_validates_labeler_did_format() {
        let config = LabelerConfig {
            labeler_did: "did:plc:abc123".to_string(),
            signing_key_hex: "a".repeat(64),
            database_url: "postgres://localhost/test".to_string(),
            webhook_token: "secret".to_string(),
            port: 3001,
        };
        assert!(config.labeler_did.starts_with("did:"));
    }

    #[test]
    fn config_rejects_empty_signing_key() {
        let result = LabelerConfig::validate_signing_key("");
        assert!(result.is_err());
    }

    #[test]
    fn config_accepts_valid_hex_signing_key() {
        // 32 bytes = 64 hex chars
        let result = LabelerConfig::validate_signing_key(&"ab".repeat(32));
        assert!(result.is_ok());
    }
}
```

- [ ] **Step 2: Implement the config module**

Create `crates/divine-labeler/src/config.rs`:

```rust
//! Configuration for the divine-labeler service.

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone)]
pub struct LabelerConfig {
    /// The labeler's DID (e.g., did:plc:...).
    pub labeler_did: String,
    /// Hex-encoded secp256k1 private key for signing labels.
    pub signing_key_hex: String,
    /// PostgreSQL connection string.
    pub database_url: String,
    /// Bearer token expected on webhook requests from the JS moderation service.
    pub webhook_token: String,
    /// Port to listen on.
    pub port: u16,
}

impl LabelerConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            labeler_did: std::env::var("LABELER_DID")
                .context("LABELER_DID must be set")?,
            signing_key_hex: std::env::var("LABELER_SIGNING_KEY")
                .context("LABELER_SIGNING_KEY must be set")?,
            database_url: std::env::var("DATABASE_URL")
                .context("DATABASE_URL must be set")?,
            webhook_token: std::env::var("WEBHOOK_TOKEN")
                .context("WEBHOOK_TOKEN must be set")?,
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3001".to_string())
                .parse()
                .context("PORT must be a valid u16")?,
        })
    }

    /// Validate that a hex string represents a valid 32-byte secp256k1 private key.
    pub fn validate_signing_key(hex: &str) -> Result<Vec<u8>> {
        if hex.is_empty() {
            bail!("signing key must not be empty");
        }
        let bytes = hex::decode(hex).context("signing key must be valid hex")?;
        if bytes.len() != 32 {
            bail!("signing key must be 32 bytes (64 hex chars), got {}", bytes.len());
        }
        Ok(bytes)
    }
}
```

- [ ] **Step 3: Create Cargo.toml**

Create `crates/divine-labeler/Cargo.toml`:

```toml
[package]
name = "divine-labeler"
version = "0.1.0"
edition = "2021"

[lib]
name = "divine_labeler"
path = "src/lib.rs"

[[bin]]
name = "divine-labeler"
path = "src/main.rs"

[dependencies]
divine-bridge-db = { path = "../divine-bridge-db" }
divine-bridge-types = { path = "../divine-bridge-types" }
divine-moderation-adapter = { path = "../divine-moderation-adapter" }
anyhow = { workspace = true }
axum = { version = "0.7", features = ["macros", "json"] }
base64 = "0.22"
chrono = { workspace = true }
diesel = { workspace = true }
hex = "0.4"
k256 = { version = "0.13", features = ["ecdsa-core", "sha256"] }
serde = { workspace = true }
serde_json = { workspace = true }
serde_ipld_dagcbor = "0.6"
sha2 = "0.10"
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

[dev-dependencies]
axum-test = "16"
tower = { version = "0.5", features = ["util"] }
```

> **Note on k256 vs secp256k1 crate:** ATProto labels use ECDSA (ES256K), not Schnorr.
> The existing `secp256k1` crate is used for Nostr Schnorr signatures. `k256` provides
> a cleaner ECDSA API via the `ecdsa` trait. Both implement secp256k1 — different APIs,
> same curve. Using `k256` avoids mixing Schnorr and ECDSA usage in one crate.

- [ ] **Step 4: Create lib.rs and main.rs stubs**

Create `crates/divine-labeler/src/lib.rs`:

```rust
pub mod config;
```

Create `crates/divine-labeler/src/main.rs`:

```rust
use std::net::SocketAddr;

use divine_labeler::config::LabelerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = LabelerConfig::from_env()?;
    LabelerConfig::validate_signing_key(&config.signing_key_hex)?;

    tracing::info!(
        did = %config.labeler_did,
        port = config.port,
        "divine-labeler starting"
    );

    let addr: SocketAddr = ([0, 0, 0, 0], config.port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("listening on {addr}");

    // Server will be wired in Task 3
    Ok(())
}
```

- [ ] **Step 5: Add to workspace**

In the root `Cargo.toml`, add `"crates/divine-labeler"` to the `workspace.members` array.

- [ ] **Step 6: Run tests and verify**

Run: `cargo test -p divine-labeler -- --nocapture`
Expected: 3 tests PASS

Run: `cargo build -p divine-labeler`
Expected: Compiles without errors

- [ ] **Step 7: Commit**

```bash
git add crates/divine-labeler/ Cargo.toml
git commit -m "feat: scaffold divine-labeler crate with config"
```

---

## Chunk 2: Label Signing

### Task 2: DAG-CBOR label signing

ATProto label signatures cover a DAG-CBOR encoding of the label fields (excluding `sig`),
signed with ECDSA (ES256K) using the labeler's secp256k1 key. The signature is a
low-S normalized compact signature (64 bytes, base64-encoded).

**Files:**
- Create: `crates/divine-labeler/src/signing.rs`
- Modify: `crates/divine-labeler/src/lib.rs`

- [ ] **Step 1: Write failing tests for label signing**

In `crates/divine-labeler/src/signing.rs` (at bottom, `#[cfg(test)]` block):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_keypair() -> (SigningKey, String) {
        let key_bytes = [1u8; 32]; // deterministic test key
        let signing_key = SigningKey::from_bytes(&key_bytes.into()).unwrap();
        let hex_key = hex::encode(key_bytes);
        (signing_key, hex_key)
    }

    #[test]
    fn sign_label_produces_base64_signature() {
        let (signing_key, _) = test_keypair();
        let label = UnsignedLabel {
            ver: 1,
            src: "did:plc:test-labeler".to_string(),
            uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
            cid: None,
            val: "nudity".to_string(),
            neg: false,
            cts: "2026-03-20T12:00:00.000Z".to_string(),
        };

        let sig = sign_label(&label, &signing_key).unwrap();
        // Base64 of 64-byte compact ECDSA sig
        assert!(!sig.is_empty());
        let decoded = base64::engine::general_purpose::STANDARD.decode(&sig).unwrap();
        assert_eq!(decoded.len(), 64);
    }

    #[test]
    fn same_label_produces_same_signature() {
        let (signing_key, _) = test_keypair();
        let label = UnsignedLabel {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:u/app.bsky.feed.post/x".to_string(),
            cid: None,
            val: "porn".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
        };

        let sig1 = sign_label(&label, &signing_key).unwrap();
        let sig2 = sign_label(&label, &signing_key).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn different_labels_produce_different_signatures() {
        let (signing_key, _) = test_keypair();
        let label1 = UnsignedLabel {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:u/app.bsky.feed.post/x".to_string(),
            cid: None,
            val: "nudity".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
        };
        let label2 = UnsignedLabel {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:u/app.bsky.feed.post/x".to_string(),
            cid: None,
            val: "porn".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
        };

        let sig1 = sign_label(&label1, &signing_key).unwrap();
        let sig2 = sign_label(&label2, &signing_key).unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn signing_key_from_hex_works() {
        let hex_key = "ab".repeat(32);
        let key = signing_key_from_hex(&hex_key).unwrap();
        assert_eq!(key.to_bytes().len(), 32);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p divine-labeler -- --nocapture`
Expected: FAIL — `signing` module not found

- [ ] **Step 3: Implement the signing module**

Create `crates/divine-labeler/src/signing.rs`:

```rust
//! ATProto label signing using DAG-CBOR + ECDSA (ES256K).
//!
//! ATProto labels are signed by:
//! 1. Constructing a map of label fields (excluding `sig`) in sorted key order
//! 2. Encoding as DAG-CBOR
//! 3. SHA-256 hashing the CBOR bytes
//! 4. Signing the hash with ECDSA over secp256k1 (low-S normalized)
//! 5. Base64-encoding the 64-byte compact signature

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use k256::ecdsa::{signature::Signer, Signature, SigningKey};
use serde::Serialize;

/// Label fields to be signed (no `sig` field — that's what we're computing).
/// Field names match the ATProto label spec exactly.
/// Serialized to DAG-CBOR with sorted keys for deterministic encoding.
#[derive(Debug, Clone, Serialize)]
pub struct UnsignedLabel {
    pub ver: u32,
    pub src: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub cts: String,
}

/// Sign an unsigned label, returning a base64-encoded ECDSA signature.
pub fn sign_label(label: &UnsignedLabel, key: &SigningKey) -> Result<String> {
    // Encode to DAG-CBOR (deterministic, sorted keys)
    let cbor_bytes =
        serde_ipld_dagcbor::to_vec(label).context("failed to encode label as DAG-CBOR")?;

    // Sign the CBOR bytes directly (k256 handles SHA-256 internally)
    let signature: Signature = key.sign(&cbor_bytes);

    // Encode as base64
    Ok(BASE64.encode(signature.to_bytes()))
}

/// Parse a hex-encoded secp256k1 private key into a SigningKey.
pub fn signing_key_from_hex(hex_key: &str) -> Result<SigningKey> {
    let bytes = hex::decode(hex_key).context("invalid hex in signing key")?;
    SigningKey::from_bytes(bytes.as_slice().into()).context("invalid secp256k1 private key")
}
```

- [ ] **Step 4: Wire into lib.rs**

Add `pub mod signing;` to `crates/divine-labeler/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p divine-labeler -- --nocapture`
Expected: All tests PASS (3 config + 4 signing)

- [ ] **Step 6: Commit**

```bash
git add crates/divine-labeler/src/signing.rs crates/divine-labeler/src/lib.rs
git commit -m "feat: add DAG-CBOR label signing with ECDSA (ES256K)"
```

---

## Chunk 3: DB Store & Axum Server

### Task 3: Database store and Axum app wiring

Follow the `divine-handle-gateway` pattern: `DbStore` with `Arc<Mutex<PgConnection>>`,
`AppState` holding config + store, and `app_with_state()` for testability.

**Files:**
- Create: `crates/divine-labeler/src/store.rs`
- Create: `crates/divine-labeler/src/routes/mod.rs`
- Create: `crates/divine-labeler/src/routes/health.rs`
- Modify: `crates/divine-labeler/src/lib.rs`
- Modify: `crates/divine-labeler/src/main.rs`

- [ ] **Step 1: Create the DB store**

Create `crates/divine-labeler/src/store.rs`:

```rust
//! Database store wrapping divine-bridge-db queries for the labeler.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use diesel::Connection;
use diesel::PgConnection;

use divine_bridge_db::models::{LabelerEvent, NewLabelerEvent};
use divine_bridge_db::queries;

type SharedConnection = Arc<Mutex<PgConnection>>;

#[derive(Clone)]
pub struct DbStore {
    connection: SharedConnection,
}

impl DbStore {
    pub fn connect(database_url: &str) -> Result<Self> {
        let connection =
            PgConnection::establish(database_url).context("failed to connect to PostgreSQL")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn insert_labeler_event(&self, event: &NewLabelerEvent) -> Result<LabelerEvent> {
        let mut conn = self.connection.lock().unwrap();
        queries::insert_labeler_event(&mut conn, event)
    }

    pub fn get_events_after(&self, after_seq: i64, limit: i64) -> Result<Vec<LabelerEvent>> {
        let mut conn = self.connection.lock().unwrap();
        queries::get_labeler_events_after(&mut conn, after_seq, limit)
    }

    pub fn get_latest_seq(&self) -> Result<Option<i64>> {
        let mut conn = self.connection.lock().unwrap();
        queries::get_latest_labeler_seq(&mut conn)
    }
}
```

- [ ] **Step 2: Create the health route**

Create directory `crates/divine-labeler/src/routes/` and file `crates/divine-labeler/src/routes/mod.rs`:

```rust
pub mod health;
```

Create `crates/divine-labeler/src/routes/health.rs`:

```rust
use axum::http::StatusCode;

pub async fn handler() -> StatusCode {
    StatusCode::OK
}
```

- [ ] **Step 3: Wire up AppState and Router in lib.rs**

Replace `crates/divine-labeler/src/lib.rs`:

```rust
use axum::body::Body;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use k256::ecdsa::SigningKey;

pub mod config;
pub mod routes;
pub mod signing;
pub mod store;

#[derive(Clone)]
pub struct AppState {
    pub store: store::DbStore,
    pub labeler_did: String,
    pub signing_key: SigningKey,
    pub webhook_token: String,
}

impl AppState {
    pub fn from_config(config: config::LabelerConfig) -> anyhow::Result<Self> {
        let signing_key = signing::signing_key_from_hex(&config.signing_key_hex)?;
        let store = store::DbStore::connect(&config.database_url)?;
        Ok(Self {
            store,
            labeler_did: config.labeler_did,
            signing_key,
            webhook_token: config.webhook_token,
        })
    }
}

pub fn app_with_state(state: AppState) -> Router {
    let public_routes = Router::new()
        .route("/health", get(routes::health::handler))
        .with_state(state.clone());

    // Protected routes will be added in Tasks 4 and 5
    Router::new().merge(public_routes)
}

/// Bearer token auth middleware for webhook endpoints.
pub async fn require_webhook_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = format!("Bearer {}", state.webhook_token);
    let actual = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if actual != expected {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}
```

- [ ] **Step 4: Update main.rs to serve**

Replace `crates/divine-labeler/src/main.rs`:

```rust
use std::net::SocketAddr;

use divine_labeler::config::LabelerConfig;
use divine_labeler::{app_with_state, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = LabelerConfig::from_env()?;
    LabelerConfig::validate_signing_key(&config.signing_key_hex)?;

    let port = config.port;
    let did = config.labeler_did.clone();

    let state = AppState::from_config(config)?;
    let app = app_with_state(state);

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!(did = %did, %addr, "divine-labeler listening");

    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p divine-labeler`
Expected: Compiles (may warn about unused imports — that's fine, they'll be used in Tasks 4-5)

- [ ] **Step 6: Commit**

```bash
git add crates/divine-labeler/src/
git commit -m "feat: add labeler DB store, AppState, and Axum server scaffold"
```

---

## Chunk 4: queryLabels Endpoint

### Task 4: Implement queryLabels XRPC endpoint

Serve labels from `labeler_events` in the ATProto `queryLabels` format.
Uses the existing `format_query_labels_response()` from `divine-moderation-adapter`.

**Files:**
- Create: `crates/divine-labeler/src/routes/query_labels.rs`
- Create: `crates/divine-labeler/tests/query_labels.rs`
- Modify: `crates/divine-labeler/src/routes/mod.rs`
- Modify: `crates/divine-labeler/src/lib.rs` (add route)

- [ ] **Step 1: Write failing test for queryLabels**

Create `crates/divine-labeler/tests/query_labels.rs`:

```rust
//! Tests for the queryLabels endpoint.
//! NOTE: These tests require a PostgreSQL connection. Skip if DB unavailable.
//! For unit testing the response format, see labeler_service tests in divine-moderation-adapter.

use divine_labeler::routes::query_labels::build_query_response;
use divine_bridge_db::models::LabelerEvent;
use chrono::Utc;

#[test]
fn build_query_response_formats_labels_correctly() {
    let events = vec![
        LabelerEvent {
            seq: 1,
            src_did: "did:plc:test-labeler".to_string(),
            subject_uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
            subject_cid: None,
            val: "nudity".to_string(),
            neg: false,
            nostr_event_id: None,
            sha256: Some("abc123".to_string()),
            origin: "divine".to_string(),
            created_at: Utc::now(),
        },
    ];

    let (body, cursor) = build_query_response(&events, "did:plc:test-labeler");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["labels"].is_array());
    assert_eq!(json["labels"][0]["val"], "nudity");
    assert_eq!(json["labels"][0]["src"], "did:plc:test-labeler");
    assert_eq!(json["labels"][0]["ver"], 1);
    assert!(cursor.is_some());
}

#[test]
fn build_query_response_empty_events_returns_empty_labels() {
    let (body, cursor) = build_query_response(&[], "did:plc:test");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["labels"].as_array().unwrap().len(), 0);
    assert!(cursor.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p divine-labeler query_labels -- --nocapture`
Expected: FAIL — module not found

- [ ] **Step 3: Implement the queryLabels route**

Create `crates/divine-labeler/src/routes/query_labels.rs`:

```rust
//! GET /xrpc/com.atproto.label.queryLabels
//!
//! Serves labels from labeler_events in ATProto format.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};

use divine_bridge_db::models::LabelerEvent;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    /// URI patterns to filter (optional, supports * wildcard suffix).
    #[serde(rename = "uriPatterns")]
    pub uri_patterns: Option<String>,
    /// Source DIDs to filter (optional).
    pub sources: Option<String>,
    /// Max results (default 50, max 250).
    pub limit: Option<i64>,
    /// Cursor for pagination (seq number from previous response).
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct QueryLabelsResponse {
    labels: Vec<LabelOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
}

/// Build query response from labeler events. Exported for testing.
pub fn build_query_response(events: &[LabelerEvent], _labeler_did: &str) -> (String, Option<String>) {
    let cursor = events.last().map(|e| e.seq.to_string());

    let labels: Vec<LabelOutput> = events
        .iter()
        .map(|e| LabelOutput {
            ver: 1,
            src: e.src_did.clone(),
            uri: e.subject_uri.clone(),
            cid: e.subject_cid.clone(),
            val: e.val.clone(),
            neg: e.neg,
            cts: e.created_at.to_rfc3339(),
        })
        .collect();

    let response = QueryLabelsResponse {
        labels,
        cursor: cursor.clone(),
    };

    let body = serde_json::to_string(&response).unwrap_or_else(|_| r#"{"labels":[]}"#.to_string());
    (body, cursor)
}

pub async fn handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> Result<Json<QueryLabelsResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(250);
    let after_seq = params
        .cursor
        .as_deref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(0);

    let events = state
        .store
        .get_events_after(after_seq, limit)
        .map_err(|e| {
            tracing::error!("failed to query labeler events: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Apply URI pattern filtering if specified
    let uri_patterns: Vec<String> = params
        .uri_patterns
        .map(|p| p.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let filtered: Vec<&LabelerEvent> = if uri_patterns.is_empty() {
        events.iter().collect()
    } else {
        events
            .iter()
            .filter(|e| {
                uri_patterns.iter().any(|p| {
                    if let Some(prefix) = p.strip_suffix('*') {
                        e.subject_uri.starts_with(prefix)
                    } else {
                        e.subject_uri == *p
                    }
                })
            })
            .collect()
    };

    let cursor = filtered.last().map(|e| e.seq.to_string());

    let labels: Vec<LabelOutput> = filtered
        .iter()
        .map(|e| LabelOutput {
            ver: 1,
            src: e.src_did.clone(),
            uri: e.subject_uri.clone(),
            cid: e.subject_cid.clone(),
            val: e.val.clone(),
            neg: e.neg,
            cts: e.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(QueryLabelsResponse { labels, cursor }))
}
```

- [ ] **Step 4: Wire into routes and app**

Update `crates/divine-labeler/src/routes/mod.rs`:

```rust
pub mod health;
pub mod query_labels;
```

In `crates/divine-labeler/src/lib.rs`, update `app_with_state` to add the route:

```rust
pub fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health::handler))
        .route(
            "/xrpc/com.atproto.label.queryLabels",
            get(routes::query_labels::handler),
        )
        .with_state(state)
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p divine-labeler -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-labeler/src/routes/ crates/divine-labeler/tests/ crates/divine-labeler/src/lib.rs
git commit -m "feat: add queryLabels XRPC endpoint"
```

---

## Chunk 5: Webhook Endpoint

### Task 5: Implement webhook endpoint for moderation results

Receives moderation results from the JS moderation service, maps to ATProto labels,
signs them, and stores in `labeler_events`.

**Files:**
- Create: `crates/divine-labeler/src/routes/webhook.rs`
- Create: `crates/divine-labeler/tests/webhook.rs`
- Modify: `crates/divine-labeler/src/routes/mod.rs`
- Modify: `crates/divine-labeler/src/lib.rs` (add route + auth middleware)

- [ ] **Step 1: Write failing test for webhook payload parsing**

Create `crates/divine-labeler/tests/webhook.rs`:

```rust
//! Tests for the webhook endpoint payload parsing.

use divine_labeler::routes::webhook::WebhookPayload;

#[test]
fn webhook_payload_deserializes_from_js_format() {
    let json = r#"{
        "sha256": "abc123",
        "action": "QUARANTINE",
        "labels": [
            {"category": "nudity", "score": 0.91}
        ],
        "reviewed_by": null,
        "timestamp": "2026-03-20T12:00:00.000Z"
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert_eq!(payload.sha256, "abc123");
    assert_eq!(payload.action, "QUARANTINE");
    assert_eq!(payload.labels.len(), 1);
    assert_eq!(payload.labels[0].category, "nudity");
}

#[test]
fn webhook_payload_handles_multiple_labels() {
    let json = r#"{
        "sha256": "def456",
        "action": "PERMANENT_BAN",
        "labels": [
            {"category": "violence", "score": 0.95},
            {"category": "gore", "score": 0.88}
        ],
        "reviewed_by": "admin",
        "timestamp": "2026-03-20T12:00:00.000Z"
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert_eq!(payload.labels.len(), 2);
    assert_eq!(payload.reviewed_by, Some("admin".to_string()));
}

#[test]
fn webhook_payload_handles_empty_labels() {
    let json = r#"{
        "sha256": "ghi789",
        "action": "REVIEW",
        "labels": [],
        "reviewed_by": null,
        "timestamp": "2026-03-20T12:00:00.000Z"
    }"#;

    let payload: WebhookPayload = serde_json::from_str(json).unwrap();
    assert!(payload.labels.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p divine-labeler webhook -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement the webhook route**

Create `crates/divine-labeler/src/routes/webhook.rs`:

```rust
//! POST /webhook/moderation-result
//!
//! Receives moderation results from the JS moderation service,
//! maps to ATProto labels, signs them, and stores in labeler_events.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use divine_bridge_db::models::NewLabelerEvent;
use divine_moderation_adapter::labels::vocabulary::divine_to_atproto;

use crate::signing::{sign_label, UnsignedLabel};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    pub sha256: String,
    pub action: String,
    pub labels: Vec<LabelScore>,
    pub reviewed_by: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LabelScore {
    pub category: String,
    pub score: f64,
}

#[derive(Debug, Serialize)]
struct WebhookResponse {
    accepted: usize,
    errors: Vec<String>,
}

pub async fn handler(
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> Result<Json<WebhookResponse>, StatusCode> {
    let mut accepted = 0;
    let mut errors = Vec::new();
    let now = Utc::now();
    let cts = now.to_rfc3339();

    // Map each label category to ATProto and sign
    for label_score in &payload.labels {
        let atproto_val = match divine_to_atproto(&label_score.category) {
            Some(v) => v,
            None => {
                errors.push(format!("unknown category: {}", label_score.category));
                continue;
            }
        };

        // Build the unsigned label for signing
        let unsigned = UnsignedLabel {
            ver: 1,
            src: state.labeler_did.clone(),
            uri: format!("at://sha256:{}", payload.sha256), // content-addressed URI
            cid: None,
            val: atproto_val.to_string(),
            neg: false,
            cts: cts.clone(),
        };

        // Sign the label
        let sig = match sign_label(&unsigned, &state.signing_key) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("signing failed for {}: {e}", atproto_val));
                continue;
            }
        };

        // Store in labeler_events
        let new_event = NewLabelerEvent {
            src_did: &state.labeler_did,
            subject_uri: &unsigned.uri,
            subject_cid: None,
            val: atproto_val,
            neg: false,
            nostr_event_id: None,
            sha256: Some(&payload.sha256),
            origin: if payload.reviewed_by.is_some() {
                "human"
            } else {
                "divine"
            },
        };

        match state.store.insert_labeler_event(&new_event) {
            Ok(_event) => {
                tracing::info!(
                    sha256 = %payload.sha256,
                    val = atproto_val,
                    "label emitted (sig={} bytes)",
                    sig.len()
                );
                accepted += 1;
            }
            Err(e) => {
                errors.push(format!("db insert failed for {}: {e}", atproto_val));
            }
        }
    }

    // If action is PERMANENT_BAN, also emit a !takedown label
    if payload.action == "PERMANENT_BAN" {
        let unsigned = UnsignedLabel {
            ver: 1,
            src: state.labeler_did.clone(),
            uri: format!("at://sha256:{}", payload.sha256),
            cid: None,
            val: "!takedown".to_string(),
            neg: false,
            cts: cts.clone(),
        };

        if let Ok(_sig) = sign_label(&unsigned, &state.signing_key) {
            let new_event = NewLabelerEvent {
                src_did: &state.labeler_did,
                subject_uri: &unsigned.uri,
                subject_cid: None,
                val: "!takedown",
                neg: false,
                nostr_event_id: None,
                sha256: Some(&payload.sha256),
                origin: "divine",
            };

            match state.store.insert_labeler_event(&new_event) {
                Ok(_) => {
                    tracing::info!(sha256 = %payload.sha256, "!takedown label emitted");
                    accepted += 1;
                }
                Err(e) => errors.push(format!("db insert failed for !takedown: {e}")),
            }
        }
    }

    Ok(Json(WebhookResponse { accepted, errors }))
}
```

- [ ] **Step 4: Wire into routes and app**

Update `crates/divine-labeler/src/routes/mod.rs`:

```rust
pub mod health;
pub mod query_labels;
pub mod webhook;
```

Update `app_with_state` in `crates/divine-labeler/src/lib.rs`:

```rust
use axum::routing::{get, post};

pub fn app_with_state(state: AppState) -> Router {
    let webhook_routes = Router::new()
        .route("/webhook/moderation-result", post(routes::webhook::handler))
        .with_state(state.clone())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_webhook_auth,
        ));

    Router::new()
        .route("/health", get(routes::health::handler))
        .route(
            "/xrpc/com.atproto.label.queryLabels",
            get(routes::query_labels::handler),
        )
        .merge(webhook_routes)
        .with_state(state)
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p divine-labeler -- --nocapture`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/divine-labeler/src/ crates/divine-labeler/tests/
git commit -m "feat: add webhook endpoint for moderation result label emission"
```

---

## Summary: What Gets Built

| Component | File | Purpose |
|---|---|---|
| Config | `src/config.rs` | Env var loading + validation |
| Signing | `src/signing.rs` | DAG-CBOR + ECDSA label signing |
| DB Store | `src/store.rs` | Thin wrapper over divine-bridge-db queries |
| App State | `src/lib.rs` | Axum app wiring, auth middleware |
| queryLabels | `src/routes/query_labels.rs` | ATProto XRPC endpoint |
| Webhook | `src/routes/webhook.rs` | Receives JS moderation results |
| Health | `src/routes/health.rs` | Health check |

## What's Deferred

- **`subscribeLabels` WebSocket** — Can be added later as a new route
- **AT URI mapping** — Currently uses `at://sha256:{hash}` as content URI; needs integration with `record_mappings` once content is actually mirrored to ATProto
- **DID/PLC setup** — Manual one-time operation
- **`app.bsky.labeler.service` record** — Manual one-time publication
- **Deployment config** — Kubernetes/Docker setup
