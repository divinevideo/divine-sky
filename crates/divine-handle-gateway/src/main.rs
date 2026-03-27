use std::net::SocketAddr;

use divine_handle_gateway::app_with_config;
use divine_handle_gateway::keycast_client::KeycastClient;
use divine_handle_gateway::name_server_client::NameServerClient;
use divine_handle_gateway::provision_runner::{ProvisionRunner, ProvisioningClient};
use divine_handle_gateway::store::DbStore;
use divine_handle_gateway::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = AppConfig::from_env()?;
    let router = app_with_config(config.clone())?;

    let store = DbStore::connect(&config.database_url)?;
    let provision_runner = ProvisionRunner::new(
        store,
        ProvisioningClient::new(
            config.atproto_provisioning_url.clone(),
            config.atproto_provisioning_token.clone(),
        ),
        NameServerClient::new(
            config.atproto_name_server_sync_url.clone(),
            config.atproto_name_server_sync_token.clone(),
        ),
        KeycastClient::new(
            config.atproto_keycast_sync_url.clone(),
            config.keycast_atproto_token.clone(),
        ),
    );
    let replayed = provision_runner
        .replay_pending_from_database(&config.database_url)
        .await?;
    if replayed > 0 {
        tracing::info!(replayed, "replayed pending provisioning rows at startup");
    }

    let reconciled = provision_runner
        .reconcile_existing_from_database(&config.database_url)
        .await?;
    if reconciled > 0 {
        tracing::info!(
            reconciled,
            "reconciled existing lifecycle rows at startup"
        );
    }

    let addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
