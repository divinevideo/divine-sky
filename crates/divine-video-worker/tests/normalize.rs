use divine_video_worker::normalize::prepare_publishable_video;
use divine_video_worker::profile_image::{prepare_profile_image, ProfileImageKind};

fn fake_mp4_bytes() -> Vec<u8> {
    let mut bytes = vec![0x00, 0x00, 0x00, 0x18];
    bytes.extend_from_slice(b"ftypisom");
    bytes.extend_from_slice(b"mp42");
    bytes
}

fn fake_webm_bytes() -> Vec<u8> {
    vec![0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81]
}

fn fake_png_bytes() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    ]
}

#[test]
fn mp4_payload_is_accepted_and_canonicalized() {
    let prepared =
        prepare_publishable_video(&fake_mp4_bytes(), "application/octet-stream").unwrap();

    assert_eq!(prepared.mime_type, "video/mp4");
    assert_eq!(prepared.data, fake_mp4_bytes());
}

#[test]
fn non_mp4_payload_is_rejected_before_publish() {
    let err = prepare_publishable_video(&fake_webm_bytes(), "video/webm").unwrap_err();

    assert!(err.to_string().contains("MP4"), "got: {err:#}");
}

#[test]
fn png_profile_images_are_accepted() {
    let prepared = prepare_profile_image(
        &fake_png_bytes(),
        "application/octet-stream",
        ProfileImageKind::Avatar,
    )
    .unwrap();

    assert_eq!(prepared.mime_type, "image/png");
    assert_eq!(prepared.bytes, fake_png_bytes().len() as u64);
}

#[test]
fn oversized_profile_images_are_rejected() {
    let err = prepare_profile_image(
        &vec![0xFF; (1024 * 1024) + 1],
        "image/jpeg",
        ProfileImageKind::Banner,
    )
    .unwrap_err();

    assert!(err.to_string().contains("1 MB"), "got: {err:#}");
}
