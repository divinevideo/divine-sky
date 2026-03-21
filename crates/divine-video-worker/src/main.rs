use divine_video_worker::derivatives::{derive_media_view, persist_media_view, MediaAsset};

fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let Some(database_url) = std::env::var("DATABASE_URL").ok() else {
        return;
    };
    let Some(media_base_url) = std::env::var("APPVIEW_MEDIA_BASE_URL").ok() else {
        return;
    };
    let Some(did) = std::env::var("APPVIEW_MEDIA_DID").ok() else {
        return;
    };
    let Some(blob_cid) = std::env::var("APPVIEW_MEDIA_BLOB_CID").ok() else {
        return;
    };

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should build");
    let view = runtime
        .block_on(derive_media_view(
            &media_base_url,
            MediaAsset {
                did,
                blob_cid,
                mime_type: "video/mp4".to_string(),
                bytes: 0,
            },
        ))
        .expect("media view derivation should succeed");

    persist_media_view(&database_url, &view).expect("media view persistence should succeed");
}
