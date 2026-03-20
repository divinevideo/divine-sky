//! Deletion handler — processes Nostr kind-5 deletion events.
//!
//! When a Nostr deletion event is received, this module looks up the
//! corresponding AT Protocol record via `RecordMappingStore`, deletes
//! it from the PDS, and marks the mapping as deleted.

use anyhow::{Context, Result};
use async_trait::async_trait;
use divine_bridge_types::NostrEvent;

use crate::publisher::PdsClient;

// ---------------------------------------------------------------------------
// Record mapping store trait
// ---------------------------------------------------------------------------

/// A mapping entry linking a Nostr event to an AT Protocol record.
#[derive(Debug, Clone)]
pub struct RecordMapping {
    pub nostr_event_id: String,
    pub at_uri: String,
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub status: String,
}

/// Trait for looking up and updating record mappings.
///
/// Implementations back onto a database; tests use a mock.
#[async_trait]
pub trait RecordMappingStore: Send + Sync {
    /// Find a mapping by the Nostr event ID.
    async fn find_by_nostr_event_id(&self, event_id: &str) -> Result<Option<RecordMapping>>;

    /// Update the status of a mapping (e.g. to "deleted").
    async fn update_status(&self, nostr_event_id: &str, status: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Deletion handler
// ---------------------------------------------------------------------------

/// Extract the event ID being deleted from a kind-5 deletion event's `e` tag.
fn get_deleted_event_id(event: &NostrEvent) -> Option<&str> {
    event
        .tags
        .iter()
        .find(|t| t.len() >= 2 && t[0] == "e")
        .map(|t| t[1].as_str())
}

/// Handle a Nostr kind-5 deletion event.
///
/// 1. Extracts the referenced event ID from the `e` tag.
/// 2. Looks up the AT Protocol record mapping.
/// 3. Deletes the record from the PDS.
/// 4. Updates the mapping status to "deleted".
pub async fn handle_deletion(
    event: &NostrEvent,
    pds_client: &PdsClient,
    db: &dyn RecordMappingStore,
) -> Result<()> {
    let target_event_id = get_deleted_event_id(event)
        .context("deletion event has no 'e' tag referencing an event")?;

    let mapping = db
        .find_by_nostr_event_id(target_event_id)
        .await
        .context("failed to look up record mapping")?
        .context("no record mapping found for deleted event")?;

    if mapping.status == "deleted" {
        tracing::info!(
            nostr_event_id = target_event_id,
            "record already deleted, skipping"
        );
        return Ok(());
    }

    pds_client
        .delete_record(&mapping.did, &mapping.collection, &mapping.rkey)
        .await
        .context("failed to delete record from PDS")?;

    db.update_status(target_event_id, "deleted")
        .await
        .context("failed to update mapping status to deleted")?;

    tracing::info!(
        nostr_event_id = target_event_id,
        at_uri = mapping.at_uri,
        "successfully deleted record"
    );

    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Mock implementation of `RecordMappingStore`.
    struct MockStore {
        mappings: Vec<RecordMapping>,
        updated: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl MockStore {
        fn new(mappings: Vec<RecordMapping>) -> Self {
            Self {
                mappings,
                updated: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn updated_statuses(&self) -> Vec<(String, String)> {
            self.updated.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RecordMappingStore for MockStore {
        async fn find_by_nostr_event_id(&self, event_id: &str) -> Result<Option<RecordMapping>> {
            Ok(self
                .mappings
                .iter()
                .find(|m| m.nostr_event_id == event_id)
                .cloned())
        }

        async fn update_status(&self, nostr_event_id: &str, status: &str) -> Result<()> {
            self.updated
                .lock()
                .unwrap()
                .push((nostr_event_id.to_string(), status.to_string()));
            Ok(())
        }
    }

    fn make_deletion_event(target_event_id: &str) -> NostrEvent {
        NostrEvent {
            id: "del-event-id".to_string(),
            pubkey: "pubkey123".to_string(),
            created_at: 1700000100,
            kind: 5,
            tags: vec![vec!["e".to_string(), target_event_id.to_string()]],
            content: String::new(),
            sig: "sig-del".to_string(),
        }
    }

    fn make_mapping(event_id: &str) -> RecordMapping {
        RecordMapping {
            nostr_event_id: event_id.to_string(),
            at_uri: format!(
                "at://did:plc:abc123/app.bsky.feed.post/{}",
                event_id
            ),
            did: "did:plc:abc123".to_string(),
            collection: "app.bsky.feed.post".to_string(),
            rkey: event_id.to_string(),
            status: "published".to_string(),
        }
    }

    #[tokio::test]
    async fn deletion_handler_looks_up_mapping_and_calls_delete() {
        // Set up mock PDS server
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.repo.deleteRecord")
            .match_body(mockito::Matcher::JsonString(
                serde_json::json!({
                    "repo": "did:plc:abc123",
                    "collection": "app.bsky.feed.post",
                    "rkey": "original-event-123"
                })
                .to_string(),
            ))
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let pds_client = PdsClient::new(server.url(), "tok");
        let store = MockStore::new(vec![make_mapping("original-event-123")]);
        let event = make_deletion_event("original-event-123");

        handle_deletion(&event, &pds_client, &store).await.unwrap();

        mock.assert_async().await;
        let updated = store.updated_statuses();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, "original-event-123");
        assert_eq!(updated[0].1, "deleted");
    }

    #[tokio::test]
    async fn deletion_handler_returns_error_when_no_e_tag() {
        let server = mockito::Server::new_async().await;
        // No mock needed — should fail before HTTP call
        let pds_client = PdsClient::new(server.url(), "tok");
        let store = MockStore::new(vec![]);

        let event = NostrEvent {
            id: "del-no-e".to_string(),
            pubkey: "pk".to_string(),
            created_at: 1700000100,
            kind: 5,
            tags: vec![],
            content: String::new(),
            sig: "sig".to_string(),
        };

        let err = handle_deletion(&event, &pds_client, &store)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no 'e' tag"));
    }

    #[tokio::test]
    async fn deletion_handler_returns_error_when_no_mapping() {
        let server = mockito::Server::new_async().await;
        let pds_client = PdsClient::new(server.url(), "tok");
        let store = MockStore::new(vec![]); // empty

        let event = make_deletion_event("nonexistent-event");

        let err = handle_deletion(&event, &pds_client, &store)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no record mapping found"));
    }

    #[tokio::test]
    async fn deletion_handler_skips_already_deleted() {
        let server = mockito::Server::new_async().await;
        let pds_client = PdsClient::new(server.url(), "tok");

        let mut mapping = make_mapping("already-deleted-event");
        mapping.status = "deleted".to_string();
        let store = MockStore::new(vec![mapping]);

        let event = make_deletion_event("already-deleted-event");

        // Should succeed without calling PDS
        handle_deletion(&event, &pds_client, &store)
            .await
            .unwrap();

        // No status updates should have happened
        assert!(store.updated_statuses().is_empty());
    }
}
