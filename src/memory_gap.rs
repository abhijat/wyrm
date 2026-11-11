use crate::arena::{ArenaComparison, CompactArenaReport};
use crate::connection::ConnectionSnapshot;
use crate::memory::MemorySnapshot;
use rmcp::schemars;

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct SourceTiming {
    pub(crate) source: String,
    // Report offsets use that report's start. Observation offsets use the overall
    // call's collection start, before the first arena scans and observation window.
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

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryGapToolResponse {
    #[serde(flatten)]
    pub(crate) snapshot: MemoryGapReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) comparison: Option<MemoryGapComparison>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryGapComparison {
    pub(crate) before: MemoryGapReport,
    pub(crate) requested_interval_ms: u64,
    pub(crate) elapsed_ms: f64,
    pub(crate) rss_bytes_delta: Option<i128>,
    pub(crate) db_used_bytes_delta: Option<i128>,
    pub(crate) rss_minus_db_used_bytes_delta: Option<i128>,
    pub(crate) connected_clients_delta: Option<i128>,
    pub(crate) direct_connection_estimate_bytes_delta: Option<i128>,
    pub(crate) client_read_buffer_capacity_bytes_delta: Option<i128>,
    pub(crate) data_arena: ArenaComparison,
    pub(crate) backing_arena: ArenaComparison,
    pub(crate) sampling: MemoryGapSampling,
    pub(crate) findings: Vec<String>,
    pub(crate) limitations: Vec<String>,
}

impl MemoryGapComparison {
    pub(crate) fn new(
        before: MemoryGapReport,
        after: &MemoryGapReport,
        data_arena: ArenaComparison,
        backing_arena: ArenaComparison,
        requested_interval_ms: u64,
        elapsed_ms: f64,
        sampling: MemoryGapSampling,
    ) -> Self {
        let signed_delta = |before: Option<u64>, after: Option<u64>| {
            before
                .zip(after)
                .map(|(before, after)| i128::from(after) - i128::from(before))
        };
        let rss_bytes_delta = signed_delta(before.rss_bytes, after.rss_bytes);
        let db_used_bytes_delta = signed_delta(before.db_used_bytes, after.db_used_bytes);
        let rss_minus_db_used_bytes_delta = before
            .rss_minus_db_used_bytes
            .zip(after.rss_minus_db_used_bytes)
            .map(|(before, after)| after - before);
        let connected_clients_delta =
            signed_delta(before.connected_clients, after.connected_clients);
        let direct_connection_estimate_bytes_delta = signed_delta(
            before.direct_connection_estimate_bytes,
            after.direct_connection_estimate_bytes,
        );
        let client_read_buffer_capacity_bytes_delta = signed_delta(
            before.client_read_buffer_capacity_bytes,
            after.client_read_buffer_capacity_bytes,
        );

        let mut findings = Vec::new();
        for (name, unit, delta) in [
            ("RSS", "bytes", rss_bytes_delta),
            ("DB used memory", "bytes", db_used_bytes_delta),
            (
                "RSS minus DB used memory",
                "bytes",
                rss_minus_db_used_bytes_delta,
            ),
            ("connected clients", "clients", connected_clients_delta),
            (
                "direct connection estimate",
                "bytes",
                direct_connection_estimate_bytes_delta,
            ),
            (
                "client read-buffer capacity",
                "bytes",
                client_read_buffer_capacity_bytes_delta,
            ),
        ] {
            if let Some(delta) = delta.filter(|delta| *delta != 0) {
                findings.push(format!("{name} changed by {delta} {unit}"));
            }
        }
        for (name, arena) in [("data", &data_arena), ("backing", &backing_arena)] {
            if arena.wasted_bytes_delta != 0 {
                findings.push(format!(
                    "{name} heap total wasted capacity changed by {} bytes",
                    arena.wasted_bytes_delta
                ));
            }
            if let Some(class) = arena.top_waste_increases.first() {
                findings.push(format!(
                    "{name} heap largest size-class waste increase was {} bytes at block size {} bytes",
                    class.wasted_bytes_delta, class.block_size_bytes
                ));
            }
            if let Some(class) = arena.top_waste_decreases.first() {
                findings.push(format!(
                    "{name} heap largest size-class waste decrease was {} bytes at block size {} bytes",
                    -class.wasted_bytes_delta, class.block_size_bytes
                ));
            }
        }
        let mut limitations = vec![
            "intermediate sampling runs during the requested observation window; actual start-to-start elapsed time can be longer because collection takes time".to_owned(),
            "finite intermediate samples cannot establish the continuous peak or rule out churn between observations".to_owned(),
            "each endpoint is sequential and not an atomic snapshot".to_owned(),
            "observed changes do not establish that connection changes caused RSS changes or that defragmentation caused allocator changes".to_owned(),
        ];
        if rss_bytes_delta.is_none() {
            limitations
                .push("RSS delta is unavailable because an endpoint value is missing".to_owned());
        }
        if db_used_bytes_delta.is_none() {
            limitations.push(
                "DB used-memory delta is unavailable because an endpoint value is missing"
                    .to_owned(),
            );
        }

        Self {
            before,
            requested_interval_ms,
            elapsed_ms,
            rss_bytes_delta,
            db_used_bytes_delta,
            rss_minus_db_used_bytes_delta,
            connected_clients_delta,
            direct_connection_estimate_bytes_delta,
            client_read_buffer_capacity_bytes_delta,
            data_arena,
            backing_arena,
            sampling,
            findings,
            limitations,
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryGapObservation {
    pub(crate) label: String,
    // These offsets and source offsets share the overall call's collection start.
    // The finish time covers INFO and HTTP collection, not arena scans.
    pub(crate) started_ms: f64,
    pub(crate) finished_ms: f64,
    pub(crate) sources: Vec<SourceTiming>,
    pub(crate) rss_bytes: Option<u64>,
    pub(crate) db_used_bytes: Option<u64>,
    pub(crate) rss_minus_db_used_bytes: Option<i128>,
    pub(crate) connected_clients: Option<u64>,
    pub(crate) direct_connection_estimate_bytes: Option<u64>,
    pub(crate) client_read_buffer_capacity_bytes: Option<u64>,
    pub(crate) error: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ObservedRange {
    pub(crate) min: Option<i128>,
    pub(crate) max: Option<i128>,
    pub(crate) valid_sample_count: usize,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryGapSampleRanges {
    pub(crate) rss_bytes: ObservedRange,
    pub(crate) db_used_bytes: ObservedRange,
    pub(crate) rss_minus_db_used_bytes: ObservedRange,
    pub(crate) connected_clients: ObservedRange,
    pub(crate) direct_connection_estimate_bytes: ObservedRange,
    pub(crate) client_read_buffer_capacity_bytes: ObservedRange,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct MemoryGapSampling {
    pub(crate) sample_period_ms: u64,
    pub(crate) observations: Vec<MemoryGapObservation>,
    pub(crate) successful_sample_count: usize,
    pub(crate) failed_sample_count: usize,
    pub(crate) ranges: MemoryGapSampleRanges,
    pub(crate) findings: Vec<String>,
    pub(crate) limitations: Vec<String>,
}

impl MemoryGapSampling {
    pub(crate) fn new(observations: Vec<MemoryGapObservation>) -> Self {
        let range = |get: fn(&MemoryGapObservation) -> Option<i128>| {
            let values: Vec<_> = observations.iter().filter_map(get).collect();
            ObservedRange {
                min: values.iter().min().copied(),
                max: values.iter().max().copied(),
                valid_sample_count: values.len(),
            }
        };
        let ranges = MemoryGapSampleRanges {
            rss_bytes: range(|sample| sample.rss_bytes.map(i128::from)),
            db_used_bytes: range(|sample| sample.db_used_bytes.map(i128::from)),
            rss_minus_db_used_bytes: range(|sample| sample.rss_minus_db_used_bytes),
            connected_clients: range(|sample| sample.connected_clients.map(i128::from)),
            direct_connection_estimate_bytes: range(|sample| {
                sample.direct_connection_estimate_bytes.map(i128::from)
            }),
            client_read_buffer_capacity_bytes: range(|sample| {
                sample.client_read_buffer_capacity_bytes.map(i128::from)
            }),
        };
        let successful_sample_count = observations
            .iter()
            .filter(|sample| sample.error.is_none())
            .count();
        let failed_sample_count = observations.len() - successful_sample_count;
        let mut findings = Vec::new();
        add_excursion_findings(&mut findings, &observations, "RSS", "bytes", |sample| {
            sample.rss_bytes.map(i128::from)
        });
        add_excursion_findings(
            &mut findings,
            &observations,
            "DB used memory",
            "bytes",
            |sample| sample.db_used_bytes.map(i128::from),
        );
        add_excursion_findings(
            &mut findings,
            &observations,
            "RSS minus DB used memory",
            "bytes",
            |sample| sample.rss_minus_db_used_bytes,
        );
        add_excursion_findings(
            &mut findings,
            &observations,
            "connected clients",
            "clients",
            |sample| sample.connected_clients.map(i128::from),
        );
        add_excursion_findings(
            &mut findings,
            &observations,
            "direct connection estimate",
            "bytes",
            |sample| sample.direct_connection_estimate_bytes.map(i128::from),
        );
        add_excursion_findings(
            &mut findings,
            &observations,
            "client read-buffer capacity",
            "bytes",
            |sample| sample.client_read_buffer_capacity_bytes.map(i128::from),
        );
        if failed_sample_count > 0 {
            findings.push(format!(
                "{failed_sample_count} intermediate sample(s) failed; any partial values remain included in ranges"
            ));
        }
        let limitations = vec![
            "observation and source offsets are measured from the start of the first full collection; the requested observation window starts after that collection finishes".to_owned(),
            "one-second sampling slots missed during slow collection are skipped; actual timing is recorded for each attempted observation".to_owned(),
            "intermediate samples are finite observations and cannot establish the continuous peak or rule out churn between samples".to_owned(),
            "unchanged values apply only to the available samples and do not establish continuous stability".to_owned(),
            "intermediate sampling uses INFO and HTTP sources; allocator arenas are sampled only at the full endpoints".to_owned(),
        ];
        Self {
            sample_period_ms: 1000,
            observations,
            successful_sample_count,
            failed_sample_count,
            ranges,
            findings,
            limitations,
        }
    }
}

fn add_excursion_findings(
    findings: &mut Vec<String>,
    samples: &[MemoryGapObservation],
    name: &str,
    unit: &str,
    get: fn(&MemoryGapObservation) -> Option<i128>,
) {
    let intermediate: Vec<_> = samples
        .iter()
        .skip(1)
        .take(samples.len().saturating_sub(2))
        .filter_map(get)
        .collect();
    if let (Some(first), Some(last)) = (samples.first().and_then(get), samples.last().and_then(get))
    {
        let endpoints_min = first.min(last);
        let endpoints_max = first.max(last);
        if let Some(maximum) = intermediate.iter().max()
            && *maximum > endpoints_max
        {
            findings.push(format!(
                "intermediate {name} reached {maximum} {unit}, {excess} {unit} above the higher endpoint value {endpoints_max} {unit}",
                excess = *maximum - endpoints_max
            ));
        }
        if let Some(minimum) = intermediate.iter().min()
            && *minimum < endpoints_min
        {
            findings.push(format!(
                "intermediate {name} reached {minimum} {unit}, {shortfall} {unit} below the lower endpoint value {endpoints_min} {unit}",
                shortfall = endpoints_min - *minimum
            ));
        }
    }
    let valid_values: Vec<_> = samples.iter().filter_map(get).collect();
    if valid_values.len() >= 2 && valid_values.iter().all(|value| *value == valid_values[0]) {
        findings.push(format!(
            "{name} was unchanged at {} {unit} across {} valid readings out of {} attempted observations",
            valid_values[0], valid_values.len(), samples.len()
        ));
    }
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
