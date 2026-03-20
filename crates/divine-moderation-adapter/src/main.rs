pub mod labels;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    tracing::info!("divine moderation adapter ready");
    Ok(())
}
