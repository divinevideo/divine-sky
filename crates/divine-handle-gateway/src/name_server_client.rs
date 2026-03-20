use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Serialize;

#[derive(Clone)]
pub struct NameServerClient {
    client: Client,
    sync_url: String,
    bearer_token: String,
}

#[derive(Debug, Serialize)]
struct AtprotoSyncRequest<'a> {
    name: &'a str,
    atproto_did: Option<&'a str>,
    atproto_state: &'a str,
}

impl NameServerClient {
    pub fn new(sync_url: String, bearer_token: String) -> Self {
        Self {
            client: Client::new(),
            sync_url,
            bearer_token,
        }
    }

    pub async fn sync_state_for_handle(
        &self,
        handle: &str,
        did: Option<&str>,
        state: &str,
    ) -> Result<()> {
        let name = handle
            .split('.')
            .next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("invalid handle: {handle}"))?;

        let request = AtprotoSyncRequest {
            name,
            atproto_did: did,
            atproto_state: state,
        };

        self.client
            .post(&self.sync_url)
            .bearer_auth(&self.bearer_token)
            .json(&request)
            .send()
            .await
            .context("name server sync request failed")?
            .error_for_status()
            .context("name server sync returned non-success status")?;

        Ok(())
    }
}
