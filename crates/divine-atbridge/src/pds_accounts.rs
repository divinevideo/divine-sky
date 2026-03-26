//! PDS account provisioning client.

use anyhow::{Context, Result};
use data_encoding::BASE64;
use reqwest::StatusCode;
use std::time::Duration;

use crate::provisioner::PdsAccountCreator;

const DEFAULT_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct PdsAccountsClient {
    base_url: String,
    /// Pre-computed HTTP Basic auth header value: `Basic base64(admin:<password>)`
    basic_auth_header: String,
    client: reqwest::Client,
    max_attempts: usize,
}

impl PdsAccountsClient {
    pub fn new(base_url: impl Into<String>, admin_password: impl Into<String>) -> Self {
        Self::with_max_attempts(base_url, admin_password, DEFAULT_MAX_ATTEMPTS)
    }

    pub fn with_max_attempts(
        base_url: impl Into<String>,
        admin_password: impl Into<String>,
        max_attempts: usize,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("reqwest client builder should succeed");

        let password = admin_password.into();
        let encoded = BASE64.encode(format!("admin:{}", password).as_bytes());
        let basic_auth_header = format!("Basic {}", encoded);

        Self {
            base_url: base_url.into(),
            basic_auth_header,
            client,
            max_attempts: max_attempts.max(1),
        }
    }

    fn create_account_endpoint(&self) -> String {
        format!(
            "{}/xrpc/com.atproto.server.createAccount",
            self.base_url.trim_end_matches('/')
        )
    }

    fn describe_repo_endpoint(&self) -> String {
        format!(
            "{}/xrpc/com.atproto.repo.describeRepo",
            self.base_url.trim_end_matches('/')
        )
    }

    async fn confirm_existing_repo(&self, did: &str, handle: &str) -> Result<()> {
        let response = self
            .client
            .get(self.describe_repo_endpoint())
            .query(&[("repo", did)])
            .header("Authorization", &self.basic_auth_header)
            .send()
            .await
            .context("sending describeRepo request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = parse_error_message(&body);
            anyhow::bail!("describeRepo failed ({}): {}", status.as_u16(), message);
        }

        let repo: DescribeRepoResponse = response
            .json()
            .await
            .context("parsing describeRepo response")?;
        anyhow::ensure!(repo.did == did, "existing repo DID mismatch");

        if let Some(existing_handle) = repo.handle {
            anyhow::ensure!(
                existing_handle.eq_ignore_ascii_case(handle),
                "existing repo handle mismatch"
            );
        }

        Ok(())
    }
}

#[derive(Debug, serde::Deserialize)]
struct DescribeRepoResponse {
    did: String,
    handle: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct XrpcErrorBody {
    error: Option<String>,
    message: Option<String>,
}

#[async_trait::async_trait]
impl PdsAccountCreator for PdsAccountsClient {
    async fn create_account(&self, did: &str, handle: &str) -> Result<()> {
        let body = serde_json::json!({
            "did": did,
            "handle": handle,
        });

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..self.max_attempts {
            let response = match self
                .client
                .post(self.create_account_endpoint())
                .header("Authorization", &self.basic_auth_header)
                .json(&body)
                .send()
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    if should_retry_request_error(&err) && attempt + 1 < self.max_attempts {
                        tokio::time::sleep(retry_delay(attempt)).await;
                        continue;
                    }
                    return Err(err).context("sending PDS createAccount request");
                }
            };

            let status = response.status();
            if status.is_success() {
                return Ok(());
            }

            let body = response.text().await.unwrap_or_default();
            if is_existing_account_conflict(status, &body) {
                return self.confirm_existing_repo(did, handle).await;
            }

            let message = parse_error_message(&body);
            let err = anyhow::anyhow!("createAccount failed ({}): {message}", status.as_u16());
            if status.is_server_error() && attempt + 1 < self.max_attempts {
                last_error = Some(err);
                tokio::time::sleep(retry_delay(attempt)).await;
                continue;
            }
            return Err(err);
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("createAccount failed")))
    }
}

fn is_existing_account_conflict(status: StatusCode, body: &str) -> bool {
    if status == StatusCode::CONFLICT {
        return true;
    }
    if status != StatusCode::BAD_REQUEST {
        return false;
    }

    let lowered = body.to_ascii_lowercase();
    lowered.contains("already") || lowered.contains("exists") || lowered.contains("taken")
}

fn parse_error_message(body: &str) -> String {
    if let Ok(err) = serde_json::from_str::<XrpcErrorBody>(body) {
        let error = err.error.unwrap_or_default();
        let message = err.message.unwrap_or_default();
        if !error.is_empty() && !message.is_empty() {
            return format!("{error}: {message}");
        }
        if !error.is_empty() {
            return error;
        }
        if !message.is_empty() {
            return message;
        }
    }
    body.to_string()
}

fn should_retry_request_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn retry_delay(attempt: usize) -> Duration {
    let millis = 50u64 * (attempt as u64 + 1);
    Duration::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_account_posts_expected_payload() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .match_header("authorization", "Basic YWRtaW46YWRtaW4tdG9rZW4=")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::JsonString(
                serde_json::json!({
                    "did":"did:plc:abc123",
                    "handle":"alice.divine.video"
                })
                .to_string(),
            ))
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = PdsAccountsClient::new(server.url(), "admin-token");
        client
            .create_account("did:plc:abc123", "alice.divine.video")
            .await
            .unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn create_account_conflict_recovers_by_describing_repo() {
        let mut server = mockito::Server::new_async().await;
        let conflict = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .with_status(409)
            .with_body("Account already exists")
            .create_async()
            .await;
        let describe = server
            .mock("GET", "/xrpc/com.atproto.repo.describeRepo")
            .match_query(mockito::Matcher::UrlEncoded(
                "repo".into(),
                "did:plc:abc123".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "did":"did:plc:abc123",
                    "handle":"alice.divine.video"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsAccountsClient::new(server.url(), "admin-token");
        client
            .create_account("did:plc:abc123", "alice.divine.video")
            .await
            .unwrap();

        conflict.assert_async().await;
        describe.assert_async().await;
    }

    #[tokio::test]
    async fn create_account_retries_server_error() {
        let mut server = mockito::Server::new_async().await;
        let _first = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .with_status(503)
            .with_body("temporary")
            .expect(1)
            .create_async()
            .await;
        let second = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .with_status(200)
            .with_body("{}")
            .expect(1)
            .create_async()
            .await;

        let client = PdsAccountsClient::with_max_attempts(server.url(), "admin-token", 2);
        client
            .create_account("did:plc:abc123", "alice.divine.video")
            .await
            .unwrap();
        second.assert_async().await;
    }
}
