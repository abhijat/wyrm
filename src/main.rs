use crate::metrics::MetricsCollector;
use redis::aio::ConnectionManager;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use server::Wyrm;
use std::env;

mod arena;
mod connection;
mod memory;
mod memory_gap;
mod metrics;
mod parsing;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = redis::Client::open("redis://127.0.0.1:6379/")?;
    let connection = ConnectionManager::new(client).await?;

    let metrics = env::var("WYRM_METRICS_URL")
        .ok()
        .map(|url| MetricsCollector::new(&url))
        .transpose()?;
    let service = Wyrm::new(connection, metrics).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
