use redis::aio::ConnectionManager;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use server::Wyrm;

mod connection;
mod memory;
mod parsing;
mod server;
mod arena;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = redis::Client::open("redis://127.0.0.1:6379/")?;
    let connection = ConnectionManager::new(client).await?;

    let service = Wyrm::new(connection).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
