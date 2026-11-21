use crate::parsing::info_iterator;
use rmcp::schemars;

#[derive(Clone, Copy, Default, serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryConsumers {
    pub(crate) pipeline_cache_bytes: Option<u64>,
    pub(crate) dispatch_queue_bytes: Option<u64>,
    pub(crate) pipeline_queue_bytes: Option<u64>,
    pub(crate) used_memory_lua: Option<u64>,
    pub(crate) tls_bytes: Option<u64>,
    pub(crate) replication_streaming_buffer_bytes: Option<u64>,
    pub(crate) replication_full_sync_buffer_bytes: Option<u64>,
    pub(crate) psync_buffer_bytes: Option<u64>,
}

#[derive(Default, serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemorySnapshot {
    pub(crate) used_memory: Option<u64>,
    pub(crate) used_memory_rss: Option<u64>,
    maxmemory: Option<u64>,
    pub(crate) named_consumers: MemoryConsumers,
}

pub(crate) fn parse_memory_info(input: &str) -> anyhow::Result<MemorySnapshot> {
    let mut snapshot = MemorySnapshot::default();
    for (k, v) in info_iterator(input) {
        match k.trim() {
            "used_memory" => {
                snapshot.used_memory = Some(crate::parsing::parse_u64(v, "used_memory")?)
            }
            "used_memory_rss" => {
                snapshot.used_memory_rss = Some(crate::parsing::parse_u64(v, "used_memory_rss")?)
            }
            "maxmemory" => snapshot.maxmemory = Some(crate::parsing::parse_u64(v, "maxmemory")?),
            "pipeline_cache_bytes" => {
                snapshot.named_consumers.pipeline_cache_bytes =
                    Some(crate::parsing::parse_u64(v, "pipeline_cache_bytes")?)
            }
            "dispatch_queue_bytes" => {
                snapshot.named_consumers.dispatch_queue_bytes =
                    Some(crate::parsing::parse_u64(v, "dispatch_queue_bytes")?)
            }
            "pipeline_queue_bytes" => {
                snapshot.named_consumers.pipeline_queue_bytes =
                    Some(crate::parsing::parse_u64(v, "pipeline_queue_bytes")?)
            }
            "used_memory_lua" => {
                snapshot.named_consumers.used_memory_lua =
                    Some(crate::parsing::parse_u64(v, "used_memory_lua")?)
            }
            "tls_bytes" => {
                snapshot.named_consumers.tls_bytes =
                    Some(crate::parsing::parse_u64(v, "tls_bytes")?)
            }
            "replication_streaming_buffer_bytes" => {
                snapshot.named_consumers.replication_streaming_buffer_bytes = Some(
                    crate::parsing::parse_u64(v, "replication_streaming_buffer_bytes")?,
                )
            }
            "replication_full_sync_buffer_bytes" => {
                snapshot.named_consumers.replication_full_sync_buffer_bytes = Some(
                    crate::parsing::parse_u64(v, "replication_full_sync_buffer_bytes")?,
                )
            }
            "psync_buffer_bytes" => {
                snapshot.named_consumers.psync_buffer_bytes =
                    Some(crate::parsing::parse_u64(v, "psync_buffer_bytes")?)
            }
            _ => {}
        }
    }
    Ok(snapshot)
}
