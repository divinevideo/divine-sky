//! Post Text Builder — converts NIP-71 Nostr events into ATProto post text + facets.
//!
//! ATProto `feed.post.text` has a 300-grapheme limit. Facet byte offsets are computed
//! on UTF-8 byte positions. Hashtags produce `app.bsky.richtext.facet#tag` facets.

use divine_bridge_types::NostrEvent;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A rich-text facet (matches ATProto `app.bsky.richtext.facet`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facet {
    pub index: FacetIndex,
    pub features: Vec<FacetFeature>,
}

/// Byte-offset range within UTF-8 text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetIndex {
    #[serde(rename = "byteStart")]
    pub byte_start: usize,
    #[serde(rename = "byteEnd")]
    pub byte_end: usize,
}

/// The feature attached to a facet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum FacetFeature {
    #[serde(rename = "app.bsky.richtext.facet#tag")]
    Tag { tag: String },
}

// ---------------------------------------------------------------------------
// Helpers for extracting tags from NostrEvent
// ---------------------------------------------------------------------------

/// Get the first value for a single-letter or named tag (e.g. `"title"`, `"t"`).
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

// ---------------------------------------------------------------------------
// Grapheme truncation
// ---------------------------------------------------------------------------

/// Truncate a string to at most `max` grapheme clusters.
fn truncate_graphemes(s: &str, max: usize) -> String {
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    if graphemes.len() <= max {
        s.to_string()
    } else {
        graphemes[..max].concat()
    }
}

// ---------------------------------------------------------------------------
// Facet computation
// ---------------------------------------------------------------------------

/// Scan `text` for `#hashtag` tokens and return a facet for each one.
/// Byte offsets are computed on the UTF-8 representation of `text`.
fn compute_hashtag_facets(text: &str) -> Vec<Facet> {
    let mut facets = Vec::new();
    let mut search_start = 0;

    while search_start < text.len() {
        // Find next '#'
        let Some(hash_pos) = text[search_start..].find('#') else {
            break;
        };
        let byte_start = search_start + hash_pos;

        // '#' must be at start of text or preceded by whitespace
        if byte_start > 0 {
            let prev_char = text[..byte_start].chars().last().unwrap();
            if !prev_char.is_whitespace() {
                search_start = byte_start + 1;
                continue;
            }
        }

        // Collect the tag body (everything after '#' until whitespace or end)
        let after_hash = &text[byte_start + 1..];
        let tag_len = after_hash
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after_hash.len());

        if tag_len == 0 {
            search_start = byte_start + 1;
            continue;
        }

        let byte_end = byte_start + 1 + tag_len;
        let tag_value = &text[byte_start + 1..byte_end];

        facets.push(Facet {
            index: FacetIndex {
                byte_start,
                byte_end,
            },
            features: vec![FacetFeature::Tag {
                tag: tag_value.to_string(),
            }],
        });

        search_start = byte_end;
    }

    facets
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build ATProto post text and facets from a NIP-71 Nostr event.
///
/// Returns `(text, facets)` where `text` is at most 300 grapheme clusters and
/// `facets` carry UTF-8 byte offsets for each hashtag.
pub fn build_post_text(event: &NostrEvent) -> (String, Vec<Facet>) {
    let title = get_tag(event, "title").unwrap_or_default();
    let summary = get_tag(event, "summary").unwrap_or_default();
    let description = if !summary.is_empty() {
        summary
    } else if !event.content.is_empty() {
        &event.content
    } else {
        ""
    };

    let hashtags: Vec<String> = get_tags(event, "t")
        .iter()
        .map(|t| format!("#{}", t))
        .collect();
    let hashtag_str = hashtags.join(" ");

    // Build candidate text: title, then description, then hashtags
    let candidate = match (title.is_empty(), description.is_empty()) {
        (false, false) => format!("{}\n\n{}", title, description),
        (false, true) => title.to_string(),
        (true, false) => description.to_string(),
        (true, true) => String::new(),
    };

    let full_text = if !hashtag_str.is_empty() && !candidate.is_empty() {
        format!("{}\n\n{}", candidate, hashtag_str)
    } else if !hashtag_str.is_empty() {
        hashtag_str
    } else {
        candidate
    };

    // Truncate to 300 graphemes
    let text = truncate_graphemes(&full_text, 300);

    // Compute facets for hashtags that survived truncation
    let facets = compute_hashtag_facets(&text);

    (text, facets)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use divine_bridge_types::NostrEvent;

    /// Helper to create a minimal NostrEvent for testing.
    fn make_event(
        content: &str,
        tags: Vec<Vec<&str>>,
    ) -> NostrEvent {
        NostrEvent {
            id: String::new(),
            pubkey: String::new(),
            created_at: 0,
            kind: 34235,
            tags: tags
                .into_iter()
                .map(|t| t.into_iter().map(String::from).collect())
                .collect(),
            content: content.to_string(),
            sig: String::new(),
        }
    }

    #[test]
    fn title_description_and_hashtags() {
        let event = make_event(
            "",
            vec![
                vec!["title", "My Cool Video"],
                vec!["summary", "A short description"],
                vec!["t", "sunset"],
                vec!["t", "nature"],
            ],
        );

        let (text, facets) = build_post_text(&event);

        assert_eq!(
            text,
            "My Cool Video\n\nA short description\n\n#sunset #nature"
        );
        assert!(text.graphemes(true).count() <= 300);

        // Two hashtag facets
        assert_eq!(facets.len(), 2);

        // #sunset
        let sunset_start = text.find("#sunset").unwrap();
        assert_eq!(facets[0].index.byte_start, sunset_start);
        assert_eq!(facets[0].index.byte_end, sunset_start + "#sunset".len());
        assert_eq!(
            facets[0].features[0],
            FacetFeature::Tag {
                tag: "sunset".to_string()
            }
        );

        // #nature
        let nature_start = text.find("#nature").unwrap();
        assert_eq!(facets[1].index.byte_start, nature_start);
        assert_eq!(facets[1].index.byte_end, nature_start + "#nature".len());
        assert_eq!(
            facets[1].features[0],
            FacetFeature::Tag {
                tag: "nature".to_string()
            }
        );
    }

    #[test]
    fn title_only_no_facets() {
        let event = make_event("", vec![vec!["title", "Just a Title"]]);

        let (text, facets) = build_post_text(&event);

        assert_eq!(text, "Just a Title");
        assert!(facets.is_empty());
    }

    #[test]
    fn long_content_truncated_to_300_graphemes() {
        // Create content that exceeds 300 graphemes
        let long_description = "a".repeat(350);
        let event = make_event(
            &long_description,
            vec![vec!["title", "Title"]],
        );

        let (text, _facets) = build_post_text(&event);

        assert_eq!(text.graphemes(true).count(), 300);
        // Title (5) + \n\n (2) + remaining = 300 → description part = 293 chars
        assert!(text.starts_with("Title\n\n"));
    }

    #[test]
    fn empty_content_empty_result() {
        let event = make_event("", vec![]);

        let (text, facets) = build_post_text(&event);

        assert_eq!(text, "");
        assert!(facets.is_empty());
    }

    #[test]
    fn unicode_grapheme_counting_and_byte_offsets() {
        // Emoji: wave is 4 bytes, 1 grapheme
        let event = make_event(
            "",
            vec![
                vec!["title", "\u{1F44B} Hello"],  // 👋 Hello
                vec!["t", "wave"],
            ],
        );

        let (text, facets) = build_post_text(&event);

        assert_eq!(text, "\u{1F44B} Hello\n\n#wave");

        // Verify grapheme count: 👋(1) + space(1) + H(1)e(1)l(1)l(1)o(1) + \n(1)\n(1) + #(1)w(1)a(1)v(1)e(1) = 14
        assert_eq!(text.graphemes(true).count(), 14);

        // Verify byte offsets: 👋 is 4 bytes
        // "👋 Hello\n\n#wave"
        //  4  1 5    2 = byte 12 for '#'
        let hash_pos = text.find("#wave").unwrap();
        assert_eq!(hash_pos, 12); // 4 + 1 + 5 + 2 = 12
        assert_eq!(facets.len(), 1);
        assert_eq!(facets[0].index.byte_start, 12);
        assert_eq!(facets[0].index.byte_end, 12 + "#wave".len()); // 17
        assert_eq!(
            facets[0].features[0],
            FacetFeature::Tag {
                tag: "wave".to_string()
            }
        );
    }

    #[test]
    fn multiple_hashtags_correct_byte_positions() {
        let event = make_event(
            "",
            vec![
                vec!["t", "a"],
                vec!["t", "bb"],
                vec!["t", "ccc"],
            ],
        );

        let (text, facets) = build_post_text(&event);

        assert_eq!(text, "#a #bb #ccc");
        assert_eq!(facets.len(), 3);

        // #a at byte 0..2
        assert_eq!(facets[0].index.byte_start, 0);
        assert_eq!(facets[0].index.byte_end, 2);

        // #bb at byte 3..6
        assert_eq!(facets[1].index.byte_start, 3);
        assert_eq!(facets[1].index.byte_end, 6);

        // #ccc at byte 7..11
        assert_eq!(facets[2].index.byte_start, 7);
        assert_eq!(facets[2].index.byte_end, 11);
    }

    #[test]
    fn content_used_when_no_summary() {
        let event = make_event(
            "Fallback content",
            vec![vec!["title", "Title"]],
        );

        let (text, _) = build_post_text(&event);

        assert_eq!(text, "Title\n\nFallback content");
    }

    #[test]
    fn summary_preferred_over_content() {
        let event = make_event(
            "Content ignored",
            vec![
                vec!["title", "Title"],
                vec!["summary", "Summary wins"],
            ],
        );

        let (text, _) = build_post_text(&event);

        assert_eq!(text, "Title\n\nSummary wins");
    }

    #[test]
    fn facet_serialization_matches_atproto_schema() {
        let facet = Facet {
            index: FacetIndex {
                byte_start: 0,
                byte_end: 5,
            },
            features: vec![FacetFeature::Tag {
                tag: "test".to_string(),
            }],
        };

        let json = serde_json::to_value(&facet).unwrap();
        assert_eq!(json["index"]["byteStart"], 0);
        assert_eq!(json["index"]["byteEnd"], 5);
        assert_eq!(
            json["features"][0]["$type"],
            "app.bsky.richtext.facet#tag"
        );
        assert_eq!(json["features"][0]["tag"], "test");
    }
}
