use agentsassemble_domain::{
    AgentSession, CapabilitySet, Participant, ProviderAvailability, ProviderCatalog,
    PublicRoomSettings, Room, RoomEvent, SnapshotMode,
};
pub use agentsassemble_domain::{
    MAX_ATTACHMENT_BYTES, MAX_LOBBY_MESSAGE_PINS, MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES,
    MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS, MAX_MESSAGE_ATTACHMENTS_PER_EVENT,
    MAX_MESSAGE_EVENT_ID_BYTES, MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS,
    MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS, MAX_MESSAGE_SEARCH_CURSOR_BYTES,
    MAX_MESSAGE_SEARCH_QUERY_CHARACTERS, MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX,
    MESSAGE_ATTACHMENT_ID_HEX_LENGTH, MESSAGE_ATTACHMENT_ID_PREFIX,
    MESSAGE_ATTACHMENT_REFERENCE_PREFIX, MESSAGE_ATTACHMENT_VIEW_SUFFIX, MESSAGE_CONTEXT_RADIUS,
    MESSAGE_SEARCH_PAGE_SIZE, ROOM_APPEARANCE_ASSET_HEX_LENGTH, ROOM_APPEARANCE_ASSET_PREFIX,
    ROOM_APPEARANCE_REFERENCE_PREFIX, ROOM_APPEARANCE_REFERENCE_SUFFIX,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use ts_rs::TS;

mod local_control;

pub use local_control::{
    LocalBootstrapGrant, LocalBootstrapPhase, LocalControlRequest, LocalControlResponse,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const PRODUCT_SURFACE_REVISION: u32 = 12;
pub const MAX_ROOM_SOCKET_MESSAGE_BYTES: usize = 256 * 1024;
pub const HUMAN_INVITE_SIGNED_TOKEN_PREFIX: &str = "aai1";
pub const HUMAN_INVITE_JOIN_CODE_PREFIX: &str = "aaj1_";
pub const HUMAN_INVITE_SIGNED_TOKEN_MAX_BYTES: usize = 4 * 1024;
pub const HUMAN_INVITE_SIGNATURE_BYTES: usize = 32;
pub const HUMAN_INVITE_JOIN_CODE_BYTES: usize = 24;
pub const HUMAN_INVITE_TIMESTAMP_MIN_YEAR: i32 = -262_143;
pub const HUMAN_INVITE_TIMESTAMP_MAX_YEAR: i32 = 262_142;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Delete,
    Get,
    Post,
}

impl HttpMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "DELETE",
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct HttpRouteSurface {
    pub method: HttpMethod,
    pub path: String,
}

impl HttpRouteSurface {
    #[must_use]
    pub fn new(method: HttpMethod, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RoomStream {
    RoomEvents,
}

impl RoomStream {
    pub const ALL: [Self; 1] = [Self::RoomEvents];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RoomEvents => "room_events",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub enum RoomAction {
    #[serde(rename = "message.delete")]
    MessageDelete,
    #[serde(rename = "message.edit")]
    MessageEdit,
    #[serde(rename = "message.send")]
    MessageSend,
    #[serde(rename = "participant.role.update")]
    ParticipantRoleUpdate,
    #[serde(rename = "participant.mute")]
    ParticipantMute,
    #[serde(rename = "participant.leave")]
    ParticipantLeave,
    #[serde(rename = "room.settings.update")]
    RoomSettingsUpdate,
    #[serde(rename = "room.history")]
    RoomHistory,
    #[serde(rename = "room.vote.summary")]
    RoomVoteSummary,
    #[serde(rename = "room.random.roll")]
    RoomRandomRoll,
    #[serde(rename = "room.random.choose")]
    RoomRandomChoose,
    #[serde(rename = "agent.create")]
    AgentCreate,
    #[serde(rename = "agent.configure")]
    AgentConfigure,
    #[serde(rename = "agent.start")]
    AgentStart,
    #[serde(rename = "agent.pause")]
    AgentPause,
    #[serde(rename = "agent.interrupt")]
    AgentInterrupt,
    #[serde(rename = "agent.resume")]
    AgentResume,
    #[serde(rename = "agent.stop")]
    AgentStop,
}

impl RoomAction {
    pub const ALL: [Self; 18] = [
        Self::AgentConfigure,
        Self::AgentCreate,
        Self::AgentInterrupt,
        Self::AgentPause,
        Self::AgentResume,
        Self::AgentStart,
        Self::AgentStop,
        Self::MessageDelete,
        Self::MessageEdit,
        Self::MessageSend,
        Self::ParticipantLeave,
        Self::ParticipantMute,
        Self::ParticipantRoleUpdate,
        Self::RoomHistory,
        Self::RoomRandomChoose,
        Self::RoomRandomRoll,
        Self::RoomSettingsUpdate,
        Self::RoomVoteSummary,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MessageDelete => "message.delete",
            Self::MessageEdit => "message.edit",
            Self::MessageSend => "message.send",
            Self::ParticipantMute => "participant.mute",
            Self::ParticipantLeave => "participant.leave",
            Self::ParticipantRoleUpdate => "participant.role.update",
            Self::RoomSettingsUpdate => "room.settings.update",
            Self::RoomHistory => "room.history",
            Self::RoomVoteSummary => "room.vote.summary",
            Self::RoomRandomRoll => "room.random.roll",
            Self::RoomRandomChoose => "room.random.choose",
            Self::AgentCreate => "agent.create",
            Self::AgentConfigure => "agent.configure",
            Self::AgentStart => "agent.start",
            Self::AgentInterrupt => "agent.interrupt",
            Self::AgentPause => "agent.pause",
            Self::AgentResume => "agent.resume",
            Self::AgentStop => "agent.stop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ServerProductSurface {
    pub revision: u32,
    pub digest: String,
    pub http_routes: Vec<HttpRouteSurface>,
    pub websocket_streams: Vec<RoomStream>,
    pub websocket_actions: Vec<RoomAction>,
}

impl ServerProductSurface {
    /// Builds the public server surface from its concrete HTTP route registrations.
    ///
    /// # Errors
    ///
    /// Rejects duplicate or malformed route registrations.
    pub fn from_http_routes(
        mut http_routes: Vec<HttpRouteSurface>,
    ) -> Result<Self, ProductSurfaceError> {
        http_routes.sort();
        validate_routes(&http_routes)?;
        let websocket_streams = RoomStream::ALL.to_vec();
        let websocket_actions = RoomAction::ALL.to_vec();
        let digest = server_surface_digest(
            PRODUCT_SURFACE_REVISION,
            &http_routes,
            &websocket_streams,
            &websocket_actions,
        );
        Ok(Self {
            revision: PRODUCT_SURFACE_REVISION,
            digest,
            http_routes,
            websocket_streams,
            websocket_actions,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct HostProductSurface {
    pub revision: u32,
    pub digest: String,
    pub commands: Vec<String>,
}

impl HostProductSurface {
    /// Builds the native host surface from the registered/allowed command intersection.
    ///
    /// # Errors
    ///
    /// Rejects duplicate or malformed command names.
    pub fn from_commands(mut commands: Vec<String>) -> Result<Self, ProductSurfaceError> {
        commands.sort();
        validate_names(&commands, "host command")?;
        let digest = named_surface_digest("agentsassemble.host-product-surface.v1", &commands);
        Ok(Self {
            revision: PRODUCT_SURFACE_REVISION,
            digest,
            commands,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProductSurfaceError {
    #[error("{kind} registration is empty or malformed: {value}")]
    Invalid { kind: &'static str, value: String },
    #[error("duplicate {kind} registration: {value}")]
    Duplicate { kind: &'static str, value: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    Subscribe {
        streams: Vec<RoomStream>,
        #[serde(default)]
        resume_from_seq: i64,
    },
    Command {
        request_id: String,
        action: RoomAction,
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
    Subscribed(Box<Subscribed>),
    Snapshot(Box<RoomSnapshot>),
    Event {
        stream: &'static str,
        events: Vec<RoomEvent>,
        latest_seq: i64,
    },
    ProviderCatalogUpdated {
        catalog: ProviderCatalog,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Subscribed {
    pub streams: Vec<RoomStream>,
    pub protocol_version: u32,
    pub room_id: String,
    pub principal_id: String,
    pub participant_id: String,
    pub server_surface_revision: u32,
    pub server_surface_digest: String,
    pub snapshot_cursor: i64,
    pub catchup_high_water: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RoomSnapshot {
    pub stream: &'static str,
    pub room: Room,
    pub room_settings: PublicRoomSettings,
    pub participants: Vec<Participant>,
    pub agent_sessions: Vec<AgentSession>,
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
    pub available_providers: Vec<ProviderAvailability>,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CommandAck {
    pub request_id: String,
    pub accepted: bool,
    pub resolution: CommandResolution,
    pub action: String,
    pub result: Value,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CommandResolution {
    Committed,
    Rejected,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandNack {
    pub request_id: String,
    pub accepted: bool,
    pub resolution: CommandResolution,
    pub action: String,
    pub error: ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct TicketResponse {
    pub ticket: String,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct OperatorHttpTicketResponse {
    pub ticket: String,
    pub ttl_seconds: u64,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

fn validate_routes(routes: &[HttpRouteSurface]) -> Result<(), ProductSurfaceError> {
    for (index, route) in routes.iter().enumerate() {
        if !valid_path(&route.path) {
            return Err(ProductSurfaceError::Invalid {
                kind: "HTTP route",
                value: format!("{} {}", route.method.as_str(), route.path),
            });
        }
        if index > 0 && routes[index - 1] == *route {
            return Err(ProductSurfaceError::Duplicate {
                kind: "HTTP route",
                value: format!("{} {}", route.method.as_str(), route.path),
            });
        }
    }
    Ok(())
}

fn validate_names(names: &[String], kind: &'static str) -> Result<(), ProductSurfaceError> {
    for (index, name) in names.iter().enumerate() {
        if name.is_empty()
            || name.trim() != name
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
        {
            return Err(ProductSurfaceError::Invalid {
                kind,
                value: name.clone(),
            });
        }
        if index > 0 && names[index - 1] == *name {
            return Err(ProductSurfaceError::Duplicate {
                kind,
                value: name.clone(),
            });
        }
    }
    Ok(())
}

fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains(['?', '#'])
        && !path.chars().any(char::is_whitespace)
        && path.len() <= 256
}

fn server_surface_digest(
    revision: u32,
    routes: &[HttpRouteSurface],
    streams: &[RoomStream],
    actions: &[RoomAction],
) -> String {
    let mut fields = vec![revision.to_string()];
    fields.extend(
        routes
            .iter()
            .map(|route| format!("{} {}", route.method.as_str(), route.path)),
    );
    fields.push("streams".to_owned());
    fields.extend(streams.iter().map(|stream| stream.as_str().to_owned()));
    fields.push("actions".to_owned());
    fields.extend(actions.iter().map(|action| action.as_str().to_owned()));
    named_surface_digest("agentsassemble.server-product-surface.v1", &fields)
}

fn named_surface_digest(context: &str, fields: &[String]) -> String {
    let mut digest = Sha256::new();
    add_digest_field(&mut digest, context.as_bytes());
    for field in fields {
        add_digest_field(&mut digest, field.as_bytes());
    }
    let bytes = digest.finalize();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn add_digest_field(digest: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len())
        .unwrap_or_else(|_| panic!("product-surface digest field exceeds u64"));
    digest.update(length.to_be_bytes());
    digest.update(value);
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde skip predicates receive a reference.
const fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ClientFrame, CommandAck, CommandResolution, HostProductSurface, HttpMethod,
        HttpRouteSurface, LocalControlRequest, ProductSurfaceError, ServerFrame,
        ServerProductSurface,
    };

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

        let mutation: ClientFrame = serde_json::from_value(json!({
            "op": "command",
            "request_id": "web-edit-1",
            "action": "message.edit",
            "payload": {"event_id": "event-1", "content": "updated"}
        }))
        .unwrap_or_else(|error| panic!("valid message mutation envelope: {error}"));
        assert!(matches!(
            mutation,
            ClientFrame::Command {
                action: super::RoomAction::MessageEdit,
                ..
            }
        ));
    }

    #[test]
    fn ack_keeps_existing_outer_shape() {
        let frame = ServerFrame::Ack(CommandAck {
            request_id: "web-1".to_owned(),
            accepted: true,
            resolution: CommandResolution::Committed,
            action: "message.send".to_owned(),
            result: json!({"event_seq": 1}),
            deduplicated: false,
        });
        let value =
            serde_json::to_value(frame).unwrap_or_else(|error| panic!("serializable ACK: {error}"));
        assert_eq!(value["op"], "ack");
        assert_eq!(value["accepted"], true);
        assert_eq!(value["resolution"], "committed");
        assert!(value.get("deduplicated").is_none());
    }

    #[test]
    fn server_surface_is_canonical_and_rejects_duplicate_routes() {
        let surface = ServerProductSurface::from_http_routes(vec![
            HttpRouteSurface::new(HttpMethod::Post, "/api/ws-ticket"),
            HttpRouteSurface::new(HttpMethod::Get, "/healthz"),
            HttpRouteSurface::new(HttpMethod::Delete, "/api/provider-credentials/deepseek"),
        ])
        .unwrap_or_else(|error| panic!("build server surface: {error}"));
        assert_eq!(
            surface
                .http_routes
                .iter()
                .map(|route| format!("{} {}", route.method.as_str(), route.path))
                .collect::<Vec<_>>(),
            [
                "DELETE /api/provider-credentials/deepseek",
                "GET /healthz",
                "POST /api/ws-ticket",
            ]
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::ParticipantRoleUpdate)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::AgentInterrupt)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::AgentPause)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::MessageDelete)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::MessageEdit)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::ParticipantMute)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::ParticipantLeave)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::RoomHistory)
        );
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::RoomVoteSummary)
        );
        assert_eq!(surface.digest.len(), 64);
        assert!(matches!(
            ServerProductSurface::from_http_routes(vec![
                HttpRouteSurface::new(HttpMethod::Get, "/healthz"),
                HttpRouteSurface::new(HttpMethod::Get, "/healthz"),
            ]),
            Err(ProductSurfaceError::Duplicate { .. })
        ));
    }

    #[test]
    fn host_surface_is_sorted_and_closed() {
        let surface = HostProductSurface::from_commands(vec![
            "runtime_ticket".to_owned(),
            "runtime_bootstrap_status".to_owned(),
        ])
        .unwrap_or_else(|error| panic!("build host surface: {error}"));
        assert_eq!(
            surface.commands,
            ["runtime_bootstrap_status", "runtime_ticket"]
        );
        assert!(HostProductSurface::from_commands(vec!["invalid.name".to_owned()]).is_err());
    }

    #[test]
    fn manager_invite_control_request_carries_the_exact_room_authority_tuple() {
        let request = LocalControlRequest::IssueHumanInviteCreateTicket {
            request_id: "request-1".to_owned(),
            server_id: "10000000-0000-4000-8000-000000000001".to_owned(),
            authority_lineage_id: "20000000-0000-4000-8000-000000000002".to_owned(),
            meeting_id: "general".to_owned(),
            room_uid: "30000000-0000-4000-8000-000000000003".to_owned(),
        };
        let encoded =
            serde_json::to_value(request).unwrap_or_else(|error| panic!("encode request: {error}"));
        assert_eq!(
            encoded,
            json!({
                "op": "issue_human_invite_create_ticket",
                "request_id": "request-1",
                "server_id": "10000000-0000-4000-8000-000000000001",
                "authority_lineage_id": "20000000-0000-4000-8000-000000000002",
                "meeting_id": "general",
                "room_uid": "30000000-0000-4000-8000-000000000003"
            })
        );
    }

    #[test]
    fn appearance_read_control_request_carries_exact_authority_and_asset() {
        let request = LocalControlRequest::IssueAppearancePendingReadTicket {
            request_id: "request-2".to_owned(),
            server_id: "10000000-0000-4000-8000-000000000001".to_owned(),
            authority_lineage_id: "20000000-0000-4000-8000-000000000002".to_owned(),
            meeting_id: "general".to_owned(),
            room_uid: "30000000-0000-4000-8000-000000000003".to_owned(),
            asset_id: "ra_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        };
        let encoded =
            serde_json::to_value(request).unwrap_or_else(|error| panic!("encode request: {error}"));
        assert_eq!(encoded["op"], "issue_appearance_pending_read_ticket");
        assert_eq!(encoded["meeting_id"], "general");
        assert_eq!(encoded["asset_id"], "ra_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    }
}
