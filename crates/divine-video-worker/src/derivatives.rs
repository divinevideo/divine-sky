use anyhow::{Context, Result};
use diesel::{Connection, PgConnection};
use divine_bridge_db::models::NewAppviewMediaView;
use divine_bridge_db::upsert_appview_media_view;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAsset {
    pub did: String,
    pub blob_cid: String,
    pub mime_type: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedMediaView {
    pub did: String,
    pub blob_cid: String,
    pub playlist_url: String,
    pub thumbnail_url: Option<String>,
    pub mime_type: String,
    pub bytes: u64,
    pub ready: bool,
}

pub async fn derive_media_view(base_url: &str, asset: MediaAsset) -> Result<DerivedMediaView> {
    let base_url = base_url.trim_end_matches('/');
    let did_path = asset.did.replace(':', "/");

    Ok(DerivedMediaView {
        playlist_url: format!("{base_url}/playlists/{did_path}/{}.m3u8", asset.blob_cid),
        thumbnail_url: Some(format!(
            "{base_url}/thumbnails/{did_path}/{}.jpg",
            asset.blob_cid
        )),
        ready: true,
        did: asset.did,
        blob_cid: asset.blob_cid,
        mime_type: asset.mime_type,
        bytes: asset.bytes,
    })
}

pub fn persist_media_view(database_url: &str, view: &DerivedMediaView) -> Result<()> {
    let mut conn =
        PgConnection::establish(database_url).context("failed to connect for media view write")?;

    upsert_appview_media_view(
        &mut conn,
        &NewAppviewMediaView {
            did: &view.did,
            blob_cid: &view.blob_cid,
            playlist_url: &view.playlist_url,
            thumbnail_url: view.thumbnail_url.as_deref(),
            mime_type: &view.mime_type,
            bytes: view.bytes as i64,
            ready: view.ready,
            last_derived_at: Some(chrono::Utc::now()),
        },
    )?;

    Ok(())
}
