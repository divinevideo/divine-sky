use anyhow::Context;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod keycast_client;
pub mod name_server_client;
pub mod provision_runner;
pub mod routes;
pub mod store;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningState {
    Pending,
    Ready,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountLinkRecord {
    pub nostr_pubkey: String,
    pub handle: String,
    pub did: Option<String>,
    pub crosspost_enabled: bool,
    pub provisioning_state: ProvisioningState,
    pub provisioning_error: Option<String>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<divine_bridge_db::models::AccountLinkLifecycleRow> for AccountLinkRecord {
    fn from(value: divine_bridge_db::models::AccountLinkLifecycleRow) -> Self {
        let provisioning_state = match value.provisioning_state.as_str() {
            "pending" => ProvisioningState::Pending,
            "ready" => ProvisioningState::Ready,
            "failed" => ProvisioningState::Failed,
            "disabled" => ProvisioningState::Disabled,
            _ => ProvisioningState::Failed,
        };

        Self {
            nostr_pubkey: value.nostr_pubkey,
            handle: value.handle,
            did: value.did,
            crosspost_enabled: value.crosspost_enabled,
            provisioning_state,
            provisioning_error: value.provisioning_error,
            disabled_at: value.disabled_at,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    store: store::DbStore,
    provision_runner: provision_runner::ProvisionRunner,
    keycast_client: keycast_client::KeycastClient,
    name_server_client: name_server_client::NameServerClient,
    keycast_atproto_token: String,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub keycast_atproto_token: String,
    pub atproto_provisioning_url: String,
    pub atproto_provisioning_token: Option<String>,
    pub atproto_keycast_sync_url: String,
    pub atproto_name_server_sync_url: String,
    pub atproto_name_server_sync_token: String,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .context("DATABASE_URL must be set for handle gateway")?,
            keycast_atproto_token: std::env::var("KEYCAST_ATPROTO_TOKEN")
                .context("KEYCAST_ATPROTO_TOKEN must be set for handle gateway")?,
            atproto_provisioning_url: std::env::var("ATPROTO_PROVISIONING_URL")
                .context("ATPROTO_PROVISIONING_URL must be set for handle gateway")?,
            atproto_provisioning_token: std::env::var("ATPROTO_PROVISIONING_TOKEN").ok(),
            atproto_keycast_sync_url: std::env::var("ATPROTO_KEYCAST_SYNC_URL")
                .context("ATPROTO_KEYCAST_SYNC_URL must be set for handle gateway")?,
            atproto_name_server_sync_url: std::env::var("ATPROTO_NAME_SERVER_SYNC_URL")
                .context("ATPROTO_NAME_SERVER_SYNC_URL must be set for handle gateway")?,
            atproto_name_server_sync_token: std::env::var("ATPROTO_NAME_SERVER_SYNC_TOKEN")
                .context("ATPROTO_NAME_SERVER_SYNC_TOKEN must be set for handle gateway")?,
        })
    }
}

impl AppState {
    pub(crate) fn from_config(config: AppConfig) -> anyhow::Result<Self> {
        let store = store::DbStore::connect(&config.database_url)?;
        let name_server_client = name_server_client::NameServerClient::new(
            config.atproto_name_server_sync_url,
            config.atproto_name_server_sync_token,
        );
        let keycast_client = keycast_client::KeycastClient::new(
            config.atproto_keycast_sync_url,
            config.keycast_atproto_token.clone(),
        );
        let provision_runner = provision_runner::ProvisionRunner::new(
            store.clone(),
            provision_runner::ProvisioningClient::new(
                config.atproto_provisioning_url,
                config.atproto_provisioning_token,
            ),
            name_server_client.clone(),
            keycast_client.clone(),
        );

        Ok(Self {
            store,
            provision_runner,
            keycast_client,
            name_server_client,
            keycast_atproto_token: config.keycast_atproto_token,
        })
    }

    pub(crate) fn upsert_pending_result(
        &self,
        nostr_pubkey: String,
        handle: String,
    ) -> anyhow::Result<AccountLinkRecord> {
        self.store.upsert_pending_opt_in(&nostr_pubkey, &handle)
    }

    pub(crate) fn enqueue_provisioning(&self, nostr_pubkey: &str, handle: &str) {
        self.provision_runner
            .enqueue(nostr_pubkey.to_string(), handle.to_string());
    }

    pub(crate) fn upsert_ready(
        &self,
        nostr_pubkey: String,
        handle: String,
        did: String,
    ) -> AccountLinkRecord {
        if self
            .store
            .get_by_pubkey(&nostr_pubkey)
            .ok()
            .flatten()
            .is_none()
        {
            let _ = self.store.upsert_pending_opt_in(&nostr_pubkey, &handle);
        }
        self.store
            .mark_ready(&nostr_pubkey, &did)
            .expect("failed to mark account link ready")
    }

    pub(crate) fn get_by_pubkey_result(
        &self,
        nostr_pubkey: &str,
    ) -> anyhow::Result<Option<AccountLinkRecord>> {
        self.store.get_by_pubkey(nostr_pubkey)
    }

    pub(crate) fn disable_by_pubkey_result(
        &self,
        nostr_pubkey: &str,
    ) -> anyhow::Result<Option<AccountLinkRecord>> {
        self.store.disable(nostr_pubkey)
    }

    pub(crate) async fn sync_disabled_state(
        &self,
        nostr_pubkey: &str,
        handle: &str,
    ) -> anyhow::Result<()> {
        self.keycast_client.sync_disabled(nostr_pubkey).await?;
        self.name_server_client
            .sync_state_for_handle(handle, None, "disabled")
            .await
    }
}

pub fn app() -> Router {
    let config = AppConfig::from_env().expect("failed to load handle gateway configuration");
    app_with_config(config).expect("failed to construct handle gateway app")
}

pub fn app_with_config(config: AppConfig) -> anyhow::Result<Router> {
    let state = AppState::from_config(config)?;
    Ok(app_with_state(state))
}

fn app_with_state(state: AppState) -> Router {
    let protected_routes = Router::new()
        .route("/api/account-links/opt-in", post(routes::opt_in::handler))
        .route(
            "/api/account-links/provision",
            post(routes::provision::handler),
        )
        .route(
            "/api/account-links/:nostr_pubkey/status",
            get(routes::status::handler),
        )
        .route(
            "/api/account-links/:nostr_pubkey/disable",
            post(routes::disable::handler),
        )
        .route(
            "/api/account-links/:nostr_pubkey/export",
            get(routes::export::handler),
        )
        .with_state(state.clone())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_internal_auth,
        ));

    Router::new()
        .route("/", get(root_info))
        .route("/health", get(health))
        .route("/health/ready", get(health_ready))
        .merge(protected_routes)
        .with_state(state)
}

async fn require_internal_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = format!("Bearer {}", state.keycast_atproto_token);
    let actual = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if actual != expected {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

const ROOT_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>diVine Handle Gateway</title>
<style>
:root {
    --primary: #27C58B;
    --primary-dark: #1fa06f;
    --bg-dark: #1b1b1b;
    --bg-section: #2d2d2d;
    --text: #ffffff;
    --text-muted: #999999;
    --border: #3d3d3d;
    --code-bg: #41444e;
}
body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg-dark);
    color: var(--text);
    line-height: 1.6;
    max-width: 700px;
    margin: 0 auto;
    padding: 2rem;
}
h1 { color: var(--primary); margin-bottom: 0.5rem; }
h2 { color: var(--primary); margin-top: 2rem; font-size: 1.2rem; }
code { background: var(--code-bg); padding: 2px 6px; border-radius: 3px; font-size: 0.9em; }
pre { background: var(--code-bg); padding: 1rem; border-radius: 6px; overflow-x: auto; margin: 0.5rem 0; }
a { color: var(--primary); }
.muted { color: var(--text-muted); }
.endpoint { margin: 0.3rem 0; }
</style>
</head>
<body>
<h1>diVine Handle Gateway</h1>
<p class="muted">ATProto handle provisioning and account linking service</p>

<h2>Endpoints</h2>
<div class="endpoint"><code>POST</code> <code>/api/account-links/opt-in</code> &mdash; Opt-in to ATProto bridge (authenticated)</div>
<div class="endpoint"><code>POST</code> <code>/api/account-links/provision</code> &mdash; Provision ATProto account (authenticated)</div>
<div class="endpoint"><code>GET</code> <code>/api/account-links/:pubkey/status</code> &mdash; Check provisioning status (authenticated)</div>
<div class="endpoint"><code>GET</code> <code>/.well-known/atproto-did</code> &mdash; DID resolution</div>
<div class="endpoint"><code>GET</code> <code>/health</code> &mdash; Health check</div>

<p class="muted" style="margin-top: 1rem;">All API endpoints require internal authentication.</p>

<h2>Links</h2>
<p><a href="https://divine.video">divine.video</a> &middot; <a href="https://github.com/nicobao/divine">GitHub</a></p>

<hr style="border-color: var(--border); margin-top: 2rem;">
<p class="muted" style="font-size: 0.85em;">Powered by diVine</p>
</body>
</html>"#;

async fn root_info() -> Html<&'static str> {
    Html(ROOT_HTML)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn health_ready() -> StatusCode {
    StatusCode::OK
}
