use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct IndexerConfig {
    pub database_url: String,
    pub pds_base_url: String,
    pub relay_url: String,
    pub oneshot: bool,
}

impl IndexerConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            pds_base_url: std::env::var("DIVINE_PDS_URL").context("DIVINE_PDS_URL is required")?,
            relay_url: std::env::var("DIVINE_RELAY_URL")
                .unwrap_or_else(|_| "ws://127.0.0.1:3001".to_string()),
            oneshot: std::env::var("DIVINE_INDEXER_ONESHOT")
                .map(|value| value != "false")
                .unwrap_or(true),
        })
    }
}
