use crate::arena::CompactArenaReport;
use crate::connection::ConnectionSnapshot;
use crate::memory::MemorySnapshot;
use rmcp::schemars;

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct SourceTiming {
    pub(crate) source: String,
    // Offsets from the start of this report's collection, not Unix timestamps.
    pub(crate) started_ms: f64,
    pub(crate) finished_ms: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryGapReport {
    pub(crate) collected_at_unix_ms: u128,
    pub(crate) sources: Vec<SourceTiming>,
    pub(crate) rss_bytes: Option<u64>,
    pub(crate) db_used_bytes: Option<u64>,
    pub(crate) rss_minus_db_used_bytes: Option<i128>,
    pub(crate) rss_to_db_used_ratio: Option<f64>,
    pub(crate) connected_clients: Option<u64>,
    pub(crate) direct_connection_estimate_bytes: Option<u64>,
    pub(crate) client_read_buffer_capacity_bytes: Option<u64>,
    pub(crate) data_arena: CompactArenaReport,
    pub(crate) backing_arena: CompactArenaReport,
    pub(crate) findings: Vec<String>,
    pub(crate) limitations: Vec<String>,
}

impl MemoryGapReport {
    pub(crate) fn new(
        memory: MemorySnapshot,
        clients: ConnectionSnapshot,
        direct_connection_estimate_bytes: Option<u64>,
        data_arena: CompactArenaReport,
        backing_arena: CompactArenaReport,
        sources: Vec<SourceTiming>,
        collected_at_unix_ms: u128,
    ) -> Self {
        let rss_bytes = memory.used_memory_rss;
        let db_used_bytes = memory.used_memory;
        // Convert before subtracting: RSS can be lower than accounted allocations.
        let rss_minus_db_used_bytes = rss_bytes
            .zip(db_used_bytes)
            .map(|(rss, used)| i128::from(rss) - i128::from(used));
        let rss_to_db_used_ratio = match (rss_bytes, db_used_bytes) {
            (Some(rss), Some(used)) if used != 0 => Some(rss as f64 / used as f64),
            _ => None,
        };
        let connected_clients = clients.connected_clients;
        let client_read_buffer_capacity_bytes = clients.client_read_buffer_bytes;
        let mut findings = Vec::new();
        if let Some(gap) = rss_minus_db_used_bytes {
            findings.push(format!("RSS minus DB used memory is {gap} bytes"));
        }
        if let Some(bytes) = direct_connection_estimate_bytes {
            findings.push(format!(
                "HTTP connection memory estimate reports {bytes} bytes"
            ));
        }
        if backing_arena.total.wasted_bytes > 0 {
            findings.push(format!(
                "backing allocator reports {} wasted bytes in capacity outside the reach of DB object defragmentation",
                backing_arena.total.wasted_bytes
            ));
        }
        for (name, arena) in [("data", &data_arena), ("backing", &backing_arena)] {
            if let Some(class) = arena.top_waste_classes.first() {
                findings.push(format!(
                    "{name} heap largest wasted size class is {} bytes with {} wasted bytes",
                    class.block_size_bytes, class.wasted_bytes
                ));
            }
        }
        let mut limitations = vec![
            "sources are collected sequentially and are not an atomic snapshot".to_owned(),
            "arena heap totals cover reported shard threads and may not include every process allocation".to_owned(),
            "connection direct memory is approximate and includes a 9000-byte stack allowance per eligible connection; it excludes separately tracked read buffers and queues".to_owned(),
            "client read-buffer capacity includes shared and private buffers".to_owned(),
            "DB used memory and allocator totals can overlap and must not be added as independent components".to_owned(),
            "allocation estimates and buffer capacities are not measurements of their resident contribution to RSS".to_owned(),
        ];
        if rss_bytes.is_none() {
            limitations.push("INFO MEMORY did not provide used_memory_rss".to_owned());
        }
        if db_used_bytes.is_none() {
            limitations.push("INFO MEMORY did not provide used_memory".to_owned());
        }
        if connected_clients.is_none() {
            limitations.push("INFO CLIENTS did not provide connected_clients".to_owned());
        }
        if client_read_buffer_capacity_bytes.is_none() {
            limitations.push("INFO CLIENTS did not provide client_read_buffer_bytes".to_owned());
        }
        if direct_connection_estimate_bytes.is_none() {
            limitations
                .push("HTTP metrics did not provide dragonfly_connection_memory_bytes".to_owned());
        }
        Self {
            collected_at_unix_ms,
            sources,
            rss_bytes,
            db_used_bytes,
            rss_minus_db_used_bytes,
            rss_to_db_used_ratio,
            connected_clients,
            direct_connection_estimate_bytes,
            client_read_buffer_capacity_bytes,
            data_arena,
            backing_arena,
            findings,
            limitations,
        }
    }
}
