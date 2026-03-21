use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct AppviewConfig {
    pub bind_addr: String,
    pub database_url: String,
    pub media_base_url: String,
    pub viewer_origin: Option<String>,
}

impl AppviewConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3004".to_string()),
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            media_base_url: std::env::var("APPVIEW_MEDIA_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3100".to_string()),
            viewer_origin: std::env::var("VIEWER_ORIGIN").ok(),
        })
    }
}
