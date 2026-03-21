use divine_video_worker::derivatives::{derive_media_view, MediaAsset};

fn fake_mp4_asset() -> MediaAsset {
    MediaAsset {
        did: "did:plc:ebt5msdpfavoklkap6gl54bm".to_string(),
        blob_cid: "bafkrei-demo".to_string(),
        mime_type: "video/mp4".to_string(),
        bytes: 42,
    }
}

#[tokio::test]
async fn derive_media_view_creates_playlist_and_thumbnail_urls() {
    let view = derive_media_view("https://media.divine.test", fake_mp4_asset())
        .await
        .unwrap();

    assert!(view.playlist_url.ends_with(".m3u8"));
    assert!(view.thumbnail_url.as_deref().unwrap().ends_with(".jpg"));
    assert!(view.ready);
}
