mod skeleton;

use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use axum::{http::StatusCode, Json, Router};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FeedQuery {
    feed: String,
}

async fn describe_feed_generator() -> Json<skeleton::DescribeFeedGeneratorResponse> {
    Json(skeleton::describe_feed_generator())
}

async fn get_feed_skeleton(
    Query(query): Query<FeedQuery>,
) -> Result<Json<skeleton::FeedSkeletonResponse>, axum::http::StatusCode> {
    skeleton::feed_skeleton(&query.feed)
        .map(Json)
        .map_err(|_| axum::http::StatusCode::NOT_FOUND)
}

const ROOT_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>diVine Feed Generator</title>
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
<h1>diVine Feed Generator</h1>
<p class="muted">ATProto feed generator for diVine video content</p>

<h2>Endpoints</h2>
<div class="endpoint"><code>GET</code> <code>/xrpc/app.bsky.feed.describeFeedGenerator</code> &mdash; Feed metadata</div>
<div class="endpoint"><code>GET</code> <code>/xrpc/app.bsky.feed.getFeedSkeleton</code> &mdash; Feed skeleton</div>
<div class="endpoint"><code>GET</code> <code>/health</code> &mdash; Health check</div>
<div class="endpoint"><code>GET</code> <code>/health/ready</code> &mdash; Readiness check</div>

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

pub fn app() -> Router {
    Router::new()
        .route("/", get(root_info))
        .route("/health", get(health))
        .route("/health/ready", get(health_ready))
        .route(
            "/xrpc/app.bsky.feed.describeFeedGenerator",
            get(describe_feed_generator),
        )
        .route(
            "/xrpc/app.bsky.feed.getFeedSkeleton",
            get(get_feed_skeleton),
        )
}
