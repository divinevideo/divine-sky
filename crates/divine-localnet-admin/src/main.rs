use std::net::SocketAddr;

use divine_localnet_admin::{app_with_config, AppConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = AppConfig::from_env()?;
    let router = app_with_config(config)?;

    let addr: SocketAddr = std::env::var("LOCALNET_ADMIN_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
