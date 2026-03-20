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
