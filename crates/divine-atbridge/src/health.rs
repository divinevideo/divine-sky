use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::config::BridgeConfig;
use crate::pds_accounts::PdsAccountsClient;
use crate::plc_directory::PlcDirectoryClient;
use crate::provision_runtime::{DbAccountLinkStore, DbProvisioningKeyStore};
use crate::provisioner::{
    AccountLinkStore, AccountProvisioner, KeyStore, PdsAccountCreator, PlcClient, ProvisionResult,
};

const DEGRADED_FAILURE_THRESHOLD: u32 = 3;

#[derive(Clone, Default)]
pub struct RuntimeHealthState {
    inner: Arc<Mutex<RuntimeHealthInner>>,
}

#[derive(Default)]
struct RuntimeHealthInner {
    consecutive_readiness_failures: u32,
    degraded: bool,
    last_error: Option<String>,
}

impl RuntimeHealthState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_relay_failure(&self, error: impl Into<String>) {
        self.record_readiness_failure(error.into());
    }

    pub fn record_runtime_failure(&self, error: impl Into<String>) {
        self.record_readiness_failure(error.into());
    }

    pub fn record_processing_failure(&self, error: impl Into<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.last_error = Some(error.into());
    }

    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.consecutive_readiness_failures = 0;
        inner.degraded = false;
        inner.last_error = None;
    }

    pub fn is_ready(&self) -> bool {
        !self.inner.lock().unwrap().degraded
    }

    pub fn next_retry_delay(&self) -> Duration {
        let failures = self.inner.lock().unwrap().consecutive_readiness_failures;
        let exponent = failures.min(5);
        Duration::from_secs(2u64.saturating_pow(exponent.max(1)))
    }

    fn record_readiness_failure(&self, error: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.consecutive_readiness_failures =
            inner.consecutive_readiness_failures.saturating_add(1);
        inner.last_error = Some(error);
        if inner.consecutive_readiness_failures >= DEGRADED_FAILURE_THRESHOLD {
            inner.degraded = true;
        }
    }
}

#[async_trait]
trait ProvisioningService: Send + Sync {
    async fn provision_account(&self, nostr_pubkey: &str, handle: &str) -> Result<ProvisionResult>;
}

#[async_trait]
impl<K, P, A, L> ProvisioningService for AccountProvisioner<K, P, A, L>
where
    K: KeyStore,
    P: PlcClient,
    A: PdsAccountCreator,
    L: AccountLinkStore,
{
    async fn provision_account(&self, nostr_pubkey: &str, handle: &str) -> Result<ProvisionResult> {
        AccountProvisioner::provision_account(self, nostr_pubkey, handle).await
    }
}

#[derive(Clone, Default)]
struct InternalApiState {
    runtime: RuntimeHealthState,
    expected_bearer: Option<String>,
    provisioner: Option<Arc<dyn ProvisioningService>>,
}

#[derive(Debug, Deserialize)]
struct ProvisionRequest {
    nostr_pubkey: String,
    handle: String,
}

#[derive(Debug, Serialize)]
struct ProvisionResponse {
    did: String,
    handle: String,
    signing_key_id: String,
}

impl From<ProvisionResult> for ProvisionResponse {
    fn from(value: ProvisionResult) -> Self {
        Self {
            did: value.did,
            handle: value.handle,
            signing_key_id: value.signing_key_id,
        }
    }
}

struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = self.0.to_string();
        tracing::error!(error = %message, "AT bridge internal API request failed");
        (status_for_error(&message), message).into_response()
    }
}

fn status_for_error(message: &str) -> StatusCode {
    if message.contains("handle already taken")
        || message.contains("different handle")
        || message.contains("account link is disabled")
    {
        return StatusCode::CONFLICT;
    }

    if message.contains("handle must end with") || message.contains("handle must include") {
        return StatusCode::BAD_REQUEST;
    }

    StatusCode::INTERNAL_SERVER_ERROR
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn health_ready(State(state): State<InternalApiState>) -> StatusCode {
    if state.runtime.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn provision(
    State(state): State<InternalApiState>,
    Json(payload): Json<ProvisionRequest>,
) -> Result<Json<ProvisionResponse>, ApiError> {
    let provisioner = state
        .provisioner
        .as_ref()
        .context("AT bridge provisioning API is not configured")
        .map_err(ApiError)?;
    let result = provisioner
        .provision_account(&payload.nostr_pubkey, &payload.handle)
        .await
        .map_err(ApiError)?;
    Ok(Json(result.into()))
}

async fn require_internal_auth(
    State(state): State<InternalApiState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.provisioner.is_none() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let expected = state
        .expected_bearer
        .as_deref()
        .map(|token| format!("Bearer {token}"));
    if expected.is_none() {
        return Ok(next.run(request).await);
    }
    let actual = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if expected.as_deref() != Some(actual) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

fn app_with_state(state: InternalApiState) -> Router {
    let protected = Router::new()
        .route("/provision", post(provision))
        .with_state(state.clone())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_internal_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/health/ready", get(health_ready))
        .merge(protected)
        .with_state(state)
}

pub fn app() -> Router {
    app_with_runtime_state(RuntimeHealthState::default())
}

pub fn app_with_runtime_state(runtime: RuntimeHealthState) -> Router {
    app_with_state(InternalApiState {
        runtime,
        expected_bearer: None,
        provisioner: None,
    })
}

fn build_configured_provisioner(
    config: &BridgeConfig,
) -> Result<
    AccountProvisioner<
        DbProvisioningKeyStore,
        PlcDirectoryClient,
        PdsAccountsClient,
        DbAccountLinkStore,
    >,
> {
    Ok(AccountProvisioner {
        key_store: DbProvisioningKeyStore::new(
            config.database_url.clone(),
            config.provisioning_key_encryption_key()?,
        ),
        plc_client: PlcDirectoryClient::new(config.plc_directory_url.clone()),
        pds_creator: PdsAccountsClient::new(config.pds_url.clone(), config.pds_auth_token.clone()),
        link_store: DbAccountLinkStore::new(config.database_url.clone()),
        pds_endpoint: config.pds_url.clone(),
        handle_domain: config.handle_domain.clone(),
    })
}

pub fn app_with_config(config: BridgeConfig) -> Result<Router> {
    anyhow::ensure!(
        !config.provisioning_bearer_token.trim().is_empty(),
        "ATPROTO_PROVISIONING_TOKEN must not be empty"
    );
    let provisioner = build_configured_provisioner(&config)?;

    Ok(app_with_state(InternalApiState {
        runtime: RuntimeHealthState::default(),
        expected_bearer: Some(config.provisioning_bearer_token),
        provisioner: Some(Arc::new(provisioner)),
    }))
}

pub async fn spawn(
    config: BridgeConfig,
    runtime: RuntimeHealthState,
) -> Result<tokio::task::JoinHandle<()>> {
    let addr: SocketAddr = config
        .health_bind_addr
        .parse()
        .context("HEALTH_BIND_ADDR must be a valid socket address")?;
    let app = app_with_state(InternalApiState {
        runtime,
        expected_bearer: Some(config.provisioning_bearer_token.clone()),
        provisioner: Some(Arc::new(build_configured_provisioner(&config)?)),
    });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind AT bridge health listener on {addr}"))?;

    Ok(tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(error = %error, %addr, "AT bridge health server stopped");
        }
    }))
}
