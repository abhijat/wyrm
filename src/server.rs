use crate::arena::{ArenaParams, ArenaReport, HeapKind, parse_arena_summary};
use crate::connection::{ConnectionMemorySample, ConnectionProfile};
use crate::memory::MemorySnapshot;
use redis::aio::ConnectionManager;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerConfig};
use rmcp::{Json, ServerHandler, schemars, tool, tool_handler, tool_router};
use std::time::Duration;
use tokio::time::{Instant, sleep};

pub(crate) struct Wyrm {
    tool_router: ToolRouter<Self>,
    connection: ConnectionManager,
}

#[tool_router]
impl Wyrm {
    pub(crate) fn new(connection: ConnectionManager) -> Self {
        Self {
            tool_router: Self::tool_router(),
            connection,
        }
    }

    #[tool(description = "Fetch INFO MEMORY snapshot")]
    async fn fetch_info_memory_snapshot(&self) -> CallResult<MemorySnapshot> {
        let mut conn = self.connection.clone();
        let raw: String = redis::cmd("INFO")
            .arg("MEMORY")
            .query_async(&mut conn)
            .await
            .map_err(|err| format!("Failed to fetch INFO MEMORY: {err}"))?;
        let snapshot = crate::memory::parse_memory_info(&raw)
            .map_err(|err| format!("Failed to parse INFO MEMORY: {err:#}"))?;
        Ok(Json(snapshot))
    }

    #[tool(description = "Fetch INFO CLIENTS snapshot")]
    async fn fetch_connection_snapshot(&self) -> CallResult<ConnectionProfile> {
        let p = self.fetch_and_parse_conn_snapshot().await?;
        Ok(Json(p))
    }

    async fn fetch_and_parse_conn_snapshot(&self) -> Result<ConnectionProfile, String> {
        let mut conn = self.connection.clone();
        let raw: String = redis::cmd("INFO")
            .arg("CLIENTS")
            .query_async(&mut conn)
            .await
            .map_err(|err| format!("Failed to fetch INFO CLIENTS: {err:#}"))?;
        let snapshot = crate::connection::parse_client_info(&raw)
            .map_err(|err| format!("Failed to parse INFO CLIENTS: {err:#}"))?;

        let average_read_buffer_bytes_per_connection =
            crate::connection::average_read_buffer_bytes(&snapshot);
        Ok(ConnectionProfile {
            snapshot,
            average_read_buffer_bytes_per_connection,
        })
    }

    #[tool(description = "Sample connection memory")]
    async fn sample_connection_memory(
        &self,
        Parameters(SamplingParams { interval_ms }): Parameters<SamplingParams>,
    ) -> CallResult<ConnectionMemorySample> {
        if interval_ms < 100 || interval_ms > 5000 {
            return Err(format!(
                "interval_ms must be between 100 and 5000, got {interval_ms}"
            ));
        }

        let start = Instant::now();
        let before = self.fetch_and_parse_conn_snapshot().await?;

        sleep(Duration::from_millis(interval_ms)).await;

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let after = self.fetch_and_parse_conn_snapshot().await?;

        let change = crate::connection::connection_memory_change(&before.snapshot, &after.snapshot);
        let findings = crate::connection::connection_memory_findings(&change);
        let sample = ConnectionMemorySample {
            before,
            after,
            elapsed_ms,
            change,
            findings,
        };
        Ok(Json(sample))
    }

    #[tool(description = "Fetch Arena")]
    async fn fetch_arena_snapshot(
        &self,
        Parameters(ArenaParams { heap }): Parameters<ArenaParams>,
    ) -> CallResult<ArenaReport> {
        let mut command = redis::cmd("MEMORY");
        command.arg("ARENA").arg("SUMMARY");
        if matches!(&heap, HeapKind::Backing) {
            command.arg("BACKING");
        }
        let mut conn = self.connection.clone();
        let raw: String = command
            .query_async(&mut conn)
            .await
            .map_err(|err| format!("Failed to fetch ARENA: {err}"))?;
        let snapshot = parse_arena_summary(&raw, heap)
            .map_err(|err| format!("Count not parse ARENA: {err}"))?;
        let report = ArenaReport::new(snapshot);
        Ok(Json(report))
    }
}

#[tool_handler]
impl ServerHandler for Wyrm {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SamplingParams {
    interval_ms: u64,
}

type CallResult<T> = Result<Json<T>, String>;
