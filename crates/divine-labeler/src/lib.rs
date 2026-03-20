use axum::body::Body;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
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
