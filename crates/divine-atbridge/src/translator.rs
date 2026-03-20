//! NIP-71 → ATProto Record Translator
//!
//! Translates verified NIP-71 Nostr events into ATProto `app.bsky.feed.post`
//! records with `app.bsky.embed.video` embeds.

use crate::text_builder::{self, Facet};
use anyhow::Result;
use divine_bridge_types::{BlobRef, NostrEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// ATProto record types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtProtoPost {
    #[serde(rename = "$type")]
    pub type_: String, // "app.bsky.feed.post"
    pub text: String,
    #[serde(rename = "createdAt")]
    pub created_at: String, // ISO 8601
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub facets: Vec<Facet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub langs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<VideoEmbed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<SelfLabels>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEmbed {
    #[serde(rename = "$type")]
    pub type_: String, // "app.bsky.embed.video"
    pub video: BlobRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(rename = "aspectRatio", skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<AspectRatio>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfLabels {
    #[serde(rename = "$type")]
    pub type_: String, // "com.atproto.label.defs#selfLabels"
    pub values: Vec<SelfLabelValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfLabelValue {
    pub val: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get the first value for a given tag name.
fn get_tag<'a>(event: &'a NostrEvent, name: &str) -> Option<&'a str> {
    event
        .tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == name)
        .map(|t| t[1].as_str())
}

/// Get all values for a given tag name.
fn get_tags<'a>(event: &'a NostrEvent, name: &str) -> Vec<&'a str> {
    event
        .tags
        .iter()
        .filter(|t| t.len() >= 2 && t[0] == name)
        .map(|t| t[1].as_str())
        .collect()
}

/// Parse an `imeta` tag to extract a named field.
/// imeta tags look like: ["imeta", "url https://...", "dim 1920x1080", ...]
fn get_imeta_field<'a>(event: &'a NostrEvent, field: &str) -> Option<&'a str> {
    for tag in &event.tags {
        if tag.first().map(|s| s.as_str()) == Some("imeta") {
            for entry in &tag[1..] {
                if let Some(val) = entry
                    .strip_prefix(field)
                    .and_then(|rest| rest.strip_prefix(' '))
                {
                    return Some(val);
                }
            }
        }
    }
    None
}

/// Compute GCD of two numbers.
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Parse a "WxH" dimension string into an AspectRatio with GCD reduction.
fn parse_aspect_ratio(dim: &str) -> Option<AspectRatio> {
    let (w_str, h_str) = dim.split_once('x')?;
    let w: u32 = w_str.parse().ok()?;
    let h: u32 = h_str.parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    let g = gcd(w, h);
    Some(AspectRatio {
        width: w / g,
        height: h / g,
    })
}

/// Convert a Unix timestamp to ISO 8601 string.
fn unix_to_iso8601(ts: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

/// Characters valid in an ATProto rkey.
fn is_valid_rkey_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '.' | '_'
                | '~'
                | ':'
                | '@'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | '-'
        )
}

/// Check if a string is a valid ATProto rkey.
fn is_valid_rkey(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && s.chars().all(is_valid_rkey_char)
}

/// Base32 encode (lowercase, no padding) a SHA-256 hash of the input.
fn base32_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let hash = hasher.finalize();
    // Use RFC 4648 base32 lowercase, no padding, truncated to fit rkey limits
    let encoded = data_encoding::BASE32_NOPAD.encode(&hash);
    encoded.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Derive an rkey for a Nostr event, using the `d` tag if it's a valid rkey,
/// otherwise falling back to a base32-encoded hash.
pub fn derive_rkey(event: &NostrEvent) -> String {
    if let Some(d_tag) = get_tag(event, "d") {
        if !d_tag.is_empty() && is_valid_rkey(d_tag) {
            return d_tag.to_string();
        }
        if !d_tag.is_empty() {
            // d tag exists but has invalid chars — hash it
            return base32_hash(d_tag);
        }
    }
    // No d tag or empty d tag — use event id
    if !event.id.is_empty() && is_valid_rkey(&event.id) {
        return event.id.clone();
    }
    base32_hash(&event.id)
}

/// Translate a NIP-71 Nostr event into an ATProto post record.
pub fn translate_nip71_to_post(event: &NostrEvent, blob_ref: &BlobRef) -> Result<AtProtoPost> {
    let (text, facets) = text_builder::build_post_text(event);
    let created_at = unix_to_iso8601(event.created_at);

    // Video embed
    let alt = get_tag(event, "alt").map(String::from);
    let aspect_ratio = get_imeta_field(event, "dim").and_then(parse_aspect_ratio);

    let embed = Some(VideoEmbed {
        type_: "app.bsky.embed.video".to_string(),
        video: blob_ref.clone(),
        alt,
        aspect_ratio,
    });

    // Content warning → self-labels
    let labels = {
        let cw_values: Vec<&str> = get_tags(event, "content-warning");
        if cw_values.is_empty() {
            None
        } else {
            Some(SelfLabels {
                type_: "com.atproto.label.defs#selfLabels".to_string(),
                values: cw_values
                    .into_iter()
                    .map(|v| SelfLabelValue { val: v.to_string() })
                    .collect(),
            })
        }
    };

    // Default language
    let langs = Some(vec!["en".to_string()]);

    Ok(AtProtoPost {
        type_: "app.bsky.feed.post".to_string(),
        text,
        created_at,
        facets,
        langs,
        embed,
        labels,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(content: &str, tags: Vec<Vec<&str>>) -> NostrEvent {
        NostrEvent {
            id: "abc123def456".to_string(),
            pubkey: "pubkey123".to_string(),
            created_at: 1700000000,
            kind: 34235,
            tags: tags
                .into_iter()
                .map(|t| t.into_iter().map(String::from).collect())
                .collect(),
            content: content.to_string(),
            sig: "sig123".to_string(),
        }
    }

    fn make_blob_ref() -> BlobRef {
        BlobRef::new(
            "bafkreiexample".to_string(),
            "video/mp4".to_string(),
            1024000,
        )
    }

    #[test]
    fn full_nip71_event_translation() {
        let event = make_event(
            "",
            vec![
                vec!["title", "Sunset Timelapse"],
                vec!["t", "sunset"],
                vec!["t", "nature"],
                vec![
                    "imeta",
                    "url https://example.com/video.mp4",
                    "dim 1080x1920",
                    "x abc123hash",
                ],
                vec!["alt", "A beautiful sunset timelapse video"],
            ],
        );
        let blob = make_blob_ref();

        let post = translate_nip71_to_post(&event, &blob).unwrap();

        assert_eq!(post.type_, "app.bsky.feed.post");
        assert!(post.text.contains("Sunset Timelapse"));
        assert!(post.text.contains("#sunset"));
        assert!(post.text.contains("#nature"));
        assert_eq!(post.created_at, "2023-11-14T22:13:20Z");

        // Facets for hashtags
        assert_eq!(post.facets.len(), 2);

        // Video embed
        let embed = post.embed.as_ref().unwrap();
        assert_eq!(embed.type_, "app.bsky.embed.video");
        assert_eq!(embed.video.cid(), "bafkreiexample");
        assert_eq!(
            embed.alt.as_deref(),
            Some("A beautiful sunset timelapse video")
        );

        // Aspect ratio: 1080x1920 → GCD 120 → 9x16
        let ar = embed.aspect_ratio.as_ref().unwrap();
        assert_eq!(ar.width, 9);
        assert_eq!(ar.height, 16);

        // Labels: none
        assert!(post.labels.is_none());
    }

    #[test]
    fn content_warning_produces_self_labels() {
        let event = make_event(
            "",
            vec![
                vec!["title", "NSFW Video"],
                vec!["content-warning", "nudity"],
            ],
        );
        let blob = make_blob_ref();

        let post = translate_nip71_to_post(&event, &blob).unwrap();

        let labels = post.labels.as_ref().unwrap();
        assert_eq!(labels.type_, "com.atproto.label.defs#selfLabels");
        assert_eq!(labels.values.len(), 1);
        assert_eq!(labels.values[0].val, "nudity");
    }

    #[test]
    fn d_tag_used_as_rkey() {
        let event = make_event("", vec![vec!["d", "my-video-slug"]]);
        assert_eq!(derive_rkey(&event), "my-video-slug");
    }

    #[test]
    fn d_tag_with_invalid_chars_falls_back_to_hash() {
        let event = make_event("", vec![vec!["d", "invalid/rkey/with/slashes"]]);
        let rkey = derive_rkey(&event);
        // Should not contain slashes
        assert!(!rkey.contains('/'));
        // Should be valid
        assert!(is_valid_rkey(&rkey));
    }

    #[test]
    fn empty_d_tag_falls_back_to_event_id() {
        let event = make_event("", vec![vec!["d", ""]]);
        // Falls back to event.id which is "abc123def456" — valid rkey
        assert_eq!(derive_rkey(&event), "abc123def456");
    }

    #[test]
    fn no_d_tag_falls_back_to_event_id() {
        let event = make_event("", vec![]);
        assert_eq!(derive_rkey(&event), "abc123def456");
    }

    #[test]
    fn aspect_ratio_gcd_reduction() {
        assert_eq!(
            parse_aspect_ratio("1080x1920"),
            Some(AspectRatio {
                width: 9,
                height: 16
            })
        );
        assert_eq!(
            parse_aspect_ratio("1920x1080"),
            Some(AspectRatio {
                width: 16,
                height: 9
            })
        );
        assert_eq!(
            parse_aspect_ratio("640x480"),
            Some(AspectRatio {
                width: 4,
                height: 3
            })
        );
        assert_eq!(parse_aspect_ratio("0x100"), None);
        assert_eq!(parse_aspect_ratio("invalid"), None);
    }

    #[test]
    fn serialization_matches_atproto_schema() {
        let event = make_event(
            "",
            vec![
                vec!["title", "Test"],
                vec!["imeta", "url https://example.com/v.mp4", "dim 1920x1080"],
                vec!["alt", "Test alt"],
                vec!["t", "test"],
                vec!["content-warning", "graphic"],
            ],
        );
        let blob = make_blob_ref();

        let post = translate_nip71_to_post(&event, &blob).unwrap();
        let json = serde_json::to_value(&post).unwrap();

        // Top-level fields
        assert_eq!(json["$type"], "app.bsky.feed.post");
        assert!(json["createdAt"].is_string());
        assert!(json["langs"].is_array());

        // Embed structure
        assert_eq!(json["embed"]["$type"], "app.bsky.embed.video");
        assert_eq!(json["embed"]["alt"], "Test alt");
        assert_eq!(json["embed"]["aspectRatio"]["width"], 16);
        assert_eq!(json["embed"]["aspectRatio"]["height"], 9);

        // Blob ref matches ATProto schema
        assert_eq!(json["embed"]["video"]["$type"], "blob");
        assert_eq!(json["embed"]["video"]["ref"]["$link"], "bafkreiexample");
        assert_eq!(json["embed"]["video"]["mimeType"], "video/mp4");
        assert_eq!(json["embed"]["video"]["size"], 1024000);

        // Labels structure
        assert_eq!(json["labels"]["$type"], "com.atproto.label.defs#selfLabels");
        assert_eq!(json["labels"]["values"][0]["val"], "graphic");

        // Facets
        assert!(json["facets"].is_array());
        assert_eq!(
            json["facets"][0]["features"][0]["$type"],
            "app.bsky.richtext.facet#tag"
        );
    }

    #[test]
    fn unix_timestamp_conversion() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_iso8601(1700000000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn rkey_validation() {
        assert!(is_valid_rkey("simple-rkey"));
        assert!(is_valid_rkey("with.dots_and~tilde"));
        assert!(!is_valid_rkey("has/slash"));
        assert!(!is_valid_rkey("has space"));
        assert!(!is_valid_rkey(""));
        // 513-char string should be invalid
        let long = "a".repeat(513);
        assert!(!is_valid_rkey(&long));
        // 512-char string should be valid
        let max = "a".repeat(512);
        assert!(is_valid_rkey(&max));
    }
}
