use divine_appview_indexer::config::IndexerConfig;
use divine_appview_indexer::pds_client::HttpPdsClient;
use divine_appview_indexer::relay::NoopRelayStream;
use divine_appview_indexer::store::DbStore;
use divine_appview_indexer::sync::{backfill_from_pds, run_single_event_loop};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let config = IndexerConfig::from_env()?;
    tracing::info!(relay_url = %config.relay_url, "starting divine appview indexer");

    let pds = HttpPdsClient::new(config.pds_base_url);
    let store = DbStore::new(config.database_url);

    backfill_from_pds(&pds, &store).await?;

    if !config.oneshot {
        let mut relay = NoopRelayStream;
        run_single_event_loop(&mut relay, &pds, &store).await?;
    }

    Ok(())
}
