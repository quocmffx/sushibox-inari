use anyhow::Result;

pub async fn run() -> Result<()> {
    super::stop::run().await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    super::start::run().await
}
