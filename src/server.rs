use crate::arena::{HeapKind, parse_arena_summary};
use crate::connection::parse_client_info;
use crate::memory::{MemorySnapshot, parse_memory_info};
use crate::memory_gap::{MemoryGapReport, SourceTiming};
use crate::metrics::MetricsCollector;
use redis::aio::ConnectionManager;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{ServerCapabilities, ServerConfig};
use rmcp::{Json, ServerHandler, tool, tool_handler, tool_router};
use tokio::time::Instant;

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

    #[tool(
        description = "Explain the RSS versus database memory gap from one sequential snapshot. Requires configured HTTP metrics and scans both arena heaps, which may be costly on large heaps."
    )]
    async fn explain_memory_gap(&self) -> CallResult<MemoryGapReport> {
        let metrics = self.metrics_collector.as_ref().ok_or_else(|| {
            "Failed to collect connection memory gauge: WYRM_METRICS_URL is not set up".to_owned()
        })?;
        let start = Instant::now();
        let collected_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| format!("Failed to timestamp collection: {err}"))?
            .as_millis();
        let mut sources = Vec::new();

        let source_start = Instant::now();
        let raw = self
            .run_command("INFO MEMORY")
            .await
            .map_err(|err| format!("Failed to fetch INFO MEMORY: {err:#}"))?;
        sources.push(source_timing(
            "INFO MEMORY",
            start,
            source_start,
            Instant::now(),
        ));
        let memory = parse_memory_info(&raw)
            .map_err(|err| format!("Failed to parse INFO MEMORY: {err:#}"))?;

        let source_start = Instant::now();
        let raw = self
            .run_command("INFO CLIENTS")
            .await
            .map_err(|err| format!("Failed to fetch INFO CLIENTS: {err:#}"))?;
        sources.push(source_timing(
            "INFO CLIENTS",
            start,
            source_start,
            Instant::now(),
        ));
        let clients = parse_client_info(&raw)
            .map_err(|err| format!("Failed to parse INFO CLIENTS: {err:#}"))?;

        let source_start = Instant::now();
        let direct_connection_estimate_bytes = metrics
            .collect_connection_memory_bytes()
            .await
            .map_err(|err| {
                format!("Failed to collect dragonfly_connection_memory_bytes: {err:#}")
            })?;
        sources.push(source_timing(
            "HTTP dragonfly_connection_memory_bytes",
            start,
            source_start,
            Instant::now(),
        ));

        let source_start = Instant::now();
        let raw = self
            .run_command("MEMORY ARENA SUMMARY")
            .await
            .map_err(|err| format!("Failed to fetch MEMORY ARENA SUMMARY: {err:#}"))?;
        sources.push(source_timing(
            "MEMORY ARENA SUMMARY",
            start,
            source_start,
            Instant::now(),
        ));
        let data_arena = parse_arena_summary(&raw, HeapKind::Data)
            .map_err(|err| format!("Failed to parse MEMORY ARENA SUMMARY: {err:#}"))?
            .compact_report();

        let source_start = Instant::now();
        let raw = self
            .run_command("MEMORY ARENA SUMMARY BACKING")
            .await
            .map_err(|err| format!("Failed to fetch MEMORY ARENA SUMMARY BACKING: {err:#}"))?;
        sources.push(source_timing(
            "MEMORY ARENA SUMMARY BACKING",
            start,
            source_start,
            Instant::now(),
        ));
        let backing_arena = parse_arena_summary(&raw, HeapKind::Backing)
            .map_err(|err| format!("Failed to parse MEMORY ARENA SUMMARY BACKING: {err:#}"))?
            .compact_report();

        Ok(Json(MemoryGapReport::new(
            memory,
            clients,
            direct_connection_estimate_bytes,
            data_arena,
            backing_arena,
            sources,
            collected_at_unix_ms,
        )))
    }
}

fn source_timing(
    source: &str,
    start: Instant,
    source_start: Instant,
    source_end: Instant,
) -> SourceTiming {
    SourceTiming {
        source: source.to_owned(),
        started_ms: source_start.duration_since(start).as_secs_f64() * 1000.0,
        finished_ms: source_end.duration_since(start).as_secs_f64() * 1000.0,
    }
}

#[tool_handler]
impl ServerHandler for Wyrm {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

type CallResult<T> = Result<Json<T>, String>;
