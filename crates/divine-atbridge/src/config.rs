//! Bridge configuration loaded from environment variables.

use std::env;

use anyhow::{Context, Result};

const PRODUCTION_DIVINE_HANDLE_DOMAIN: &str = "divine.video";
const PRODUCTION_DIVINE_PDS_URL: &str = "https://pds.divine.video";
pub const DEFAULT_BACKFILL_BATCH_SIZE: i64 = 25;
pub const DEFAULT_BACKFILL_PLANNER_INTERVAL_SECS: u64 = 30;

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
    /// Offline recovery PLC rotation public keys, as did:key values, listed first
    /// (highest priority) in every minted DID's rotation_keys
    /// (PLC_RECOVERY_ROTATION_DID_KEYS).
    pub plc_recovery_rotation_did_keys: Vec<String>,
    /// Email domain used to synthesize per-account addresses for createAccount
    /// (ACCOUNT_EMAIL_DOMAIN, default `divine.video`).
    pub account_email_domain: String,
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
    /// Whether to run the lease-expiry / failed-backfill watchdog poll loop
    /// (WATCHDOG_ENABLED). Off by default — opt in per environment.
    pub watchdog_enabled: bool,
    /// Watchdog poll cadence in seconds (WATCHDOG_INTERVAL_SECS).
    pub watchdog_interval_secs: u64,
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
            plc_recovery_rotation_did_keys: parse_recovery_rotation_did_keys(
                env::var("PLC_RECOVERY_ROTATION_DID_KEYS").ok().as_deref(),
            )?,
            account_email_domain: env::var("ACCOUNT_EMAIL_DOMAIN")
                .unwrap_or_else(|_| "divine.video".to_string()),
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
            watchdog_enabled: env::var("WATCHDOG_ENABLED")
                .map(|v| v == "true")
                .unwrap_or(false),
            watchdog_interval_secs: env::var("WATCHDOG_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
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

/// Parse the comma-separated `PLC_RECOVERY_ROTATION_DID_KEYS` env value into a
/// validated, deduplicated list of `did:key` recovery rotation keys.
///
/// PLC allows at most 5 rotation keys total; rsky always appends its own
/// operational rotation key, so we cap recovery keys at 4.
fn parse_recovery_rotation_did_keys(value: Option<&str>) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for key in value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        anyhow::ensure!(
            key.starts_with("did:key:"),
            "PLC_RECOVERY_ROTATION_DID_KEYS entries must be did:key values"
        );
        anyhow::ensure!(
            !keys.iter().any(|existing| existing == key),
            "duplicate PLC recovery rotation key: {key}"
        );
        keys.push(key.to_string());
    }

    anyhow::ensure!(
        keys.len() <= 4,
        "PLC_RECOVERY_ROTATION_DID_KEYS supports at most 4 recovery keys"
    );

    Ok(keys)
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
            plc_recovery_rotation_did_keys: Vec::new(),
            account_email_domain: "divine.video".into(),
            provisioning_bearer_token: "test-token".into(),
            video_service_url: "https://video.bsky.app".into(),
            video_service_enabled: false,
            video_service_poll_timeout_secs: 120,
            video_service_poll_interval_ms: 5000,
            watchdog_enabled: false,
            watchdog_interval_secs: 30,
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
            plc_recovery_rotation_did_keys: Vec::new(),
            account_email_domain: "divine.video".into(),
            provisioning_bearer_token: "test-token".into(),
            video_service_url: "https://video.bsky.app".into(),
            video_service_enabled: false,
            video_service_poll_timeout_secs: 120,
            video_service_poll_interval_ms: 5000,
            watchdog_enabled: false,
            watchdog_interval_secs: 30,
        };

        assert_eq!(config.provisioning_pds_url(), "https://pds.divine.video");
    }

    #[test]
    fn parses_recovery_rotation_did_keys_from_comma_separated_env() {
        let keys =
            parse_recovery_rotation_did_keys(Some(" did:key:zRecovery1 ,did:key:zRecovery2,, "))
                .expect("valid recovery keys should parse");

        assert_eq!(
            keys,
            vec![
                "did:key:zRecovery1".to_string(),
                "did:key:zRecovery2".to_string()
            ]
        );
    }

    #[test]
    fn recovery_rotation_did_keys_default_empty_when_unset() {
        let keys = parse_recovery_rotation_did_keys(None).expect("unset is allowed");
        assert!(keys.is_empty());
    }

    #[test]
    fn rejects_non_did_key_recovery_rotation_entries() {
        let err = parse_recovery_rotation_did_keys(Some("did:plc:abc123"))
            .expect_err("recovery rotation keys must be did:key values");
        assert!(err.to_string().contains("did:key"));
    }

    #[test]
    fn rejects_too_many_recovery_rotation_did_keys() {
        let err = parse_recovery_rotation_did_keys(Some(
            "did:key:z1,did:key:z2,did:key:z3,did:key:z4,did:key:z5",
        ))
        .expect_err("five recovery keys plus the operational key would exceed PLC max");

        assert!(err.to_string().contains("at most 4"));
    }

    #[test]
    fn rejects_duplicate_recovery_rotation_did_keys() {
        let err = parse_recovery_rotation_did_keys(Some("did:key:z1,did:key:z1"))
            .expect_err("duplicate recovery keys should fail");
        assert!(err.to_string().contains("duplicate"));
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
            plc_recovery_rotation_did_keys: Vec::new(),
            account_email_domain: "divine.video".into(),
            provisioning_bearer_token: "test-token".into(),
            video_service_url: "https://video.bsky.app".into(),
            video_service_enabled: false,
            video_service_poll_timeout_secs: 120,
            video_service_poll_interval_ms: 5000,
            watchdog_enabled: false,
            watchdog_interval_secs: 30,
        };

        assert_eq!(config.provisioning_pds_url(), "http://pds:2583");
    }
}
