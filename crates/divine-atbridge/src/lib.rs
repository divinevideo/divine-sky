//! Divine ATBridge library surface.

pub mod backfill_planner;
pub mod config;
pub mod deletion;
pub mod health;
pub mod nostr_consumer;
pub mod pds_accounts;
pub mod pds_host_backfill;
pub mod pipeline;
pub mod plc_directory;
pub mod profile_sync;
pub mod provision_runtime;
pub mod provisioner;
pub mod publisher;
pub mod runtime;
pub mod signature;
pub mod text_builder;
pub mod translator;
pub mod video_service;

use anyhow::{Context, Result};
use nostr_consumer::{
    parse_relay_message, NostrConsumer, NostrFilter, RelayConnection, RelayMessage,
};
use pipeline::{
    AccountStore, BlobFetcher, BlobUploader, BridgePipeline, PdsPublisher, QueueDecision,
    RecordStore,
};

pub fn runtime_filter() -> NostrFilter {
    NostrFilter {
        kinds: vec![0, 5, 34235, 34236],
        authors: None,
        since: None,
    }
}

pub async fn run_bridge_session<C, A, R, F, U, P>(
    consumer: &mut NostrConsumer,
    conn: &mut C,
    pipeline: &BridgePipeline<A, R, F, U, P>,
) -> Result<()>
where
    C: RelayConnection,
    A: AccountStore,
    R: RecordStore,
    F: BlobFetcher,
    U: BlobUploader,
    P: PdsPublisher,
{
    let req = consumer.build_req(&runtime_filter());
    conn.send(req)
        .await
        .context("failed to send subscription")?;

    while let Some(raw) = conn.recv().await.context("failed to read relay frame")? {
        match parse_relay_message(&raw) {
            Ok(RelayMessage::Event { event, .. }) => {
                let created_at = event.created_at;
                let result = match pipeline.prepare_publish_job(&event).await {
                    Ok(QueueDecision::Enqueue(job)) => {
                        pipeline.execute_publish_job(&job).await.map(|_| ())
                    }
                    Ok(QueueDecision::Cancel { tombstone_job, .. }) => pipeline
                        .execute_publish_job(&tombstone_job)
                        .await
                        .map(|_| ()),
                    Ok(QueueDecision::Skip { .. }) => Ok(()),
                    Err(error) => Err(error),
                };

                match result {
                    Ok(()) => {
                        consumer.last_seen_timestamp = Some(created_at);
                    }
                    Err(error) => {
                        tracing::error!(
                            error = %error,
                            event_id = %event.id,
                            "bridge pipeline rejected relay event"
                        );
                    }
                }
            }
            Ok(RelayMessage::Eose { .. }) => {}
            Ok(RelayMessage::Notice(message)) => {
                tracing::warn!("relay NOTICE: {message}");
            }
            Ok(RelayMessage::Unknown(_)) => {}
            Err(error) => {
                tracing::warn!(error = %error, "failed to parse relay frame");
            }
        }
    }

    Ok(())
}
