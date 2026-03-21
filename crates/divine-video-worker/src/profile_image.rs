use anyhow::{ensure, Result};

pub const MAX_PROFILE_IMAGE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileImageKind {
    Avatar,
    Banner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedProfileImage {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub bytes: u64,
}

pub fn prepare_profile_image(
    data: &[u8],
    mime_type: &str,
    kind: ProfileImageKind,
) -> Result<PreparedProfileImage> {
    let bytes = data.len() as u64;
    ensure!(
        bytes <= MAX_PROFILE_IMAGE_BYTES,
        "{} image exceeds ATProto 1 MB limit",
        kind_name(kind)
    );

    let normalized_mime = normalize_profile_image_mime(data, mime_type)
        .ok_or_else(|| anyhow::anyhow!("profile images must be PNG or JPEG"))?;

    Ok(PreparedProfileImage {
        data: data.to_vec(),
        mime_type: normalized_mime.to_string(),
        bytes,
    })
}

fn kind_name(kind: ProfileImageKind) -> &'static str {
    match kind {
        ProfileImageKind::Avatar => "avatar",
        ProfileImageKind::Banner => "banner",
    }
}

fn normalize_profile_image_mime(data: &[u8], mime_type: &str) -> Option<&'static str> {
    if mime_type.eq_ignore_ascii_case("image/png") {
        return Some("image/png");
    }
    if mime_type.eq_ignore_ascii_case("image/jpeg") || mime_type.eq_ignore_ascii_case("image/jpg") {
        return Some("image/jpeg");
    }
    if looks_like_png(data) {
        return Some("image/png");
    }
    if looks_like_jpeg(data) {
        return Some("image/jpeg");
    }
    None
}

fn looks_like_png(data: &[u8]) -> bool {
    data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

fn looks_like_jpeg(data: &[u8]) -> bool {
    data.starts_with(&[0xFF, 0xD8, 0xFF])
}
