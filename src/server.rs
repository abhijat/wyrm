use crate::arena::{ArenaSnapshot, HeapKind, parse_arena_summary};
use crate::connection::{ConnectionSnapshot, parse_client_info};
use crate::memory::{MemorySnapshot, parse_memory_info};
use crate::memory_gap::{
    MemoryGapComparison, MemoryGapObservation, MemoryGapReport, MemoryGapSampling,
    MemoryGapToolResponse, SourceTiming,
};
use crate::metrics::MetricsCollector;
use redis::aio::ConnectionManager;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerConfig};
use rmcp::{Json, ServerHandler, schemars, tool, tool_handler, tool_router};
use tokio::time::{Duration, Instant, sleep_until};

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
        description = "Explain the RSS versus database memory gap from one sequential snapshot, or compare two full snapshots with intermediate INFO and HTTP samples using interval_ms. A supplied interval must be 1000–10000 ms. Requires configured HTTP metrics and scans both arena heaps at the endpoints, which may be costly on large heaps."
    )]
    async fn explain_memory_gap(
        &self,
        Parameters(MemoryGapParams { interval_ms }): Parameters<MemoryGapParams>,
    ) -> CallResult<MemoryGapToolResponse> {
        if let Some(interval_ms) = interval_ms
            && !(1000..=10000).contains(&interval_ms)
        {
            return Err(format!(
                "interval_ms must be between 1000 and 10000, got {interval_ms}"
            ));
        }

        let before_start = Instant::now();
        let before = self.collect_memory_gap(before_start, "before").await?;
        let Some(requested_interval_ms) = interval_ms else {
            return Ok(Json(MemoryGapToolResponse {
                snapshot: before.report,
                comparison: None,
            }));
        };

        let mut observations = vec![before.observation];
        let window_start = Instant::now();
        let deadline = window_start + Duration::from_millis(requested_interval_ms);
        let mut next_tick = window_start + Duration::from_secs(1);
        while next_tick < deadline {
            sleep_until(next_tick).await;
            if Instant::now() >= deadline {
                break;
            }
            let observation = self
                .collect_cheap_snapshot(before_start, "intermediate")
                .await
                .observation;
            observations.push(observation);
            next_tick += Duration::from_secs(1);
            let now = Instant::now();
            while next_tick <= now {
                next_tick += Duration::from_secs(1);
            }
        }
        sleep_until(deadline).await;
        let after_start = Instant::now();
        let elapsed_ms = after_start.duration_since(before_start).as_secs_f64() * 1000.0;
        let after = self.collect_memory_gap(before_start, "after").await?;
        observations.push(after.observation);
        let sampling = MemoryGapSampling::new(observations);
        let comparison = MemoryGapComparison::new(
            before.report,
            &after.report,
            before.data_arena.compare(&after.data_arena),
            before.backing_arena.compare(&after.backing_arena),
            requested_interval_ms,
            elapsed_ms,
            sampling,
        );
        Ok(Json(MemoryGapToolResponse {
            snapshot: after.report,
            comparison: Some(comparison),
        }))
    }

    async fn collect_memory_gap(
        &self,
        sampling_start: Instant,
        label: &str,
    ) -> Result<CollectedMemoryGap, String> {
        let report_start = Instant::now();
        let collected_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| format!("Failed to timestamp collection: {err}"))?
            .as_millis();
        let cheap = self.collect_cheap_snapshot(sampling_start, label).await;
        if let Some(error) = cheap.observation.error.as_ref() {
            return Err(format!(
                "Failed to collect {label} memory snapshot: {error}"
            ));
        }
        let offset_ms = report_start.duration_since(sampling_start).as_secs_f64() * 1000.0;
        let mut sources: Vec<_> = cheap
            .observation
            .sources
            .iter()
            .map(|source| SourceTiming {
                source: source.source.clone(),
                started_ms: source.started_ms - offset_ms,
                finished_ms: source.finished_ms - offset_ms,
            })
            .collect();

        let source_start = Instant::now();
        let raw = self
            .run_command("MEMORY ARENA SUMMARY")
            .await
            .map_err(|err| format!("Failed to fetch MEMORY ARENA SUMMARY: {err:#}"))?;
        sources.push(source_timing(
            "MEMORY ARENA SUMMARY",
            report_start,
            source_start,
            Instant::now(),
        ));
        let data_arena = parse_arena_summary(&raw, HeapKind::Data)
            .map_err(|err| format!("Failed to parse MEMORY ARENA SUMMARY: {err:#}"))?;

        let source_start = Instant::now();
        let raw = self
            .run_command("MEMORY ARENA SUMMARY BACKING")
            .await
            .map_err(|err| format!("Failed to fetch MEMORY ARENA SUMMARY BACKING: {err:#}"))?;
        sources.push(source_timing(
            "MEMORY ARENA SUMMARY BACKING",
            report_start,
            source_start,
            Instant::now(),
        ));
        let backing_arena = parse_arena_summary(&raw, HeapKind::Backing)
            .map_err(|err| format!("Failed to parse MEMORY ARENA SUMMARY BACKING: {err:#}"))?;

        let report = MemoryGapReport::new(
            cheap.memory,
            cheap.clients,
            cheap.direct_connection_estimate_bytes,
            data_arena.compact_report(),
            backing_arena.compact_report(),
            sources,
            collected_at_unix_ms,
        );
        Ok(CollectedMemoryGap {
            report,
            data_arena,
            backing_arena,
            observation: cheap.observation,
        })
    }

    async fn collect_cheap_snapshot(&self, sampling_start: Instant, label: &str) -> CheapSnapshot {
        let started = Instant::now();
        let mut memory = MemorySnapshot::default();
        let mut clients = ConnectionSnapshot::default();
        let mut direct_connection_estimate_bytes = None;
        let mut sources = Vec::new();
        let mut error = None;

        let source_start = Instant::now();
        match self.run_command("INFO MEMORY").await {
            Ok(raw) => {
                sources.push(source_timing(
                    "INFO MEMORY",
                    sampling_start,
                    source_start,
                    Instant::now(),
                ));
                match parse_memory_info(&raw) {
                    Ok(snapshot) => memory = snapshot,
                    Err(err) => error = Some(format!("Failed to parse INFO MEMORY: {err:#}")),
                }
            }
            Err(err) => {
                sources.push(source_timing(
                    "INFO MEMORY",
                    sampling_start,
                    source_start,
                    Instant::now(),
                ));
                error = Some(format!("Failed to fetch INFO MEMORY: {err:#}"));
            }
        }

        if error.is_none() {
            let source_start = Instant::now();
            match self.run_command("INFO CLIENTS").await {
                Ok(raw) => {
                    sources.push(source_timing(
                        "INFO CLIENTS",
                        sampling_start,
                        source_start,
                        Instant::now(),
                    ));
                    match parse_client_info(&raw) {
                        Ok(snapshot) => clients = snapshot,
                        Err(err) => error = Some(format!("Failed to parse INFO CLIENTS: {err:#}")),
                    }
                }
                Err(err) => {
                    sources.push(source_timing(
                        "INFO CLIENTS",
                        sampling_start,
                        source_start,
                        Instant::now(),
                    ));
                    error = Some(format!("Failed to fetch INFO CLIENTS: {err:#}"));
                }
            }
        }

        if error.is_none() {
            if let Some(metrics) = self.metrics_collector.as_ref() {
                let source_start = Instant::now();
                match metrics.collect_connection_memory_bytes().await {
                    Ok(value) => direct_connection_estimate_bytes = value,
                    Err(err) => {
                        error = Some(format!(
                            "Failed to collect dragonfly_connection_memory_bytes: {err:#}"
                        ));
                    }
                }
                sources.push(source_timing(
                    "HTTP dragonfly_connection_memory_bytes",
                    sampling_start,
                    source_start,
                    Instant::now(),
                ));
            } else {
                error =
                    Some("WYRM_METRICS_URL is not set up for connection memory gauge".to_owned());
            }
        }

        let rss_bytes = memory.used_memory_rss;
        let db_used_bytes = memory.used_memory;
        let observation = MemoryGapObservation {
            label: label.to_owned(),
            started_ms: started.duration_since(sampling_start).as_secs_f64() * 1000.0,
            finished_ms: Instant::now().duration_since(sampling_start).as_secs_f64() * 1000.0,
            sources,
            rss_bytes,
            db_used_bytes,
            rss_minus_db_used_bytes: rss_bytes
                .zip(db_used_bytes)
                .map(|(rss, db)| i128::from(rss) - i128::from(db)),
            connected_clients: clients.connected_clients,
            direct_connection_estimate_bytes,
            client_read_buffer_capacity_bytes: clients.client_read_buffer_bytes,
            error,
        };
        CheapSnapshot {
            memory,
            clients,
            direct_connection_estimate_bytes,
            observation,
        }
    }
}

struct CollectedMemoryGap {
    report: MemoryGapReport,
    data_arena: ArenaSnapshot,
    backing_arena: ArenaSnapshot,
    observation: MemoryGapObservation,
}

struct CheapSnapshot {
    memory: MemorySnapshot,
    clients: ConnectionSnapshot,
    direct_connection_estimate_bytes: Option<u64>,
    observation: MemoryGapObservation,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct MemoryGapParams {
    interval_ms: Option<u64>,
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
