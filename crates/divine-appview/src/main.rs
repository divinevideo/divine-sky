use std::net::SocketAddr;

use divine_appview::app_from_config;
use divine_appview::config::AppviewConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = AppviewConfig::from_env()?;
    let bind_addr: SocketAddr = config.bind_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app_from_config(config)).await?;
    Ok(())
}
