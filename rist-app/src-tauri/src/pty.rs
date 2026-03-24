use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AgentOutputPayload {
    pub agent_id: String,
    pub data: String,
}

pub fn bytes_to_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
