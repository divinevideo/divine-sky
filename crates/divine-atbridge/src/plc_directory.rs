//! PLC directory HTTP client.

use anyhow::{Context, Result};
use reqwest::StatusCode;
use std::time::Duration;

use crate::provisioner::{derive_did_plc, PlcClient, PlcOperation};

const DEFAULT_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct PlcDirectoryClient {
    base_url: String,
    client: reqwest::Client,
    max_attempts: usize,
}

impl PlcDirectoryClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_max_attempts(base_url, DEFAULT_MAX_ATTEMPTS)
    }

    pub fn with_max_attempts(base_url: impl Into<String>, max_attempts: usize) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("reqwest client builder should succeed");

        Self {
            base_url: base_url.into(),
            client,
            max_attempts: max_attempts.max(1),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/", self.base_url.trim_end_matches('/'))
    }

    fn did_endpoint(&self, did: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), did)
    }
}

#[derive(Debug, serde::Deserialize)]
struct PlcCreateDidResponse {
    did: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PlcErrorBody {
    error: Option<String>,
    message: Option<String>,
}

#[async_trait::async_trait]
impl PlcClient for PlcDirectoryClient {
    async fn create_did(&self, operation: &PlcOperation) -> Result<String> {
        let derived_did = derive_did_plc(operation);
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..self.max_attempts {
            let response = match self
                .client
                .post(self.endpoint())
                .json(operation)
                .send()
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    if should_retry_request_error(&err) && attempt + 1 < self.max_attempts {
                        tokio::time::sleep(retry_delay(attempt)).await;
                        continue;
                    }
                    return Err(err).context("sending PLC directory create request");
                }
            };

            let status = response.status();
            if status.is_success() {
                let payload: PlcCreateDidResponse = response
                    .json()
                    .await
                    .context("parsing PLC directory create response")?;
                return Ok(payload.did.unwrap_or(derived_did));
            }

            if status == StatusCode::CONFLICT {
                return Ok(derived_did);
            }

            let body = response.text().await.unwrap_or_default();
            let message = parse_error_message(&body);
            let err = anyhow::anyhow!("PLC directory create failed ({}): {message}", status);
            if status.is_server_error() && attempt + 1 < self.max_attempts {
                last_error = Some(err);
                tokio::time::sleep(retry_delay(attempt)).await;
                continue;
            }
            return Err(err);
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("PLC directory create failed")))
    }
}

impl PlcDirectoryClient {
    pub async fn update_did(&self, did: &str, operation: &PlcOperation) -> Result<()> {
        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..self.max_attempts {
            let response = match self
                .client
                .post(self.did_endpoint(did))
                .json(operation)
                .send()
                .await
            {
                Ok(response) => response,
                Err(err) => {
                    if should_retry_request_error(&err) && attempt + 1 < self.max_attempts {
                        tokio::time::sleep(retry_delay(attempt)).await;
                        continue;
                    }
                    return Err(err).context("sending PLC directory update request");
                }
            };

            let status = response.status();
            if status.is_success() {
                return Ok(());
            }

            let body = response.text().await.unwrap_or_default();
            let message = parse_error_message(&body);
            let err = anyhow::anyhow!("PLC directory update failed ({}): {message}", status);
            if status.is_server_error() && attempt + 1 < self.max_attempts {
                last_error = Some(err);
                tokio::time::sleep(retry_delay(attempt)).await;
                continue;
            }
            return Err(err);
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("PLC directory update failed")))
    }
}

fn parse_error_message(body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<PlcErrorBody>(body) {
        let error = parsed.error.unwrap_or_default();
        let message = parsed.message.unwrap_or_default();
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
    use crate::provisioner::{PlcOperation, PlcService};
    use std::collections::BTreeMap;

    fn operation() -> PlcOperation {
        let mut verification_methods = BTreeMap::new();
        verification_methods.insert("atproto".to_string(), "did:key:zexample".to_string());

        let mut services = BTreeMap::new();
        services.insert(
            "atproto_pds".to_string(),
            PlcService {
                service_type: "AtprotoPersonalDataServer".to_string(),
                endpoint: "https://pds.divine.video".to_string(),
            },
        );

        PlcOperation {
            op_type: "plc_operation".to_string(),
            rotation_keys: vec!["did:key:zrotation".to_string()],
            verification_methods,
            also_known_as: vec!["at://alice.divine.video".to_string()],
            services,
            prev: None,
            sig: "sig".to_string(),
        }
    }

    #[tokio::test]
    async fn create_did_returns_did_from_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"did":"did:plc:from-directory"}).to_string())
            .create_async()
            .await;

        let client = PlcDirectoryClient::new(server.url());
        let did = client.create_did(&operation()).await.unwrap();
        mock.assert_async().await;
        assert_eq!(did, "did:plc:from-directory");
    }

    #[tokio::test]
    async fn create_did_retries_server_errors() {
        let mut server = mockito::Server::new_async().await;
        let _first = server
            .mock("POST", "/")
            .with_status(503)
            .with_body("temporary")
            .expect(1)
            .create_async()
            .await;
        let second = server
            .mock("POST", "/")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"did":"did:plc:retry-ok"}).to_string())
            .expect(1)
            .create_async()
            .await;

        let client = PlcDirectoryClient::with_max_attempts(server.url(), 2);
        let did = client.create_did(&operation()).await.unwrap();
        second.assert_async().await;
        assert_eq!(did, "did:plc:retry-ok");
    }

    #[tokio::test]
    async fn create_did_conflict_returns_derived_did() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(409)
            .with_body("already exists")
            .create_async()
            .await;

        let op = operation();
        let expected = derive_did_plc(&op);
        let client = PlcDirectoryClient::new(server.url());
        let did = client.create_did(&op).await.unwrap();
        mock.assert_async().await;
        assert_eq!(did, expected);
    }
}
