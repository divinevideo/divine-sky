use std::net::SocketAddr;

use divine_labeler::config::LabelerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = LabelerConfig::from_env()?;
    LabelerConfig::validate_signing_key(&config.signing_key_hex)?;

    tracing::info!(
        did = %config.labeler_did,
        port = config.port,
        "divine-labeler starting"
    );

    let addr: SocketAddr = ([0, 0, 0, 0], config.port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("listening on {addr}");

    Ok(())
}
