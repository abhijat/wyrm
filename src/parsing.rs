use anyhow::Context;

pub(crate)fn parse_u64(s: &str, field: &str) -> anyhow::Result<u64> {
    let v = s.parse().context(format!("invalid {field}"))?;
    Ok(v)
}