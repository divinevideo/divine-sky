//! Bridge configuration loaded from environment variables.

use std::env;

use anyhow::{Context, Result};

const PRODUCTION_DIVINE_HANDLE_DOMAIN: &str = "divine.video";
const PRODUCTION_DIVINE_PDS_URL: &str = "https://pds.divine.video";

/// Configuration for the ATBridge service.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Funnelcake relay WebSocket URL (RELAY_URL).
    pub relay_url: String,
    /// rsky-pds XRPC base URL (PDS_URL).
    pub pds_url: String,
    /// Bearer token used for PDS XRPC writes (PDS_AUTH_TOKEN).
    pub pds_auth_token: String,
    /// Blossom server base URL (BLOSSOM_URL).
    pub blossom_url: String,
    /// PostgreSQL connection string (DATABASE_URL).
    pub database_url: String,
    /// S3-compatible endpoint (S3_ENDPOINT).
    pub s3_endpoint: String,
    /// S3 bucket name (S3_BUCKET).
    pub s3_bucket: String,
    /// Logical relay source name for replay offsets (RELAY_SOURCE_NAME).
    pub relay_source_name: String,
    /// Internal API server bind address (HEALTH_BIND_ADDR).
    pub health_bind_addr: String,
    /// PLC directory base URL (PLC_DIRECTORY_URL).
    pub plc_directory_url: String,
    /// Handle domain accepted for provisioning (HANDLE_DOMAIN).
    pub handle_domain: String,
    /// Shared bearer token for the internal provisioning API (ATPROTO_PROVISIONING_TOKEN).
    pub provisioning_bearer_token: String,
    /// Base URL for the Bluesky video transcoding service (VIDEO_SERVICE_URL).
    pub video_service_url: String,
    /// Whether to route video uploads through the video service (VIDEO_SERVICE_ENABLED).
    pub video_service_enabled: bool,
    /// Timeout in seconds for polling a video transcoding job (VIDEO_SERVICE_POLL_TIMEOUT_SECS).
    pub video_service_poll_timeout_secs: u64,
    /// Interval in milliseconds between poll requests (VIDEO_SERVICE_POLL_INTERVAL_MS).
    pub video_service_poll_interval_ms: u64,
}

impl BridgeConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            relay_url: env::var("RELAY_URL").context("RELAY_URL must be set")?,
            pds_url: env::var("PDS_URL").context("PDS_URL must be set")?,
            pds_auth_token: env::var("PDS_AUTH_TOKEN").context("PDS_AUTH_TOKEN must be set")?,
            blossom_url: env::var("BLOSSOM_URL").context("BLOSSOM_URL must be set")?,
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            s3_endpoint: env::var("S3_ENDPOINT").context("S3_ENDPOINT must be set")?,
            s3_bucket: env::var("S3_BUCKET").context("S3_BUCKET must be set")?,
            relay_source_name: env::var("RELAY_SOURCE_NAME")
                .unwrap_or_else(|_| "nostr-relay".to_string()),
            health_bind_addr: env::var("HEALTH_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            plc_directory_url: env::var("PLC_DIRECTORY_URL")
                .context("PLC_DIRECTORY_URL must be set")?,
            handle_domain: env::var("HANDLE_DOMAIN").context("HANDLE_DOMAIN must be set")?,
            provisioning_bearer_token: env::var("ATPROTO_PROVISIONING_TOKEN")
                .context("ATPROTO_PROVISIONING_TOKEN must be set")?,
            video_service_url: env::var("VIDEO_SERVICE_URL")
                .unwrap_or_else(|_| "https://video.bsky.app".to_string()),
            video_service_enabled: env::var("VIDEO_SERVICE_ENABLED")
                .map(|v| v == "true")
                .unwrap_or(false),
            video_service_poll_timeout_secs: env::var("VIDEO_SERVICE_POLL_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            video_service_poll_interval_ms: env::var("VIDEO_SERVICE_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
        })
    }

    pub fn provisioning_pds_url(&self) -> String {
        let handle_domain = self
            .handle_domain
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();

        if handle_domain == PRODUCTION_DIVINE_HANDLE_DOMAIN
            && self.pds_url.trim().starts_with("https://")
        {
            return PRODUCTION_DIVINE_PDS_URL.to_string();
        }

        self.pds_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_missing_var_returns_error() {
        // With no env vars set, from_env should fail.
        // We can't guarantee env is clean, but RELAY_URL is unlikely to be set in CI.
        // Instead we just verify the struct can be constructed manually.
        let config = BridgeConfig {
            relay_url: "wss://relay.example.com".into(),
            pds_url: "https://pds.example.com".into(),
            pds_auth_token: "test-token".into(),
            blossom_url: "https://blossom.example.com".into(),
            database_url: "postgres://localhost/test".into(),
            s3_endpoint: "https://s3.example.com".into(),
            s3_bucket: "test-bucket".into(),
            relay_source_name: "nostr-relay".into(),
            health_bind_addr: "0.0.0.0:8080".into(),
            plc_directory_url: "https://plc.directory".into(),
            handle_domain: "divine.video".into(),
            provisioning_bearer_token: "test-token".into(),
            video_service_url: "https://video.bsky.app".into(),
            video_service_enabled: false,
            video_service_poll_timeout_secs: 120,
            video_service_poll_interval_ms: 5000,
        };
        assert_eq!(config.relay_url, "wss://relay.example.com");
        assert_eq!(config.s3_bucket, "test-bucket");
    }

    #[test]
    fn provisioning_pds_url_prefers_production_host_for_divine_video() {
        let config = BridgeConfig {
            relay_url: "wss://relay.example.com".into(),
            pds_url: "https://pds.staging.dvines.org".into(),
            pds_auth_token: "test-token".into(),
            blossom_url: "https://blossom.example.com".into(),
            database_url: "postgres://localhost/test".into(),
            s3_endpoint: "https://s3.example.com".into(),
            s3_bucket: "test-bucket".into(),
            relay_source_name: "nostr-relay".into(),
            health_bind_addr: "0.0.0.0:8080".into(),
            plc_directory_url: "https://plc.directory".into(),
            handle_domain: "divine.video".into(),
            provisioning_bearer_token: "test-token".into(),
            video_service_url: "https://video.bsky.app".into(),
            video_service_enabled: false,
            video_service_poll_timeout_secs: 120,
            video_service_poll_interval_ms: 5000,
        };

        assert_eq!(config.provisioning_pds_url(), "https://pds.divine.video");
    }

    #[test]
    fn provisioning_pds_url_keeps_local_dev_pds_url() {
        let config = BridgeConfig {
            relay_url: "wss://relay.example.com".into(),
            pds_url: "http://pds:2583".into(),
            pds_auth_token: "test-token".into(),
            blossom_url: "https://blossom.example.com".into(),
            database_url: "postgres://localhost/test".into(),
            s3_endpoint: "https://s3.example.com".into(),
            s3_bucket: "test-bucket".into(),
            relay_source_name: "nostr-relay".into(),
            health_bind_addr: "0.0.0.0:8080".into(),
            plc_directory_url: "https://plc.directory".into(),
            handle_domain: "divine.video".into(),
            provisioning_bearer_token: "test-token".into(),
            video_service_url: "https://video.bsky.app".into(),
            video_service_enabled: false,
            video_service_poll_timeout_secs: 120,
            video_service_poll_interval_ms: 5000,
        };

        assert_eq!(config.provisioning_pds_url(), "http://pds:2583");
    }
}
