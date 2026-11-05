use crate::parsing::info_iterator;
use rmcp::schemars;

#[derive(Default, serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemorySnapshot {
    used_memory: Option<u64>,
    used_memory_rss: Option<u64>,
    maxmemory: Option<u64>,
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
            _ => {}
        }
    }
    Ok(snapshot)
}
