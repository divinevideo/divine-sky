use anyhow::{Context, Result};
use divine_atbridge::config::BridgeConfig;
use divine_atbridge::health;
use divine_atbridge::runtime::run_service_with_state;

#[tokio::main]
async fn main() -> Result<()> {
    let config =
        BridgeConfig::from_env().context("failed to load bridge configuration from environment")?;

    tracing_subscriber::fmt().with_target(false).init();

    // Apply bridge-owned schema migrations before anything touches the database.
    // The bridge DB has no external migration tooling, so the binary self-migrates
    // on startup with idempotent SQL (safe against any existing schema state).
    divine_bridge_db::run_pending_migrations(&config.database_url)
        .context("failed to apply bridge database migrations on startup")?;
    tracing::info!("bridge database migrations applied");

    let runtime_state = health::RuntimeHealthState::new();
    let _health = health::spawn(config.clone(), runtime_state.clone()).await?;
    run_service_with_state(&config, runtime_state).await
}
