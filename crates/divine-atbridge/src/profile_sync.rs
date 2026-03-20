//! Nostr kind 0 -> ATProto profile record translation.

use anyhow::{Context, Result};
use divine_bridge_types::{BlobRef, NostrEvent};
use serde_json::{Map, Value};

pub const PROFILE_COLLECTION: &str = "app.bsky.actor.profile";
pub const PROFILE_RKEY: &str = "self";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileAssets {
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedProfile {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub website: Option<String>,
}

pub fn parse_kind0_profile(event: &NostrEvent) -> Result<ParsedProfile> {
    let value: Value =
        serde_json::from_str(&event.content).context("kind 0 content must be valid JSON")?;
    let obj = value
        .as_object()
        .context("kind 0 content must be a JSON object")?;

    let display_name = get_string(obj, &["display_name", "displayName", "name"]);
    let description = get_string(obj, &["about", "bio", "description"]);
    let avatar_url = get_string(obj, &["picture", "image", "avatar"]);
    let banner_url = get_string(obj, &["banner"]);
    let website = get_string(obj, &["website", "url"]);

    Ok(ParsedProfile {
        display_name,
        description,
        avatar_url,
        banner_url,
        website,
    })
}

pub fn build_profile_record(
    parsed: &ParsedProfile,
    avatar: Option<BlobRef>,
    banner: Option<BlobRef>,
) -> Value {
    let mut record = serde_json::json!({
        "$type": PROFILE_COLLECTION,
    });

    if let Some(obj) = record.as_object_mut() {
        if let Some(display_name) = &parsed.display_name {
            obj.insert(
                "displayName".to_string(),
                Value::String(display_name.clone()),
            );
        }
        if let Some(description) = profile_description(parsed) {
            obj.insert("description".to_string(), Value::String(description));
        }
        if let Some(avatar) = avatar {
            obj.insert("avatar".to_string(), serde_json::to_value(avatar).unwrap());
        }
        if let Some(banner) = banner {
            obj.insert("banner".to_string(), serde_json::to_value(banner).unwrap());
        }
    }

    record
}

pub fn profile_assets(parsed: &ParsedProfile) -> ProfileAssets {
    ProfileAssets {
        avatar_url: parsed.avatar_url.clone(),
        banner_url: parsed.banner_url.clone(),
    }
}

fn get_string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| obj.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn profile_description(parsed: &ParsedProfile) -> Option<String> {
    match (&parsed.description, &parsed.website) {
        (Some(description), Some(website)) => Some(format!("{description}\n\nWebsite: {website}")),
        (Some(description), None) => Some(description.clone()),
        (None, Some(website)) => Some(format!("Website: {website}")),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind0_event(content: &str) -> NostrEvent {
        NostrEvent {
            id: "event-0".to_string(),
            pubkey: "pubkey".to_string(),
            created_at: 1_700_000_000,
            kind: 0,
            tags: vec![],
            content: content.to_string(),
            sig: "sig".to_string(),
        }
    }

    #[test]
    fn parse_kind0_profile_accepts_common_fields() {
        let parsed = parse_kind0_profile(&kind0_event(
            r#"{
                "display_name":"DiVine",
                "about":"Short bio",
                "picture":"https://cdn.example/avatar.png",
                "banner":"https://cdn.example/banner.png",
                "website":"https://divine.video"
            }"#,
        ))
        .unwrap();

        assert_eq!(parsed.display_name.as_deref(), Some("DiVine"));
        assert_eq!(parsed.description.as_deref(), Some("Short bio"));
        assert_eq!(
            parsed.avatar_url.as_deref(),
            Some("https://cdn.example/avatar.png")
        );
        assert_eq!(
            parsed.banner_url.as_deref(),
            Some("https://cdn.example/banner.png")
        );
        assert_eq!(parsed.website.as_deref(), Some("https://divine.video"));
    }

    #[test]
    fn build_profile_record_appends_website_to_description() {
        let parsed = ParsedProfile {
            display_name: Some("DiVine".to_string()),
            description: Some("Short bio".to_string()),
            avatar_url: None,
            banner_url: None,
            website: Some("https://divine.video".to_string()),
        };

        let record = build_profile_record(&parsed, None, None);
        assert_eq!(record["$type"], PROFILE_COLLECTION);
        assert_eq!(record["displayName"], "DiVine");
        assert_eq!(
            record["description"],
            "Short bio\n\nWebsite: https://divine.video"
        );
    }
}
