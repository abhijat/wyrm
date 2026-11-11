use anyhow::{anyhow, bail, ensure};
use rmcp::schemars;
use std::collections::BTreeMap;

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HeapKind {
    Data,
    Backing,
}

enum ArenaSection {
    ThreadId(u64),
    Aggregate,
}

#[derive(Default, serde::Serialize, schemars::JsonSchema)]
pub(crate) struct AllocStats {
    pub(crate) reserved_bytes: u64,
    pub(crate) committed_bytes: u64,
    pub(crate) used_bytes: u64,
    pub(crate) wasted_bytes: u64,
}

impl AllocStats {
    fn new(cols: &[&str]) -> anyhow::Result<Self> {
        let s = Self {
            reserved_bytes: cols[1].parse()?,
            committed_bytes: cols[2].parse()?,
            used_bytes: cols[3].parse()?,
            wasted_bytes: cols[4].parse()?,
        };
        Ok(s)
    }

    fn add(&mut self, other: &AllocStats) {
        self.reserved_bytes += other.reserved_bytes;
        self.committed_bytes += other.committed_bytes;
        self.used_bytes += other.used_bytes;
        self.wasted_bytes += other.wasted_bytes;
    }
}

#[derive(Default, serde::Serialize, schemars::JsonSchema)]
struct ArenaSummary {
    size_classes: BTreeMap<u64, AllocStats>,
    total: AllocStats,
}

pub(crate) struct ArenaSnapshot {
    heap: HeapKind,
    threads: BTreeMap<u64, ArenaSummary>,
    aggregate: ArenaSummary,
}

impl ArenaSnapshot {
    fn new(heap_kind: HeapKind) -> Self {
        Self {
            heap: heap_kind,
            threads: Default::default(),
            aggregate: Default::default(),
        }
    }
}

fn parse_alloc_stat(row: &str) -> anyhow::Result<(u64, AllocStats)> {
    let cols: Vec<_> = row.split_whitespace().collect();
    ensure!(cols.len() == 6, "Expected six colums in row {row}");

    let block_size = cols[0].parse()?;
    let alloc_stats = AllocStats::new(&cols)?;
    Ok((block_size, alloc_stats))
}

pub(crate) fn parse_arena_summary(input: &str, heap: HeapKind) -> anyhow::Result<ArenaSnapshot> {
    let mut arena_snapshot = ArenaSnapshot::new(heap);
    let mut seen_agg = false;

    let mut current_section: Option<ArenaSection> = None;

    for row in input.lines() {
        let row = row.trim();
        if row == "Arena statistics for machine:" {
            ensure!(!seen_agg, "duplicate machine section");
            current_section = Some(ArenaSection::Aggregate);
            continue;
        }

        if let Some(row) = row.strip_prefix("Arena statistics for thread ") {
            let thread_id = parse_thread_id(row)?;
            ensure!(
                !arena_snapshot.threads.contains_key(&thread_id),
                "duplicate thread id {thread_id}"
            );
            current_section = Some(ArenaSection::ThreadId(thread_id));
            arena_snapshot.threads.insert(thread_id, Default::default());
            continue;
        }

        if row.is_empty() {
            continue;
        }

        if row.starts_with("BlockSize") {
            continue;
        }

        if row.starts_with("Total:") {
            seen_agg = matches!(current_section, Some(ArenaSection::Aggregate));
            continue;
        }

        if row.starts_with("--- End mimalloc statistics") {
            break;
        }

        let (block_size, alloc_stats) = parse_alloc_stat(row)?;
        let entry =
            match current_section {
                None => bail!("Unexpected data row with no header"),
                Some(ArenaSection::Aggregate) => &mut arena_snapshot.aggregate,
                Some(ArenaSection::ThreadId(thread_id)) => arena_snapshot
                    .threads
                    .get_mut(&thread_id)
                    .ok_or_else(|| anyhow!("memory arena unexpected structure"))?,
            };

        ensure!(
            !entry.size_classes.contains_key(&block_size),
            "duplicate block size {block_size}"
        );
        entry.total.add(&alloc_stats);
        entry.size_classes.insert(block_size, alloc_stats);
    }

    ensure!(seen_agg, "Machine aggregate was never found in output");
    Ok(arena_snapshot)
}

fn parse_thread_id(row: &str) -> anyhow::Result<u64> {
    let thread_id = row
        .strip_suffix(':')
        .ok_or_else(|| anyhow!("memory arena unexpected structure"))?;
    let thread_id = thread_id.parse()?;
    Ok(thread_id)
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct SizeClassWaste {
    pub(crate) block_size_bytes: u64,
    pub(crate) committed_bytes: u64,
    pub(crate) used_bytes: u64,
    pub(crate) wasted_bytes: u64,
    pub(crate) waste_percent: Option<f64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct CompactArenaReport {
    pub(crate) heap: HeapKind,
    pub(crate) total: AllocStats,
    pub(crate) top_waste_classes: Vec<SizeClassWaste>,
    pub(crate) waste_class_count: usize,
    pub(crate) top_waste_classes_truncated: bool,
}

impl ArenaSnapshot {
    pub(crate) fn compact_report(self) -> CompactArenaReport {
        let ranked_waste = rank_waste(&self.aggregate);
        let waste_class_count = ranked_waste.len();
        let mut top_waste_classes = ranked_waste;
        top_waste_classes.truncate(5);
        CompactArenaReport {
            heap: self.heap,
            total: self.aggregate.total,
            top_waste_classes_truncated: waste_class_count > top_waste_classes.len(),
            top_waste_classes,
            waste_class_count,
        }
    }
}

fn rank_waste(summary: &ArenaSummary) -> Vec<SizeClassWaste> {
    let mut size_class_waste: Vec<_> = summary
        .size_classes
        .iter()
        .filter(|(_, alloc_stats)| alloc_stats.wasted_bytes > 0)
        .map(|(block_size, alloc_stats)| {
            let waste_percent = if alloc_stats.committed_bytes == 0 {
                None
            } else {
                Some(100.0 * alloc_stats.wasted_bytes as f64 / alloc_stats.committed_bytes as f64)
            };
            SizeClassWaste {
                block_size_bytes: *block_size,
                committed_bytes: alloc_stats.committed_bytes,
                used_bytes: alloc_stats.used_bytes,
                wasted_bytes: alloc_stats.wasted_bytes,
                waste_percent,
            }
        })
        .collect();
    size_class_waste.sort_unstable_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then_with(|| a.block_size_bytes.cmp(&b.block_size_bytes))
    });
    size_class_waste
}
