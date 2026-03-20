// Blossom fetch client — fetches content-addressed blobs by SHA-256 hash

use anyhow::{anyhow, ensure, Context, Result};
use sha2::{Digest, Sha256};

/// Client for fetching blobs from a Blossom server.
///
/// Blossom serves content-addressed blobs at `{base_url}/{sha256_hex}`.
pub struct BlossomClient {
    base_url: String,
    http: reqwest::Client,
}

impl BlossomClient {
    /// Create a new BlossomClient with the given base URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Fetch a blob by its SHA-256 hex hash, verifying integrity.
    pub async fn fetch_blob(&self, sha256_hex: &str) -> Result<Vec<u8>> {
        let url = format!("{}/{}", self.base_url, sha256_hex);

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .context("failed to fetch blob from Blossom")?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Blossom returned HTTP {} for hash {}",
                status,
                sha256_hex
            ));
        }

        let data = response
            .bytes()
            .await
            .context("failed to read blob body")?
            .to_vec();

        // Verify SHA-256 hash
        let actual_hash = hex::encode(Sha256::digest(&data));
        ensure!(
            actual_hash == sha256_hex,
            "SHA-256 mismatch: expected {}, got {}",
            sha256_hex,
            actual_hash
        );

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_blob_success() {
        let mut server = mockito::Server::new_async().await;
        let body = b"hello";
        let hash = hex::encode(Sha256::digest(body));

        let mock = server
            .mock("GET", format!("/{}", hash).as_str())
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let client = BlossomClient::new(&server.url());
        let result = client.fetch_blob(&hash).await.unwrap();
        assert_eq!(result, body);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fetch_blob_hash_mismatch() {
        let mut server = mockito::Server::new_async().await;
        let fake_hash = "a".repeat(64);

        let mock = server
            .mock("GET", format!("/{}", fake_hash).as_str())
            .with_status(200)
            .with_body(b"unexpected content")
            .create_async()
            .await;

        let client = BlossomClient::new(&server.url());
        let err = client.fetch_blob(&fake_hash).await.unwrap_err();
        assert!(
            err.to_string().contains("SHA-256 mismatch"),
            "expected hash mismatch error, got: {}",
            err
        );

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fetch_blob_404() {
        let mut server = mockito::Server::new_async().await;
        let hash = "b".repeat(64);

        let mock = server
            .mock("GET", format!("/{}", hash).as_str())
            .with_status(404)
            .create_async()
            .await;

        let client = BlossomClient::new(&server.url());
        let err = client.fetch_blob(&hash).await.unwrap_err();
        assert!(
            err.to_string().contains("404"),
            "expected 404 error, got: {}",
            err
        );

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fetch_blob_network_error() {
        // Point at a port that nothing is listening on
        let client = BlossomClient::new("http://127.0.0.1:1");
        let err = client.fetch_blob(&"c".repeat(64)).await.unwrap_err();
        assert!(
            err.to_string().contains("failed to fetch blob"),
            "expected network error, got: {}",
            err
        );
    }
}
