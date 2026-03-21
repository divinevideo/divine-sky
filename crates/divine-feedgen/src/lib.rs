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
* { box-sizing: border-box; }
body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg-dark);
    color: var(--text);
    line-height: 1.6;
    max-width: 760px;
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
.card {
    background: var(--bg-section);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    margin-top: 1.5rem;
}
.card h2 { margin-top: 0; }
.form-row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
    align-items: center;
    flex-wrap: wrap;
}
.form-row label {
    min-width: 120px;
    color: var(--text-muted);
    font-size: 0.9em;
}
.form-row input {
    flex: 1;
    background: var(--bg-dark);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    padding: 0.4rem 0.6rem;
    font-size: 0.9em;
    min-width: 0;
}
.form-row input:focus {
    outline: none;
    border-color: var(--primary);
}
.btn {
    background: var(--primary);
    color: #000;
    border: none;
    border-radius: 4px;
    padding: 0.45rem 1.1rem;
    font-size: 0.9em;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s;
}
.btn:hover { background: var(--primary-dark); }
.btn:disabled { opacity: 0.6; cursor: default; }
.result-item {
    background: var(--bg-dark);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.6rem 1rem;
    margin-bottom: 0.4rem;
    font-size: 0.88em;
    word-break: break-all;
    color: var(--text-muted);
}
.result-item .post-uri { color: var(--primary); }
.status-msg { color: var(--text-muted); font-size: 0.9em; margin-top: 0.5rem; }
.error-msg { color: #e05555; font-size: 0.9em; margin-top: 0.5rem; }
.describe-feed {
    background: var(--bg-dark);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem 1rem;
    margin-bottom: 0.5rem;
    font-size: 0.88em;
}
.describe-feed .feed-name { color: var(--primary); font-weight: 600; }
.describe-feed .feed-uri { color: var(--text-muted); word-break: break-all; }
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

<div class="card">
  <h2>Describe Feed Generator</h2>
  <div id="describe-status" class="status-msg">Loading...</div>
  <div id="describe-results"></div>
</div>

<div class="card">
  <h2>Query Feed</h2>
  <div class="form-row">
    <label for="feed-input">Feed URI</label>
    <input id="feed-input" type="text" value="at://did:plc:divine.feed/app.bsky.feed.generator/latest">
  </div>
  <div class="form-row">
    <label for="cursor-input">Cursor (optional)</label>
    <input id="cursor-input" type="text" placeholder="leave blank for first page">
  </div>
  <div style="display:flex; gap:0.5rem; align-items:center; flex-wrap:wrap;">
    <button class="btn" id="fetch-btn" onclick="fetchFeed()">Fetch</button>
    <button class="btn" id="next-btn" style="display:none; background:var(--bg-dark); border:1px solid var(--primary); color:var(--primary);" onclick="fetchNext()">Next Page</button>
    <span class="status-msg" id="feed-status"></span>
  </div>
  <div id="feed-results" style="margin-top:1rem;"></div>
</div>

<hr style="border-color: var(--border); margin-top: 2rem;">
<p class="muted" style="font-size: 0.85em;">Powered by diVine</p>

<script>
let lastCursor = null;

// Auto-load describe on page load
window.addEventListener('DOMContentLoaded', async () => {
  const statusEl = document.getElementById('describe-status');
  const resultsEl = document.getElementById('describe-results');
  try {
    const resp = await fetch('/xrpc/app.bsky.feed.describeFeedGenerator');
    if (!resp.ok) {
      statusEl.textContent = 'Failed to load feed metadata (HTTP ' + resp.status + ')';
      statusEl.className = 'error-msg';
      return;
    }
    const data = await resp.json();
    statusEl.textContent = 'DID: ' + (data.did || 'unknown');
    (data.feeds || []).forEach(feed => {
      const div = document.createElement('div');
      div.className = 'describe-feed';
      div.innerHTML = '<div class="feed-name">' + (feed.displayName || '') + '</div>'
        + '<div class="feed-uri">' + (feed.uri || '') + '</div>'
        + (feed.description ? '<div style="margin-top:0.3rem;color:var(--text)">' + feed.description + '</div>' : '');
      resultsEl.appendChild(div);
    });
  } catch (e) {
    statusEl.textContent = 'Error: ' + e.message;
    statusEl.className = 'error-msg';
  }
});

async function fetchFeed() {
  const feedUri = document.getElementById('feed-input').value.trim();
  const cursor = document.getElementById('cursor-input').value.trim();
  if (!feedUri) {
    setFeedStatus('Enter a feed URI.', true);
    return;
  }
  lastCursor = null;
  document.getElementById('feed-results').innerHTML = '';
  document.getElementById('next-btn').style.display = 'none';
  await doFetch(feedUri, cursor || null);
}

async function fetchNext() {
  const feedUri = document.getElementById('feed-input').value.trim();
  if (!feedUri || !lastCursor) return;
  await doFetch(feedUri, lastCursor);
}

function setFeedStatus(msg, isError) {
  const el = document.getElementById('feed-status');
  el.textContent = msg;
  el.className = isError ? 'error-msg' : 'status-msg';
}

async function doFetch(feedUri, cursor) {
  const btn = document.getElementById('fetch-btn');
  btn.disabled = true;
  setFeedStatus('Fetching...', false);

  let url = '/xrpc/app.bsky.feed.getFeedSkeleton?feed=' + encodeURIComponent(feedUri) + '&limit=20';
  if (cursor) url += '&cursor=' + encodeURIComponent(cursor);

  try {
    const resp = await fetch(url);
    if (!resp.ok) {
      const text = await resp.text();
      setFeedStatus('Error ' + resp.status + ': ' + text, true);
      return;
    }
    const data = await resp.json();
    const items = data.feed || [];
    lastCursor = data.cursor || null;

    const container = document.getElementById('feed-results');
    if (items.length === 0 && !cursor) {
      container.innerHTML = '<p class="status-msg">No feed items found.</p>';
    } else {
      items.forEach(item => {
        const div = document.createElement('div');
        div.className = 'result-item';
        div.innerHTML = '<span class="post-uri">' + (item.post || '') + '</span>'
          + (item.reason ? ' <em style="color:var(--text-muted)">(' + item.reason.$type + ')</em>' : '');
        container.appendChild(div);
      });
    }

    if (lastCursor) {
      document.getElementById('next-btn').style.display = 'inline-block';
      setFeedStatus('Showing ' + items.length + ' items. Cursor available for next page.', false);
    } else {
      document.getElementById('next-btn').style.display = 'none';
      setFeedStatus('Showing ' + items.length + ' item(s).', false);
    }
  } catch (e) {
    setFeedStatus('Fetch error: ' + e.message, true);
  } finally {
    btn.disabled = false;
  }
}
</script>
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
