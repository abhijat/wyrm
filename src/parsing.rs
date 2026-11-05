use anyhow::Context;

pub(crate) fn parse_u64(s: &str, field: &str) -> anyhow::Result<u64> {
    let v = s.parse().context(format!("invalid {field}"))?;
    Ok(v)
}

pub(crate) fn info_iterator(input: &str) -> impl Iterator<Item = (&str, &str)> {
    input
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once(':'))
}
