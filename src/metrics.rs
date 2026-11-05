use crate::parsing::{info_iterator, parse_u64};
use anyhow::ensure;
use prometheus_parse::{Sample, Scrape, Value};
use rmcp::schemars;
use std::time::Duration;

pub(crate) struct MetricsCollector {
    client: reqwest::Client,
    url: reqwest::Url,
}

impl MetricsCollector {
    pub(crate) fn new(url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()?,
            url: url.parse()?,
        })
    }

    pub(crate) async fn fetch(&self) -> anyhow::Result<String> {
        let response = self
            .client
            .get(self.url.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(response)
    }
}

#[derive(Default, serde::Serialize, schemars::JsonSchema)]
pub(crate) struct DefragSnapshot {
    invocations: Option<u64>,
    attempts: Option<u64>,
    objects_moved: Option<u64>,
    skipped_mem_under_threshold: Option<u64>,
    skipped_within_check_interval: Option<u64>,
    skipped_not_enough_fragmentation: Option<u64>,
}

fn parse_value_as_unsigned(value: Value) -> Option<u64> {
    if let Value::Counter(c) = value {
        if c.is_nan()
            || c.is_infinite()
            || c.is_sign_negative()
            || c < 0.0
            || c.fract() != 0.0
            || !c.is_finite()
            || c >= 18_446_744_073_709_551_616.0
        {
            return None;
        }

        return Some(c as u64);
    }
    None
}

impl DefragSnapshot {
    fn set_field(&mut self, sample: Sample) {
        let mut apply_label = |c: u64| {
            if let Some(reason) = sample.labels.get("reason") {
                match reason {
                    "mem_under_threshold" => self.skipped_mem_under_threshold = Some(c),
                    "within_check_interval" => self.skipped_within_check_interval = Some(c),
                    "not_enough_fragmentation" => self.skipped_not_enough_fragmentation = Some(c),
                    _ => {}
                }
            }
        };

        if let Some(c) = parse_value_as_unsigned(sample.value) {
            match sample.metric.as_str() {
                "dragonfly_defrag_invocations" => self.invocations = Some(c),
                "dragonfly_defrag_attempts" => self.attempts = Some(c),
                "dragonfly_defrag_objects_moved" => self.objects_moved = Some(c),
                "dragonfly_defrag_skipped_total" => apply_label(c),
                _ => {}
            }
        }
    }

    fn is_populated(&self) -> bool {
        self.invocations.is_some()
            || self.attempts.is_some()
            || self.objects_moved.is_some()
            || self.skipped_mem_under_threshold.is_some()
            || self.skipped_within_check_interval.is_some()
            || self.skipped_not_enough_fragmentation.is_some()
    }
}

pub(crate) fn parse_defrag_snapshot(input: &str) -> anyhow::Result<DefragSnapshot> {
    let mut snapshot = DefragSnapshot::default();
    let lines = input.lines().map(|line| Ok(line.to_owned()));
    let scrape = Scrape::parse(lines)?;
    for sample in scrape.samples {
        snapshot.set_field(sample);
    }
    ensure!(snapshot.is_populated(), "Snapshot is empty");
    Ok(snapshot)
}

pub(crate) fn parse_defrag_info(input: &str) -> anyhow::Result<DefragSnapshot> {
    let mut snapshot = DefragSnapshot::default();
    for (k, v) in info_iterator(input) {
        match k.trim() {
            "defrag_task_invocation_total" => {
                snapshot.invocations = Some(parse_u64(v, "invocations")?)
            }
            "defrag_attempt_total" => snapshot.attempts = Some(parse_u64(v, "attempts")?),
            "defrag_realloc_total" => snapshot.objects_moved = Some(parse_u64(v, "objects_moved")?),
            _ => {}
        }
    }
    ensure!(
        snapshot.is_populated(),
        "No usable defrag counters found in INFO STATS"
    );
    Ok(snapshot)
}
