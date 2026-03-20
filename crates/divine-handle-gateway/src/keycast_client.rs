use anyhow::{Context, Result};
use reqwest::Client;
use serde::Serialize;

#[derive(Clone)]
pub struct KeycastClient {
    client: Client,
    sync_url: String,
    bearer_token: String,
}

#[derive(Debug, Serialize)]
struct AtprotoStateSyncRequest<'a> {
    nostr_pubkey: &'a str,
    enabled: bool,
    state: &'a str,
    did: Option<&'a str>,
    error: Option<&'a str>,
}

impl KeycastClient {
    pub fn new(sync_url: String, bearer_token: String) -> Self {
        Self {
            client: Client::new(),
            sync_url,
            bearer_token,
        }
    }

    pub async fn sync_ready(&self, nostr_pubkey: &str, did: &str) -> Result<()> {
        self.sync_state(AtprotoStateSyncRequest {
            nostr_pubkey,
            enabled: true,
            state: "ready",
            did: Some(did),
            error: None,
        })
        .await
    }

    pub async fn sync_failed(&self, nostr_pubkey: &str, error: &str) -> Result<()> {
        self.sync_state(AtprotoStateSyncRequest {
            nostr_pubkey,
            enabled: true,
            state: "failed",
            did: None,
            error: Some(error),
        })
        .await
    }

    pub async fn sync_disabled(&self, nostr_pubkey: &str) -> Result<()> {
        self.sync_state(AtprotoStateSyncRequest {
            nostr_pubkey,
            enabled: false,
            state: "disabled",
            did: None,
            error: None,
        })
        .await
    }

    async fn sync_state(&self, payload: AtprotoStateSyncRequest<'_>) -> Result<()> {
        self.client
            .post(&self.sync_url)
            .bearer_auth(&self.bearer_token)
            .json(&payload)
            .send()
            .await
            .context("keycast sync request failed")?
            .error_for_status()
            .context("keycast sync returned non-success status")?;

        Ok(())
    }
}
