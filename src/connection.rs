use crate::parsing::info_iterator;
use rmcp::schemars;

#[derive(Debug, Default, serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ConnectionSnapshot {
    pub(crate) connected_clients: Option<u64>,
    pub(crate) client_read_buffer_bytes: Option<u64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ConnectionProfile {
    pub(crate) snapshot: ConnectionSnapshot,
    pub(crate) average_read_buffer_bytes_per_connection: Option<f64>,
}

pub(crate) fn average_read_buffer_bytes(snapshot: &ConnectionSnapshot) -> Option<f64> {
    let bytes = snapshot.client_read_buffer_bytes?;
    let connected = snapshot.connected_clients?;
    if connected == 0 {
        return None;
    }

    Some(bytes as f64 / connected as f64)
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ConnectionMemorySample {
    pub(crate) before: ConnectionProfile,
    pub(crate) after: ConnectionProfile,
    pub(crate) elapsed_ms: f64,
    pub(crate) change: ConnectionMemoryChange,
    pub(crate) findings: Option<String>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ConnectionMemoryChange {
    pub(crate) connected_clients_delta: Option<i128>,
    pub(crate) client_read_buffer_bytes_delta: Option<i128>,
}

pub(crate) fn connection_memory_change(
    before: &ConnectionSnapshot,
    after: &ConnectionSnapshot,
) -> ConnectionMemoryChange {
    let diff = |before: Option<u64>, after: Option<u64>| {
        before
            .zip(after)
            .map(|(before, after)| i128::from(after) - i128::from(before))
    };
    ConnectionMemoryChange {
        connected_clients_delta: diff(before.connected_clients, after.connected_clients),
        client_read_buffer_bytes_delta: diff(
            before.client_read_buffer_bytes,
            after.client_read_buffer_bytes,
        ),
    }
}

pub(crate) fn connection_memory_findings(change: &ConnectionMemoryChange) -> Option<String> {
    if let Some(0) = change.connected_clients_delta
        && let Some(n) = change.client_read_buffer_bytes_delta
        && n > 0
    {
        Some(format!(
            "aggregate read-buffer capacity increased by {n} bytes while connection count was equal at both measurements"
        ))
    } else {
        None
    }
}

pub(crate) fn parse_client_info(input: &str) -> anyhow::Result<ConnectionSnapshot> {
    let mut snapshot = ConnectionSnapshot::default();

    for (k, v) in info_iterator(input) {
        match k.trim() {
            "connected_clients" => {
                snapshot.connected_clients =
                    Some(crate::parsing::parse_u64(v, "connected_clients")?)
            }
            "client_read_buffer_bytes" => {
                snapshot.client_read_buffer_bytes =
                    Some(crate::parsing::parse_u64(v, "client_read_buffer_bytes")?)
            }
            _ => {}
        }
    }

    Ok(snapshot)
}
