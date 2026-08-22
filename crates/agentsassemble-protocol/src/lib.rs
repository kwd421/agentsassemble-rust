use agentsassemble_domain::{
    CapabilitySet, Participant, ProviderCatalog, PublicRoomSettings, Room, RoomEvent, SnapshotMode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientFrame {
    Subscribe {
        streams: Vec<String>,
        #[serde(default)]
        resume_from_seq: i64,
        #[serde(default)]
        server_challenge: Option<String>,
    },
    Command {
        request_id: String,
        action: String,
        #[serde(default = "empty_object")]
        payload: Value,
    },
    Ping {
        #[serde(default)]
        nonce: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ServerFrame {
    Snapshot(Box<RoomSnapshot>),
    Event {
        stream: &'static str,
        events: Vec<RoomEvent>,
        latest_seq: i64,
    },
    Ack(CommandAck),
    Nack(CommandNack),
    ResyncRequired {
        stream: &'static str,
        reason: String,
        latest_seq: i64,
    },
    Pong {
        nonce: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RoomSnapshot {
    pub stream: &'static str,
    pub room: Room,
    pub room_settings: PublicRoomSettings,
    pub participants: Vec<Participant>,
    pub agent_sessions: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provider_requests: Vec<Value>,
    pub active_turns: Vec<Value>,
    pub events: Vec<RoomEvent>,
    pub oldest_seq: i64,
    pub last_seq: i64,
    pub has_more_before: bool,
    pub resume_gap: bool,
    pub snapshot_mode: SnapshotMode,
    pub provider_catalog: ProviderCatalog,
    pub available_providers: Vec<Value>,
    pub capabilities: CapabilitySet,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub server_proof: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CommandAck {
    pub request_id: String,
    pub accepted: bool,
    pub action: String,
    pub result: Value,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deduplicated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandNack {
    pub request_id: String,
    pub accepted: bool,
    pub action: String,
    pub error: ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct TicketResponse {
    pub ticket: String,
    pub ttl_seconds: u64,
    pub server_proof_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LocalControlRequest {
    IssueTicket {
        request_id: String,
        meeting_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LocalControlResponse {
    Ok {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
        server_proof_key: String,
    },
    Error {
        request_id: String,
        code: String,
        message: String,
    },
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde skip predicates receive a reference.
const fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ClientFrame, CommandAck, ServerFrame};

    #[test]
    fn parses_existing_frontend_command_envelope() {
        let frame: ClientFrame = serde_json::from_value(json!({
            "op": "command",
            "request_id": "web-1",
            "action": "message.send",
            "payload": {"content": "hello"}
        }))
        .unwrap_or_else(|error| panic!("valid command envelope: {error}"));

        assert!(matches!(frame, ClientFrame::Command { request_id, .. } if request_id == "web-1"));
    }

    #[test]
    fn ack_keeps_existing_outer_shape() {
        let frame = ServerFrame::Ack(CommandAck {
            request_id: "web-1".to_owned(),
            accepted: true,
            action: "message.send".to_owned(),
            result: json!({"event_seq": 1}),
            deduplicated: false,
        });
        let value =
            serde_json::to_value(frame).unwrap_or_else(|error| panic!("serializable ACK: {error}"));
        assert_eq!(value["op"], "ack");
        assert_eq!(value["accepted"], true);
        assert!(value.get("deduplicated").is_none());
    }
}
