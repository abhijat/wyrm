use crate::parsing::info_iterator;
use rmcp::schemars;

#[derive(serde::Serialize, schemars::JsonSchema, Default)]
pub(crate) struct ConnectionSnapshot {
    pub(crate) connected_clients: Option<u64>,
    pub(crate) client_read_buffer_bytes: Option<u64>,
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
