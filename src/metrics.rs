use prometheus_parse::{Scrape, Value};
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

    pub(crate) async fn collect_connection_memory_bytes(&self) -> anyhow::Result<Option<u64>> {
        let text = self.fetch().await?;
        parse_connection_memory_bytes(&text)
    }
}

pub(crate) fn parse_connection_memory_bytes(input: &str) -> anyhow::Result<Option<u64>> {
    let lines = input.lines().map(|line| Ok(line.to_owned()));
    let scrape = Scrape::parse(lines)?;
    let mut result = None;
    for sample in scrape.samples {
        if sample.metric != "dragonfly_connection_memory_bytes" {
            continue;
        }
        let value = match sample.value {
            Value::Gauge(value) => value,
            _ => anyhow::bail!("dragonfly_connection_memory_bytes is not a Gauge"),
        };
        anyhow::ensure!(
            value.is_finite()
                && value >= 0.0
                && value.fract() == 0.0
                && value < 18_446_744_073_709_551_616.0,
            "dragonfly_connection_memory_bytes must be a finite unsigned integer, got {value}"
        );
        result = Some(value as u64);
    }
    Ok(result)
}
