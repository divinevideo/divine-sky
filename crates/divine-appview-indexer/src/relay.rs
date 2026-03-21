use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait RelayStream: Send {
    async fn next_changed_repo(&mut self) -> Result<Option<String>>;
}

#[derive(Default)]
pub struct NoopRelayStream;

#[async_trait]
impl RelayStream for NoopRelayStream {
    async fn next_changed_repo(&mut self) -> Result<Option<String>> {
        Ok(None)
    }
}
