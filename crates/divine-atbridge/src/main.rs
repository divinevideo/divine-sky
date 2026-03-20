use anyhow::{Context, Result};
use divine_atbridge::config::BridgeConfig;
use divine_atbridge::runtime::run_service;

#[tokio::main]
async fn main() -> Result<()> {
    let config =
        BridgeConfig::from_env().context("failed to load bridge configuration from environment")?;

    tracing_subscriber::fmt().with_target(false).init();
    run_service(&config).await
}
