use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub data_path: PathBuf,
    pub zone_path: PathBuf,
    pub domain: String,
    pub wildcard_ip: String,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let data_dir = PathBuf::from(
            std::env::var("LOCALNET_ADMIN_DATA_DIR").unwrap_or_else(|_| "/data".to_string()),
        );
        let zone_dir = PathBuf::from(
            std::env::var("LOCALNET_ADMIN_ZONE_DIR").unwrap_or_else(|_| "/zones".to_string()),
        );
        let domain =
            std::env::var("LOCALNET_ADMIN_DOMAIN").unwrap_or_else(|_| "divine.test".to_string());
        let wildcard_ip = std::env::var("LOCALNET_ADMIN_WILDCARD_IP")
            .unwrap_or_else(|_| "100.64.0.10".to_string());
        let zone_filename = format!("db.{domain}");

        Ok(Self {
            data_path: data_dir.join("handles.json"),
            zone_path: zone_dir.join(zone_filename),
            domain,
            wildcard_ip,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandleRecord {
    pub name: String,
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Deserialize)]
struct CreateHandleRequest {
    name: String,
    did: String,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<Mutex<FileBackedStore>>,
}

impl AppState {
    pub fn from_config(config: AppConfig) -> anyhow::Result<Self> {
        let store = FileBackedStore::load(config)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
        })
    }

    fn create_handle(&self, name: &str, did: &str) -> anyhow::Result<HandleRecord> {
        let mut store = self.store.lock().unwrap();
        store.upsert(name, did)
    }

    fn get_handle(&self, name: &str) -> anyhow::Result<Option<HandleRecord>> {
        let store = self.store.lock().unwrap();
        Ok(store.get(name))
    }
}

struct FileBackedStore {
    data_path: PathBuf,
    zone_path: PathBuf,
    domain: String,
    wildcard_ip: String,
    records: BTreeMap<String, HandleRecord>,
}

impl FileBackedStore {
    fn load(config: AppConfig) -> anyhow::Result<Self> {
        let records = if config.data_path.exists() {
            let raw = std::fs::read_to_string(&config.data_path).with_context(|| {
                format!(
                    "failed to read handle store {}",
                    config.data_path.display()
                )
            })?;
            serde_json::from_str(&raw).context("failed to parse handle store JSON")?
        } else {
            BTreeMap::new()
        };

        let mut store = Self {
            data_path: config.data_path,
            zone_path: config.zone_path,
            domain: config.domain,
            wildcard_ip: config.wildcard_ip,
            records,
        };
        store.persist()?;
        Ok(store)
    }

    fn get(&self, name: &str) -> Option<HandleRecord> {
        self.records.get(name).cloned()
    }

    fn upsert(&mut self, name: &str, did: &str) -> anyhow::Result<HandleRecord> {
        let record = HandleRecord {
            name: name.to_string(),
            handle: format!("{name}.{}", self.domain),
            did: did.to_string(),
        };
        self.records.insert(name.to_string(), record.clone());
        self.persist()?;
        Ok(record)
    }

    fn persist(&mut self) -> anyhow::Result<()> {
        write_json_atomic(&self.data_path, &self.records)?;
        write_string_atomic(&self.zone_path, &self.render_zone())?;
        Ok(())
    }

    fn render_zone(&self) -> String {
        let mut zone = format!(
            "$ORIGIN {}.\n$TTL 60\n@ IN SOA ns1.{}. admin.{}. 1 60 60 60 60\n@ IN NS ns1.{}\nns1 IN A {}\n* IN A {}\n",
            self.domain,
            self.domain,
            self.domain,
            self.domain,
            self.wildcard_ip,
            self.wildcard_ip
        );

        for record in self.records.values() {
            zone.push_str(&format!("{} IN A {}\n", record.name, self.wildcard_ip));
            zone.push_str(&format!(
                "_atproto.{} IN TXT \"did={}\"\n",
                record.name, record.did
            ));
        }

        zone
    }
}

pub fn app_with_config(config: AppConfig) -> anyhow::Result<Router> {
    let state = AppState::from_config(config)?;
    Ok(app_with_state(state))
}

pub fn app_with_state_for_tests() -> Router {
    let root = std::env::temp_dir().join(format!(
        "divine-localnet-admin-test-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        next_test_root_suffix()
    ));
    std::fs::create_dir_all(&root).expect("test data dir should be created");
    let config = AppConfig {
        data_path: root.join("handles.json"),
        zone_path: root.join("db.divine.test"),
        domain: "divine.test".to_string(),
        wildcard_ip: "100.64.0.10".to_string(),
    };
    app_with_config(config).expect("test app should build")
}

fn next_test_root_suffix() -> u64 {
    static TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);
    TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::next_test_root_suffix;

    #[test]
    fn test_root_suffix_increments() {
        let first = next_test_root_suffix();
        let second = next_test_root_suffix();
        assert!(second > first);
    }
}

fn app_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/handles", post(create_handle))
        .route("/api/handles/:name", get(get_handle))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn create_handle(
    State(state): State<AppState>,
    Json(payload): Json<CreateHandleRequest>,
) -> Result<(StatusCode, Json<HandleRecord>), ApiError> {
    validate_name(&payload.name)?;
    validate_did(&payload.did)?;
    let record = state
        .create_handle(&payload.name, &payload.did)
        .context("failed to persist handle record")
        .map_err(ApiError::internal)?;
    Ok((StatusCode::CREATED, Json(record)))
}

async fn get_handle(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<HandleRecord>, ApiError> {
    validate_name(&name)?;
    let record = state
        .get_handle(&name)
        .context("failed to read handle record")
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("handle {name} not found")))?;
    Ok(Json(record))
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty() {
        return Err(ApiError::bad_request("handle name must not be empty"));
    }

    if name.starts_with('-') || name.ends_with('-') {
        return Err(ApiError::bad_request(
            "handle name must not start or end with a hyphen",
        ));
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(ApiError::bad_request(
            "handle name must contain only lowercase letters, digits, or hyphens",
        ));
    }

    Ok(())
}

fn validate_did(did: &str) -> Result<(), ApiError> {
    if did.starts_with("did:") {
        Ok(())
    } else {
        Err(ApiError::bad_request("did must start with did:"))
    }
}

fn write_json_atomic(path: &Path, records: &BTreeMap<String, HandleRecord>) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(records).context("failed to serialize handle store")?;
    write_string_atomic(path, &json)
}

fn write_string_atomic(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create parent directory {}", parent.display())
        })?;
    }

    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)
        .with_context(|| format!("failed to write temp file {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to move {} into place", path.display()))?;
    Ok(())
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}
