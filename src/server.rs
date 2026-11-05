use crate::arena::{ArenaParams, ArenaReport, HeapKind, parse_arena_summary};
use crate::connection::{
    ConnectionMemorySample, ConnectionProfile, connection_memory_change,
    connection_memory_findings, parse_client_info,
};
use crate::memory::{MemorySnapshot, parse_memory_info};
use crate::metrics::{DefragSnapshot, MetricsCollector, parse_defrag_info, parse_defrag_snapshot};
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
    metrics_collector: Option<MetricsCollector>,
}

#[tool_router]
impl Wyrm {
    pub(crate) fn new(
        connection: ConnectionManager,
        metrics_collector: Option<MetricsCollector>,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            connection,
            metrics_collector,
        }
    }

    async fn run_command(&self, cmd: &str) -> anyhow::Result<String> {
        let cmd: Vec<_> = cmd.split_whitespace().collect();
        let mut command = redis::cmd(cmd[0]);
        for arg in cmd.iter().skip(1) {
            command.arg(arg);
        }
        let mut conn = self.connection.clone();
        let raw: String = command.query_async(&mut conn).await?;
        Ok(raw)
    }

    #[tool(description = "Fetch INFO MEMORY snapshot")]
    async fn fetch_info_memory_snapshot(&self) -> CallResult<MemorySnapshot> {
        let raw = self
            .run_command("INFO MEMORY")
            .await
            .map_err(|err| format!("Failed to fetch INFO MEMORY: {err}"))?;
        let snapshot = parse_memory_info(&raw)
            .map_err(|err| format!("Failed to parse INFO MEMORY: {err:#}"))?;
        Ok(Json(snapshot))
    }

    #[tool(description = "Fetch INFO CLIENTS snapshot")]
    async fn fetch_connection_snapshot(&self) -> CallResult<ConnectionProfile> {
        let p = self.fetch_and_parse_conn_snapshot().await?;
        Ok(Json(p))
    }

    async fn fetch_and_parse_conn_snapshot(&self) -> Result<ConnectionProfile, String> {
        let raw = self
            .run_command("INFO CLIENTS")
            .await
            .map_err(|err| format!("Failed to fetch INFO CLIENTS: {err:#}"))?;
        let snapshot = parse_client_info(&raw)
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

        let change = connection_memory_change(&before.snapshot, &after.snapshot);
        let findings = connection_memory_findings(&change);
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
        let command = if matches!(&heap, HeapKind::Backing) {
            "MEMORY ARENA SUMMARY BACKING"
        } else {
            "MEMORY ARENA SUMMARY"
        };
        let raw = self
            .run_command(command)
            .await
            .map_err(|err| format!("Failed to fetch ARENA: {err}"))?;
        let snapshot = parse_arena_summary(&raw, heap)
            .map_err(|err| format!("Count not parse ARENA: {err}"))?;
        let report = ArenaReport::new(snapshot);
        Ok(Json(report))
    }

    #[tool(description = "Fetch Defrag Task Invocation Snapshot")]
    async fn fetch_defrag_snapshot(&self) -> CallResult<DefragSnapshot> {
        let metrics = self.metrics_collector.as_ref();
        let snapshot = if let Some(m) = metrics {
            let response = m
                .fetch()
                .await
                .map_err(|err| format!("Failed to fetch metrics: {err}"))?;
            parse_defrag_snapshot(&response)
        } else {
            let raw = self
                .run_command("INFO STATS")
                .await
                .map_err(|err| format!("Failed to fetch INFO STATS: {err}"))?;
            parse_defrag_info(&raw)
        };
        let snapshot = snapshot.map_err(|err| format!("Failed to parse metrics: {err}"))?;
        Ok(Json(snapshot))
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
