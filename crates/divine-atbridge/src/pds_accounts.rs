//! PDS account provisioning client (rsky-native).
//!
//! The bridge does NOT mint DIDs. It asks rsky to create the account WITHOUT a
//! DID, so rsky authors the did:plc with its own rotation + signing keys
//! (listing the configured offline recovery key first) and the account is born
//! active. rsky returns `{did, accessJwt, refreshJwt}`, which we surface as a
//! [`CreatedAccount`].

use anyhow::{Context, Result};
use data_encoding::BASE64;
use reqwest::StatusCode;
use std::time::Duration;

use crate::provisioner::{CreatedAccount, PdsAccountCreator, PdsSession};

const DEFAULT_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct PdsAccountsClient {
    base_url: String,
    /// Pre-computed HTTP Basic auth header value: `Basic base64(admin:<password>)`
    basic_auth_header: String,
    /// Email domain used to synthesize a unique per-account address.
    email_domain: String,
    client: reqwest::Client,
    max_attempts: usize,
}

impl PdsAccountsClient {
    pub fn new(
        base_url: impl Into<String>,
        admin_password: impl Into<String>,
        email_domain: impl Into<String>,
    ) -> Self {
        Self::with_max_attempts(base_url, admin_password, email_domain, DEFAULT_MAX_ATTEMPTS)
    }

    pub fn with_max_attempts(
        base_url: impl Into<String>,
        admin_password: impl Into<String>,
        email_domain: impl Into<String>,
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
            email_domain: email_domain.into(),
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

    fn create_invite_code_endpoint(&self) -> String {
        format!(
            "{}/xrpc/com.atproto.server.createInviteCode",
            self.base_url.trim_end_matches('/')
        )
    }

    fn describe_repo_endpoint(&self) -> String {
        format!(
            "{}/xrpc/com.atproto.repo.describeRepo",
            self.base_url.trim_end_matches('/')
        )
    }

    /// Synthesize a unique, deliverable-shaped address for the account. rsky
    /// validates email format (and uniqueness), so derive it from the handle.
    fn account_email(&self, handle: &str) -> String {
        format!("noreply+{handle}@{}", self.email_domain)
    }

    /// Mint a single-use invite code (admin auth). rsky requires an invite code
    /// when PDS_INVITE_REQUIRED=true, which it is in every Divine environment.
    async fn create_invite_code(&self) -> Result<String> {
        let response = self
            .client
            .post(self.create_invite_code_endpoint())
            .header("Authorization", &self.basic_auth_header)
            .json(&serde_json::json!({ "useCount": 1 }))
            .send()
            .await
            .context("sending PDS createInviteCode request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = parse_error_message(&body);
            anyhow::bail!("createInviteCode failed ({}): {}", status.as_u16(), message);
        }

        let payload: InviteCodeResponse = response
            .json()
            .await
            .context("parsing createInviteCode response")?;
        anyhow::ensure!(
            !payload.code.is_empty(),
            "createInviteCode returned an empty code"
        );
        Ok(payload.code)
    }

    /// Resolve an already-existing account's DID from its handle, used to recover
    /// idempotently when createAccount reports the handle is already taken.
    async fn resolve_existing_did(&self, handle: &str) -> Result<String> {
        let response = self
            .client
            .get(self.describe_repo_endpoint())
            .query(&[("repo", handle)])
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
        if let Some(existing_handle) = repo.handle {
            anyhow::ensure!(
                existing_handle.eq_ignore_ascii_case(handle),
                "existing repo handle mismatch"
            );
        }
        Ok(repo.did)
    }
}

#[derive(Debug, serde::Deserialize)]
struct DescribeRepoResponse {
    did: String,
    handle: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct InviteCodeResponse {
    code: String,
}

#[derive(Debug, serde::Deserialize)]
struct CreateAccountResponse {
    did: String,
    #[serde(rename = "accessJwt")]
    access_jwt: Option<String>,
    #[serde(rename = "refreshJwt")]
    refresh_jwt: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct XrpcErrorBody {
    error: Option<String>,
    message: Option<String>,
}

#[async_trait::async_trait]
impl PdsAccountCreator for PdsAccountsClient {
    async fn create_account(
        &self,
        handle: &str,
        recovery_keys: &[String],
    ) -> Result<CreatedAccount> {
        // rsky requires an invite code; mint one per account (single use).
        let invite_code = self.create_invite_code().await?;
        let password = generate_password();
        let email = self.account_email(handle);

        let mut body = serde_json::json!({
            "handle": handle,
            "email": email,
            "password": password,
            "inviteCode": invite_code,
        });
        // rsky lists `recoveryKey` first in the new DID's rotation_keys (it accepts
        // a single key); pass the highest-priority configured recovery key.
        if let Some(recovery_key) = recovery_keys.first() {
            body["recoveryKey"] = serde_json::Value::String(recovery_key.clone());
        }

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
                // rsky-native (no supplied DID) mints the did:plc and returns the
                // account ACTIVE with a session. No activateAccount step needed.
                let payload: CreateAccountResponse = response
                    .json()
                    .await
                    .context("parsing PDS createAccount response")?;
                let session = match (payload.access_jwt, payload.refresh_jwt) {
                    (Some(access_jwt), Some(refresh_jwt)) => Some(PdsSession {
                        access_jwt,
                        refresh_jwt,
                    }),
                    _ => None,
                };
                return Ok(CreatedAccount {
                    did: payload.did,
                    session,
                });
            }

            let response_body = response.text().await.unwrap_or_default();
            if is_existing_account_conflict(status, &response_body) {
                // Handle already provisioned: recover its DID idempotently. No
                // fresh session is issued here (obtained later via login/refresh).
                let did = self.resolve_existing_did(handle).await?;
                return Ok(CreatedAccount { did, session: None });
            }

            let message = parse_error_message(&response_body);
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

/// Generate a 32-byte random account password (hex). Not persisted — rsky admin
/// can reset it, and the bridge re-authenticates via the stored session/refresh.
fn generate_password() -> String {
    use secp256k1::rand::rngs::OsRng;
    use secp256k1::rand::RngCore;

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    data_encoding::HEXLOWER.encode(&bytes)
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

    /// Stand up a mockito invite endpoint returning a fixed code.
    async fn mock_invite(server: &mut mockito::ServerGuard) -> mockito::Mock {
        server
            .mock("POST", "/xrpc/com.atproto.server.createInviteCode")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "code": "divine-invite-1" }).to_string())
            .create_async()
            .await
    }

    #[tokio::test]
    async fn create_account_posts_rsky_native_payload() {
        // rsky-native: NO `did`; WITH handle, email, password, inviteCode, and the
        // recovery key. rsky mints the DID and returns it.
        let mut server = mockito::Server::new_async().await;
        let invite = mock_invite(&mut server).await;
        let create = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .match_header("authorization", "Basic YWRtaW46YWRtaW4tdG9rZW4=")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::PartialJsonString(
                    serde_json::json!({
                        "handle": "alice.divine.video",
                        "email": "noreply+alice.divine.video@divine.video",
                        "inviteCode": "divine-invite-1",
                        "recoveryKey": "did:key:zRecovery"
                    })
                    .to_string(),
                ),
                // A password must be present and there must be NO `did` field.
                mockito::Matcher::Regex("\"password\"".to_string()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "did": "did:plc:minted",
                    "handle": "alice.divine.video",
                    "accessJwt": "access-jwt",
                    "refreshJwt": "refresh-jwt"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsAccountsClient::new(server.url(), "admin-token", "divine.video");
        let created = client
            .create_account("alice.divine.video", &["did:key:zRecovery".to_string()])
            .await
            .expect("rsky-native createAccount should succeed");

        assert_eq!(created.did, "did:plc:minted");
        let session = created.session.expect("session should be returned");
        assert_eq!(session.access_jwt, "access-jwt");
        assert_eq!(session.refresh_jwt, "refresh-jwt");
        invite.assert_async().await;
        create.assert_async().await;
    }

    #[tokio::test]
    async fn create_account_without_recovery_key_omits_recovery_field() {
        // With no configured recovery key, the body carries handle/password/invite
        // but NO `recoveryKey` (and never a `did` — rsky authors that).
        let mut server = mockito::Server::new_async().await;
        let _invite = mock_invite(&mut server).await;
        let create = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::PartialJsonString(
                    serde_json::json!({ "handle": "alice.divine.video" }).to_string(),
                ),
                mockito::Matcher::Regex("\"password\"".to_string()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({ "did": "did:plc:minted", "accessJwt": "a", "refreshJwt": "r" })
                    .to_string(),
            )
            .create_async()
            .await;

        let client = PdsAccountsClient::new(server.url(), "admin-token", "divine.video");
        let created = client
            .create_account("alice.divine.video", &[])
            .await
            .expect("createAccount without recovery key should still work");
        assert_eq!(created.did, "did:plc:minted");
        create.assert_async().await;
    }

    #[tokio::test]
    async fn create_account_conflict_recovers_did_from_handle() {
        let mut server = mockito::Server::new_async().await;
        let _invite = mock_invite(&mut server).await;
        let conflict = server
            .mock("POST", "/xrpc/com.atproto.server.createAccount")
            .with_status(400)
            .with_body("Handle already taken")
            .create_async()
            .await;
        let describe = server
            .mock("GET", "/xrpc/com.atproto.repo.describeRepo")
            .match_query(mockito::Matcher::UrlEncoded(
                "repo".into(),
                "alice.divine.video".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "did": "did:plc:existing",
                    "handle": "alice.divine.video"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let client = PdsAccountsClient::new(server.url(), "admin-token", "divine.video");
        let created = client
            .create_account("alice.divine.video", &[])
            .await
            .expect("conflict should recover");

        assert_eq!(created.did, "did:plc:existing");
        assert!(created.session.is_none());
        conflict.assert_async().await;
        describe.assert_async().await;
    }

    #[tokio::test]
    async fn create_account_retries_server_error() {
        let mut server = mockito::Server::new_async().await;
        let _invite = mock_invite(&mut server).await;
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
            .with_body(serde_json::json!({ "did": "did:plc:minted" }).to_string())
            .expect(1)
            .create_async()
            .await;

        let client =
            PdsAccountsClient::with_max_attempts(server.url(), "admin-token", "divine.video", 2);
        let created = client
            .create_account("alice.divine.video", &[])
            .await
            .expect("retry should succeed");
        assert_eq!(created.did, "did:plc:minted");
        second.assert_async().await;
    }
}
