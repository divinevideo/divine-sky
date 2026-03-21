use anyhow::Result;
use chrono::Utc;

use crate::pds_client::PdsSource;
use crate::relay::RelayStream;
use crate::store::{AppviewStore, MediaJob};

pub async fn backfill_from_pds<P, S>(pds: &P, store: &S) -> Result<()>
where
    P: PdsSource,
    S: AppviewStore,
{
    for repo in pds.list_repos().await? {
        sync_repo_from_pds(pds, store, &repo.did).await?;
    }

    let now = Utc::now().to_rfc3339();
    store
        .set_service_state("appview_last_backfill", Some(&now))
        .await?;
    Ok(())
}

pub async fn sync_repo_from_pds<P, S>(pds: &P, store: &S, did: &str) -> Result<()>
where
    P: PdsSource,
    S: AppviewStore,
{
    let snapshot = pds.sync_repo(did).await?;

    store.upsert_repo(snapshot.repo).await?;

    if let Some(profile) = snapshot.profile {
        store.upsert_profile(profile).await?;
    }

    for post in snapshot.posts {
        if let Some(blob_cid) = post.embed_blob_cid.clone() {
            store
                .queue_media_job(MediaJob {
                    did: post.did.clone(),
                    blob_cid,
                })
                .await?;
        }
        store.upsert_post(post).await?;
    }

    Ok(())
}

pub async fn run_single_event_loop<R, P, S>(relay: &mut R, pds: &P, store: &S) -> Result<()>
where
    R: RelayStream,
    P: PdsSource,
    S: AppviewStore,
{
    if let Some(did) = relay.next_changed_repo().await? {
        sync_repo_from_pds(pds, store, &did).await?;
        store
            .set_service_state("appview_last_relay_did", Some(&did))
            .await?;
    }

    Ok(())
}
