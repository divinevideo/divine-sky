//! Nostr relay consumer — subscribes to relay events via WebSocket.
//!
//! Uses a trait-based abstraction (`RelayConnection`) so unit tests can
//! inject a mock WebSocket without hitting a real relay.

use anyhow::{anyhow, Context, Result};
use divine_bridge_types::NostrEvent;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

// ---------------------------------------------------------------------------
// Nostr protocol types
// ---------------------------------------------------------------------------

/// Filter used when subscribing to a relay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NostrFilter {
    pub kinds: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<i64>,
}

impl NostrFilter {
    /// Build the NIP-71 video + deletion filter.
    pub fn nip71_video() -> Self {
        Self {
            kinds: vec![34235, 34236, 5],
            authors: None,
            since: None,
        }
    }
}

/// Messages the relay can send us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayMessage {
    Event {
        subscription_id: String,
        event: NostrEvent,
    },
    Eose {
        subscription_id: String,
    },
    Notice(String),
    Unknown(String),
}

/// Parse a raw JSON relay message into a `RelayMessage`.
pub fn parse_relay_message(raw: &str) -> Result<RelayMessage> {
    let value: serde_json::Value = serde_json::from_str(raw).context("invalid JSON from relay")?;

    let arr = value
        .as_array()
        .ok_or_else(|| anyhow!("expected JSON array"))?;

    let msg_type = arr
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing message type"))?;

    match msg_type {
        "EVENT" => {
            let sub_id = arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing subscription id"))?
                .to_string();
            let event: NostrEvent = serde_json::from_value(
                arr.get(2)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing event object"))?,
            )
            .context("invalid event object")?;
            Ok(RelayMessage::Event {
                subscription_id: sub_id,
                event,
            })
        }
        "EOSE" => {
            let sub_id = arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing subscription id in EOSE"))?
                .to_string();
            Ok(RelayMessage::Eose {
                subscription_id: sub_id,
            })
        }
        "NOTICE" => {
            let msg = arr
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Ok(RelayMessage::Notice(msg))
        }
        _ => Ok(RelayMessage::Unknown(raw.to_string())),
    }
}

// ---------------------------------------------------------------------------
// WebSocket abstraction
// ---------------------------------------------------------------------------

/// Trait abstracting a WebSocket connection to a Nostr relay.
///
/// Implementations must be `Send` so they can be used across await points
/// inside a tokio task.
#[async_trait::async_trait]
pub trait RelayConnection: Send {
    /// Send a text message to the relay.
    async fn send(&mut self, msg: String) -> Result<()>;
    /// Receive the next text message from the relay. Returns `None` when the
    /// connection is closed.
    async fn recv(&mut self) -> Result<Option<String>>;
    /// Close the connection.
    async fn close(&mut self) -> Result<()>;
}

pub struct WebSocketRelayConnection {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl WebSocketRelayConnection {
    pub async fn connect(relay_url: &str) -> Result<Self> {
        let (stream, _) = connect_async(relay_url)
            .await
            .with_context(|| format!("failed to connect to relay {relay_url}"))?;
        Ok(Self { stream })
    }
}

#[async_trait::async_trait]
impl RelayConnection for WebSocketRelayConnection {
    async fn send(&mut self, msg: String) -> Result<()> {
        self.stream
            .send(Message::Text(msg))
            .await
            .context("failed to send websocket message")?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<String>> {
        while let Some(message) = self.stream.next().await {
            match message.context("failed to receive websocket frame")? {
                Message::Text(text) => return Ok(Some(text)),
                Message::Binary(bytes) => {
                    let text = String::from_utf8(bytes.to_vec())
                        .context("binary relay frame was not utf-8")?;
                    return Ok(Some(text));
                }
                Message::Ping(payload) => {
                    self.stream
                        .send(Message::Pong(payload))
                        .await
                        .context("failed to reply to relay ping")?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Ok(None),
                Message::Frame(_) => {}
            }
        }

        Ok(None)
    }

    async fn close(&mut self) -> Result<()> {
        self.stream
            .close(None)
            .await
            .context("failed to close relay websocket")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// NostrConsumer
// ---------------------------------------------------------------------------

/// Consumes events from a Nostr relay and dispatches them via a callback.
pub struct NostrConsumer {
    pub relay_url: String,
    /// Timestamp of the last successfully processed event (unix seconds).
    pub last_seen_timestamp: Option<i64>,
    /// Subscription ID used for the current session.
    subscription_id: String,
}

impl NostrConsumer {
    pub fn new(relay_url: String) -> Self {
        Self {
            relay_url,
            last_seen_timestamp: None,
            subscription_id: "divine-bridge-0".to_string(),
        }
    }

    /// Build the REQ message JSON for the given filter.
    pub fn build_req(&self, filter: &NostrFilter) -> String {
        let mut f = filter.clone();
        // If we have a cursor, override `since` so we only get new events.
        if let Some(ts) = self.last_seen_timestamp {
            f.since = Some(ts);
        }
        serde_json::to_string(&serde_json::json!(["REQ", self.subscription_id, f]))
            .expect("REQ serialization cannot fail")
    }

    /// Run the consumer loop on the provided connection.
    ///
    /// The callback must acknowledge successful processing by returning `Ok(())`.
    /// The in-memory replay cursor only advances after that acknowledgement.
    pub async fn subscribe<C, F>(
        &mut self,
        conn: &mut C,
        filter: &NostrFilter,
        mut on_event: F,
    ) -> Result<()>
    where
        C: RelayConnection,
        F: FnMut(NostrEvent) -> Result<()> + Send,
    {
        // Send the subscription request.
        let req = self.build_req(filter);
        conn.send(req).await?;

        // Read messages until the connection closes.
        while let Some(raw) = conn.recv().await? {
            match parse_relay_message(&raw) {
                Ok(RelayMessage::Event {
                    event,
                    subscription_id: _,
                }) => {
                    let created_at = event.created_at;
                    on_event(event).context("event processing callback failed")?;
                    self.last_seen_timestamp = Some(created_at);
                }
                Ok(RelayMessage::Eose { .. }) => {
                    // End of stored events — live tail starts now.
                }
                Ok(RelayMessage::Notice(msg)) => {
                    tracing::warn!("relay NOTICE: {msg}");
                }
                Ok(RelayMessage::Unknown(_)) => {}
                Err(e) => {
                    tracing::warn!("failed to parse relay message: {e}");
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Mock WebSocket that replays a sequence of messages.
    struct MockConnection {
        outgoing: Vec<String>,
        incoming: VecDeque<String>,
    }

    impl MockConnection {
        fn new(messages: Vec<String>) -> Self {
            Self {
                outgoing: Vec::new(),
                incoming: VecDeque::from(messages),
            }
        }
    }

    #[async_trait::async_trait]
    impl RelayConnection for MockConnection {
        async fn send(&mut self, msg: String) -> Result<()> {
            self.outgoing.push(msg);
            Ok(())
        }
        async fn recv(&mut self) -> Result<Option<String>> {
            Ok(self.incoming.pop_front())
        }
        async fn close(&mut self) -> Result<()> {
            Ok(())
        }
    }

    // -- helper ---------------------------------------------------------

    fn sample_event_json(kind: u64, created_at: i64) -> String {
        serde_json::json!({
            "id": "abc123",
            "pubkey": "deadbeef",
            "created_at": created_at,
            "kind": kind,
            "tags": [["d", "video-1"]],
            "content": "hello",
            "sig": "sig000"
        })
        .to_string()
    }

    // 1. Filter serialization
    #[test]
    fn filter_serializes_correctly() {
        let filter = NostrFilter::nip71_video();
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json["kinds"], serde_json::json!([34235, 34236, 5]));
        // Optional fields should be absent
        assert!(json.get("authors").is_none());
        assert!(json.get("since").is_none());
    }

    #[test]
    fn filter_with_since_serializes() {
        let filter = NostrFilter {
            kinds: vec![34235],
            authors: None,
            since: Some(1700000000),
        };
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json["since"], serde_json::json!(1700000000));
    }

    // 2. EVENT message parsing
    #[test]
    fn parse_event_message() {
        let raw = format!(
            r#"["EVENT","sub1",{}]"#,
            sample_event_json(34235, 1700000000)
        );
        let msg = parse_relay_message(&raw).unwrap();
        match msg {
            RelayMessage::Event {
                subscription_id,
                event,
            } => {
                assert_eq!(subscription_id, "sub1");
                assert_eq!(event.kind, 34235);
                assert_eq!(event.created_at, 1700000000);
                assert_eq!(event.id, "abc123");
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    // 3. EOSE handling
    #[test]
    fn parse_eose_message() {
        let raw = r#"["EOSE","sub1"]"#;
        let msg = parse_relay_message(raw).unwrap();
        assert_eq!(
            msg,
            RelayMessage::Eose {
                subscription_id: "sub1".into()
            }
        );
    }

    // 4. Invalid messages don't panic
    #[test]
    fn parse_invalid_json_returns_error() {
        assert!(parse_relay_message("not json").is_err());
    }

    #[test]
    fn parse_empty_array_returns_error() {
        assert!(parse_relay_message("[]").is_err());
    }

    #[test]
    fn parse_unknown_type_returns_unknown() {
        let raw = r#"["OK","sub1",true,""]"#;
        let msg = parse_relay_message(raw).unwrap();
        assert!(matches!(msg, RelayMessage::Unknown(_)));
    }

    // 5. Reconnection: build_req uses last_seen_timestamp as `since`
    #[test]
    fn build_req_includes_since_on_reconnect() {
        let mut consumer = NostrConsumer::new("wss://relay.test".into());
        let filter = NostrFilter::nip71_video();

        // First request — no since
        let req1 = consumer.build_req(&filter);
        let v1: serde_json::Value = serde_json::from_str(&req1).unwrap();
        assert!(v1[2].get("since").is_none());

        // Simulate having processed an event
        consumer.last_seen_timestamp = Some(1700000000);

        let req2 = consumer.build_req(&filter);
        let v2: serde_json::Value = serde_json::from_str(&req2).unwrap();
        assert_eq!(v2[2]["since"], serde_json::json!(1700000000));
    }

    // 6. Full subscribe loop via mock
    #[tokio::test]
    async fn subscribe_processes_events_and_tracks_cursor() {
        let messages = vec![
            format!(
                r#"["EVENT","divine-bridge-0",{}]"#,
                sample_event_json(34235, 1000)
            ),
            r#"["EOSE","divine-bridge-0"]"#.to_string(),
            format!(
                r#"["EVENT","divine-bridge-0",{}]"#,
                sample_event_json(34236, 2000)
            ),
        ];

        let mut conn = MockConnection::new(messages);
        let mut consumer = NostrConsumer::new("wss://relay.test".into());
        let filter = NostrFilter::nip71_video();

        let mut received = Vec::new();
        consumer
            .subscribe(&mut conn, &filter, |ev| {
                received.push(ev);
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(received.len(), 2);
        assert_eq!(received[0].kind, 34235);
        assert_eq!(received[1].kind, 34236);
        assert_eq!(consumer.last_seen_timestamp, Some(2000));

        // Verify the REQ was sent
        assert_eq!(conn.outgoing.len(), 1);
        let req: serde_json::Value = serde_json::from_str(&conn.outgoing[0]).unwrap();
        assert_eq!(req[0], "REQ");
    }

    #[tokio::test]
    async fn subscribe_skips_invalid_messages() {
        let messages = vec![
            "not valid json".to_string(),
            r#"["UNKNOWN_TYPE"]"#.to_string(),
            format!(
                r#"["EVENT","divine-bridge-0",{}]"#,
                sample_event_json(5, 3000)
            ),
        ];

        let mut conn = MockConnection::new(messages);
        let mut consumer = NostrConsumer::new("wss://relay.test".into());
        let filter = NostrFilter::nip71_video();

        let mut received = Vec::new();
        consumer
            .subscribe(&mut conn, &filter, |ev| {
                received.push(ev);
                Ok(())
            })
            .await
            .unwrap();

        // Only the valid event should come through
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].kind, 5);
    }
}
