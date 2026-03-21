use anyhow::{ensure, Result};

pub const MAX_ATPROTO_VIDEO_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedVideo {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub bytes: u64,
}

pub fn prepare_publishable_video(data: &[u8], mime_type: &str) -> Result<PreparedVideo> {
    let bytes = data.len() as u64;
    ensure!(
        bytes <= MAX_ATPROTO_VIDEO_BYTES,
        "video exceeds ATProto 100 MB limit"
    );

    let normalized_mime = normalize_video_mime(data, mime_type)
        .ok_or_else(|| anyhow::anyhow!("video must be MP4 before publish"))?;

    Ok(PreparedVideo {
        data: data.to_vec(),
        mime_type: normalized_mime.to_string(),
        bytes,
    })
}

fn normalize_video_mime(data: &[u8], mime_type: &str) -> Option<&'static str> {
    if mime_type.eq_ignore_ascii_case("video/mp4") {
        return Some("video/mp4");
    }

    if looks_like_mp4(data) {
        return Some("video/mp4");
    }

    None
}

fn looks_like_mp4(data: &[u8]) -> bool {
    data.len() >= 8 && &data[4..8] == b"ftyp"
}
