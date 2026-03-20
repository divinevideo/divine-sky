use anyhow::{Context, Result};
use divine_atbridge::config::BridgeConfig;
use divine_atbridge::health;
use divine_atbridge::runtime::run_service_with_state;

#[tokio::main]
async fn main() -> Result<()> {
    let config =
        BridgeConfig::from_env().context("failed to load bridge configuration from environment")?;

    tracing_subscriber::fmt().with_target(false).init();
    let runtime_state = health::RuntimeHealthState::new();
    let _health = health::spawn(config.clone(), runtime_state.clone()).await?;
    run_service_with_state(&config, runtime_state).await
}
