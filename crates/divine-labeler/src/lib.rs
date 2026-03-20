use axum::body::Body;
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::response::Html;
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

async fn root_info(State(state): State<AppState>) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>diVine ATProto Labeler</title>
<style>
:root {{
    --primary: #27C58B;
    --primary-dark: #1fa06f;
    --bg-dark: #1b1b1b;
    --bg-section: #2d2d2d;
    --text: #ffffff;
    --text-muted: #999999;
    --border: #3d3d3d;
    --code-bg: #41444e;
}}
body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg-dark);
    color: var(--text);
    line-height: 1.6;
    max-width: 700px;
    margin: 0 auto;
    padding: 2rem;
}}
h1 {{ color: var(--primary); margin-bottom: 0.5rem; }}
h2 {{ color: var(--primary); margin-top: 2rem; font-size: 1.2rem; }}
code {{ background: var(--code-bg); padding: 2px 6px; border-radius: 3px; font-size: 0.9em; }}
pre {{ background: var(--code-bg); padding: 1rem; border-radius: 6px; overflow-x: auto; margin: 0.5rem 0; }}
a {{ color: var(--primary); }}
.muted {{ color: var(--text-muted); }}
.endpoint {{ margin: 0.3rem 0; }}
</style>
</head>
<body>
<h1>diVine ATProto Labeler</h1>
<p class="muted">Content moderation labeler service for ATProto</p>
<p><strong>DID:</strong> <code>{did}</code></p>

<h2>Endpoints</h2>
<div class="endpoint"><code>GET</code> <code>/xrpc/com.atproto.label.queryLabels</code> &mdash; Query labels</div>
<div class="endpoint"><code>POST</code> <code>/webhook/moderation-result</code> &mdash; Webhook (authenticated)</div>
<div class="endpoint"><code>GET</code> <code>/health</code> &mdash; Health check</div>

<h2>Links</h2>
<p><a href="https://atproto.com/specs/label">ATProto Label Spec</a> &middot; <a href="https://divine.video">divine.video</a></p>

<hr style="border-color: var(--border); margin-top: 2rem;">
<p class="muted" style="font-size: 0.85em;">Powered by diVine</p>
</body>
</html>"#,
        did = state.labeler_did
    ))
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
        .route("/", get(root_info))
        .route("/health", get(routes::health::handler))
        .route("/health/ready", get(routes::health::handler))
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
