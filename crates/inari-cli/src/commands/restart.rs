use anyhow::Result;

pub async fn run() -> Result<()> {
    // stop::run already waits for each process to fully exit (freeing its port)
    // before returning, so no arbitrary sleep is needed between phases.
    super::stop::run().await?;
    super::start::run().await
}
