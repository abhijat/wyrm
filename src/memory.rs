use rmcp::schemars;

#[derive(Default, serde::Serialize, schemars::JsonSchema)]
pub(crate)struct MemorySnapshot {
    used_memory: Option<u64>,
    used_memory_rss: Option<u64>,
    maxmemory: Option<u64>,
}

pub(crate)fn parse_memory_info(input: &str) -> anyhow::Result<MemorySnapshot> {
    let mut snapshot = MemorySnapshot::default();
    for row in input.lines() {
        if row.is_empty() {
            continue;
        }

        if row.starts_with('#') {
            continue;
        }

        if let Some((k, v)) = row.split_once(':') {
            match k.trim() {
                "used_memory" => snapshot.used_memory = Some(crate::parsing::parse_u64(v, "used_memory")?),
                "used_memory_rss" => {
                    snapshot.used_memory_rss = Some(crate::parsing::parse_u64(v, "used_memory_rss")?)
                }
                "maxmemory" => snapshot.maxmemory = Some(crate::parsing::parse_u64(v, "maxmemory")?),
                _ => {}
            }
        }
    }
    Ok(snapshot)
}
