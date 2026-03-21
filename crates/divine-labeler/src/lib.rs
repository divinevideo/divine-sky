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
* {{ box-sizing: border-box; }}
body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg-dark);
    color: var(--text);
    line-height: 1.6;
    max-width: 760px;
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
.card {{
    background: var(--bg-section);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    margin-top: 1.5rem;
}}
.card h2 {{ margin-top: 0; }}
.form-row {{
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
    align-items: center;
    flex-wrap: wrap;
}}
.form-row label {{
    min-width: 120px;
    color: var(--text-muted);
    font-size: 0.9em;
}}
.form-row input {{
    flex: 1;
    background: var(--bg-dark);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    padding: 0.4rem 0.6rem;
    font-size: 0.9em;
    min-width: 0;
}}
.form-row input:focus {{
    outline: none;
    border-color: var(--primary);
}}
.btn {{
    background: var(--primary);
    color: #000;
    border: none;
    border-radius: 4px;
    padding: 0.45rem 1.1rem;
    font-size: 0.9em;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s;
}}
.btn:hover {{ background: var(--primary-dark); }}
.btn:disabled {{ opacity: 0.6; cursor: default; }}
#results {{
    margin-top: 1rem;
}}
.result-label {{
    background: var(--bg-dark);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem 1rem;
    margin-bottom: 0.5rem;
    font-size: 0.88em;
    line-height: 1.5;
}}
.result-label .label-val {{
    color: var(--primary);
    font-weight: 600;
}}
.result-label .label-uri {{
    color: var(--text-muted);
    word-break: break-all;
}}
.status-msg {{
    color: var(--text-muted);
    font-size: 0.9em;
    margin-top: 0.5rem;
}}
.error-msg {{
    color: #e05555;
    font-size: 0.9em;
    margin-top: 0.5rem;
}}
.cursor-row {{
    margin-top: 0.75rem;
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex-wrap: wrap;
}}
.cursor-row span {{
    font-size: 0.85em;
    color: var(--text-muted);
}}
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

<div class="card">
  <h2>Query Labels</h2>
  <div class="form-row">
    <label for="uri-input">URI Pattern</label>
    <input id="uri-input" type="text" placeholder="at://did:plc:* or https://divine.video/*" value="*">
  </div>
  <div class="form-row">
    <label for="cursor-input">Cursor (optional)</label>
    <input id="cursor-input" type="text" placeholder="leave blank for first page">
  </div>
  <div style="display:flex; gap:0.5rem; align-items:center; flex-wrap:wrap;">
    <button class="btn" id="search-btn" onclick="queryLabels()">Search</button>
    <button class="btn" id="next-btn" style="display:none; background:var(--bg-dark); border:1px solid var(--primary); color:var(--primary);" onclick="queryNext()">Next Page</button>
    <span class="status-msg" id="status-msg"></span>
  </div>
  <div id="results"></div>
</div>

<hr style="border-color: var(--border); margin-top: 2rem;">
<p class="muted" style="font-size: 0.85em;">Powered by diVine</p>

<script>
let lastCursor = null;

function setStatus(msg, isError) {{
  const el = document.getElementById('status-msg');
  el.textContent = msg;
  el.className = isError ? 'error-msg' : 'status-msg';
}}

async function queryLabels() {{
  const uri = document.getElementById('uri-input').value.trim();
  const cursor = document.getElementById('cursor-input').value.trim();
  if (!uri) {{ setStatus('Enter a URI pattern.', true); return; }}
  lastCursor = null;
  document.getElementById('results').innerHTML = '';
  document.getElementById('next-btn').style.display = 'none';
  await doQuery(uri, cursor || null);
}}

async function queryNext() {{
  const uri = document.getElementById('uri-input').value.trim();
  if (!uri || !lastCursor) return;
  await doQuery(uri, lastCursor);
}}

async function doQuery(uri, cursor) {{
  const btn = document.getElementById('search-btn');
  btn.disabled = true;
  setStatus('Searching...', false);

  let url = `/xrpc/com.atproto.label.queryLabels?uriPatterns=${{encodeURIComponent(uri)}}&limit=20`;
  if (cursor) url += `&cursor=${{encodeURIComponent(cursor)}}`;

  try {{
    const resp = await fetch(url);
    if (!resp.ok) {{
      const text = await resp.text();
      setStatus(`Error ${{resp.status}}: ${{text}}`, true);
      return;
    }}
    const data = await resp.json();
    const labels = data.labels || [];
    lastCursor = data.cursor || null;

    const container = document.getElementById('results');
    if (labels.length === 0 && !cursor) {{
      container.innerHTML = '<p class="status-msg">No labels found.</p>';
    }} else {{
      labels.forEach(lbl => {{
        const div = document.createElement('div');
        div.className = 'result-label';
        div.innerHTML = `
          <div><span class="label-val">${{lbl.val || '(unlabeled)'}}</span>
            ${{lbl.neg ? ' <em style="color:var(--text-muted)">(negation)</em>' : ''}}
          </div>
          <div class="label-uri">${{lbl.uri || ''}}</div>
          ${{lbl.src ? `<div style="color:var(--text-muted);font-size:0.85em;">src: ${{lbl.src}}</div>` : ''}}
          ${{lbl.cts ? `<div style="color:var(--text-muted);font-size:0.85em;">created: ${{lbl.cts}}</div>` : ''}}
        `;
        container.appendChild(div);
      }});
    }}

    if (lastCursor) {{
      document.getElementById('next-btn').style.display = 'inline-block';
      setStatus(`Showing ${{labels.length}} labels. Cursor available for next page.`, false);
    }} else {{
      document.getElementById('next-btn').style.display = 'none';
      setStatus(`Showing ${{labels.length}} label(s).`, false);
    }}
  }} catch (e) {{
    setStatus(`Fetch error: ${{e.message}}`, true);
  }} finally {{
    btn.disabled = false;
  }}
}}
</script>
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
