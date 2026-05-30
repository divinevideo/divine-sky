use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use diesel::PgConnection;
use divine_bridge_db::models::{AccountLinkLifecycleRow, NewPublishJob};
use divine_bridge_db::{
    cancel_publish_job, enqueue_publish_job, list_accounts_requiring_backfill,
    mark_account_backfill_completed, mark_account_backfill_failed, mark_account_backfill_started,
};
use divine_bridge_types::{PublishJobSource, PublishState};

use crate::config::DEFAULT_BACKFILL_BATCH_SIZE;
use crate::nostr_consumer::{author_history_filter, collect_history_until_eose, RelayConnection};
use crate::pipeline::{BlobFetcher, BlobUploader, BridgePipeline, PdsPublisher, QueueDecision};

#[async_trait]
pub trait BackfillRelayConnector: Send + Sync {
    type Connection: RelayConnection;

    async fn connect(&self, relay_url: &str) -> Result<Self::Connection>;
}

pub struct BackfillPlanner<A, R, F, U, P, C> {
    relay_url: String,
    connection: Arc<Mutex<PgConnection>>,
    pipeline: Arc<BridgePipeline<A, R, F, U, P>>,
    relay_connector: C,
    batch_size: i64,
}

impl<A, R, F, U, P, C> BackfillPlanner<A, R, F, U, P, C>
where
    A: crate::pipeline::AccountStore,
    R: crate::pipeline::RecordStore,
    F: BlobFetcher,
    U: BlobUploader,
    P: PdsPublisher,
    C: BackfillRelayConnector,
{
    pub fn new(
        relay_url: String,
        connection: Arc<Mutex<PgConnection>>,
        pipeline: Arc<BridgePipeline<A, R, F, U, P>>,
        relay_connector: C,
        batch_size: i64,
    ) -> Self {
        Self {
            relay_url,
            connection,
            pipeline,
            relay_connector,
            batch_size,
        }
    }

    pub fn with_default_batch_size(
        relay_url: String,
        connection: Arc<Mutex<PgConnection>>,
        pipeline: Arc<BridgePipeline<A, R, F, U, P>>,
        relay_connector: C,
    ) -> Self {
        Self::new(
            relay_url,
            connection,
            pipeline,
            relay_connector,
            DEFAULT_BACKFILL_BATCH_SIZE,
        )
    }

    pub async fn run_once(&self) -> Result<()> {
        let accounts = {
            let mut connection = self.connection.lock().unwrap();
            list_accounts_requiring_backfill(&mut connection, self.batch_size)?
        };

        for account in accounts {
            let relay = self.relay_connector.connect(&self.relay_url).await;
            let mut relay = match relay {
                Ok(relay) => relay,
                Err(error) => {
                    let mut connection = self.connection.lock().unwrap();
                    mark_account_backfill_failed(
                        &mut connection,
                        &account.nostr_pubkey,
                        &error.to_string(),
                    )?;
                    continue;
                }
            };

            let result = self.replay_account_history(&mut relay, &account).await;
            let _ = relay.close().await;
            if let Err(error) = result {
                let mut connection = self.connection.lock().unwrap();
                mark_account_backfill_failed(
                    &mut connection,
                    &account.nostr_pubkey,
                    &error.to_string(),
                )?;
            }
        }

        Ok(())
    }

    async fn replay_account_history<RC>(
        &self,
        relay: &mut RC,
        account: &AccountLinkLifecycleRow,
    ) -> Result<()>
    where
        RC: RelayConnection,
    {
        {
            let mut connection = self.connection.lock().unwrap();
            mark_account_backfill_started(&mut connection, &account.nostr_pubkey)?;
        }

        let subscription_id = "sub-1".to_string();
        let mut history = collect_history_until_eose(
            relay,
            &subscription_id,
            &author_history_filter(account.nostr_pubkey.clone()),
        )
        .await
        .with_context(|| format!("failed to load relay history for {}", account.nostr_pubkey))?;

        history.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });

        for event in history {
            match self.pipeline.prepare_publish_job(&event).await? {
                QueueDecision::Enqueue(job) => {
                    let queued = new_backfill_job(&job)?;
                    let mut connection = self.connection.lock().unwrap();
                    enqueue_publish_job(&mut connection, &queued)?;
                }
                QueueDecision::Cancel { tombstone_job, .. } => {
                    let tombstone = new_backfill_job(&tombstone_job)?;
                    let mut connection = self.connection.lock().unwrap();
                    cancel_publish_job(
                        &mut connection,
                        &tombstone,
                        Some("historical delete replay"),
                    )?;
                }
                QueueDecision::Skip { .. } => {}
            }
        }

        let mut connection = self.connection.lock().unwrap();
        mark_account_backfill_completed(&mut connection, &account.nostr_pubkey)?;
        Ok(())
    }
}

fn new_backfill_job(envelope: &crate::pipeline::PublishJobEnvelope) -> Result<NewPublishJob<'_>> {
    let event_created_at = Utc
        .timestamp_opt(envelope.event_created_at, 0)
        .single()
        .context("queued event timestamp is out of range")?;

    Ok(NewPublishJob {
        nostr_event_id: &envelope.nostr_event_id,
        nostr_pubkey: &envelope.nostr_pubkey,
        event_created_at,
        event_payload: envelope.event_payload.clone(),
        job_source: PublishJobSource::Backfill.as_str(),
        state: PublishState::Pending.as_str(),
    })
}
