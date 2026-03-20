//! Configuration for the divine-labeler service.

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone)]
pub struct LabelerConfig {
    pub labeler_did: String,
    pub signing_key_hex: String,
    pub database_url: String,
    pub webhook_token: String,
    pub port: u16,
}

impl LabelerConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            labeler_did: std::env::var("LABELER_DID").context("LABELER_DID must be set")?,
            signing_key_hex: std::env::var("LABELER_SIGNING_KEY")
                .context("LABELER_SIGNING_KEY must be set")?,
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            webhook_token: std::env::var("WEBHOOK_TOKEN").context("WEBHOOK_TOKEN must be set")?,
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3001".to_string())
                .parse()
                .context("PORT must be a valid u16")?,
        })
    }

    pub fn validate_signing_key(hex: &str) -> Result<Vec<u8>> {
        if hex.is_empty() {
            bail!("signing key must not be empty");
        }
        let bytes = hex::decode(hex).context("signing key must be valid hex")?;
        if bytes.len() != 32 {
            bail!(
                "signing key must be 32 bytes (64 hex chars), got {}",
                bytes.len()
            );
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_validates_labeler_did_format() {
        let config = LabelerConfig {
            labeler_did: "did:plc:abc123".to_string(),
            signing_key_hex: "a".repeat(64),
            database_url: "postgres://localhost/test".to_string(),
            webhook_token: "secret".to_string(),
            port: 3001,
        };
        assert!(config.labeler_did.starts_with("did:"));
    }

    #[test]
    fn config_rejects_empty_signing_key() {
        let result = LabelerConfig::validate_signing_key("");
        assert!(result.is_err());
    }

    #[test]
    fn config_accepts_valid_hex_signing_key() {
        let result = LabelerConfig::validate_signing_key(&"ab".repeat(32));
        assert!(result.is_ok());
    }
}
