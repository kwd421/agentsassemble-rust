use agentsassemble_domain::{
    AgentSession, CapabilitySet, Participant, ProviderAvailability, ProviderCatalog,
    PublicRoomSettings, Room, RoomEvent, SnapshotMode, UserProfile,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 1;
pub const PRODUCT_SURFACE_REVISION: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
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
    #[serde(rename = "agent.resume")]
    AgentResume,
    #[serde(rename = "agent.stop")]
    AgentStop,
}

impl RoomAction {
    pub const ALL: [Self; 12] = [
        Self::AgentConfigure,
        Self::AgentCreate,
        Self::AgentResume,
        Self::AgentStart,
        Self::AgentStop,
        Self::MessageSend,
        Self::ParticipantLeave,
        Self::ParticipantMute,
        Self::ParticipantRoleUpdate,
        Self::RoomRandomChoose,
        Self::RoomRandomRoll,
        Self::RoomSettingsUpdate,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MessageSend => "message.send",
            Self::ParticipantMute => "participant.mute",
            Self::ParticipantLeave => "participant.leave",
            Self::ParticipantRoleUpdate => "participant.role.update",
            Self::RoomSettingsUpdate => "room.settings.update",
            Self::RoomRandomRoll => "room.random.roll",
            Self::RoomRandomChoose => "room.random.choose",
            Self::AgentCreate => "agent.create",
            Self::AgentConfigure => "agent.configure",
            Self::AgentStart => "agent.start",
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
        server_challenge: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthenticatedFrame {
    Authenticated {
        counter: u64,
        payload: String,
        proof: String,
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
    pub server_challenge: String,
    pub connection_nonce: String,
    pub room_id: String,
    pub principal_id: String,
    pub participant_id: String,
    pub server_surface_revision: u32,
    pub server_surface_digest: String,
    pub permissions_digest: String,
    pub snapshot_cursor: i64,
    pub catchup_high_water: i64,
    pub snapshot_digest: String,
    pub proof: String,
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
    pub server_proof_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct OperatorHttpTicketResponse {
    pub ticket: String,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalControlRequest {
    InspectBootstrap {
        request_id: String,
    },
    InitializeBootstrap {
        request_id: String,
        display_name: String,
    },
    IssueTicket {
        request_id: String,
        meeting_id: String,
    },
    IssueOperatorHttpTicket {
        request_id: String,
    },
    IssuePreferencesReadTicket {
        request_id: String,
        meeting_id: String,
    },
    IssuePreferencesWriteTicket {
        request_id: String,
        meeting_id: String,
    },
    IssueSettingsDirectoryReadTicket {
        request_id: String,
    },
    IssueCentralRegistrationTicket {
        request_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalBootstrapPhase {
    Empty,
    Initializing,
    Complete,
    RepairRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBootstrapGrant {
    pub phase: LocalBootstrapPhase,
    pub authority_lineage_id: String,
    pub server_id: String,
    pub server_product_surface_revision: u32,
    pub server_product_surface_digest: String,
    pub profile: Option<UserProfile>,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalControlResponse {
    BootstrapOk {
        request_id: String,
        bootstrap: Box<LocalBootstrapGrant>,
    },
    Ok {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
        server_proof_key: String,
    },
    OperatorHttpOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    PreferencesReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    PreferencesWriteOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    SettingsDirectoryReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    CentralRegistrationOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
        server_id: String,
        host_public_key_x: String,
        host_key_fingerprint: String,
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
        HttpRouteSurface, ProductSurfaceError, ServerFrame, ServerProductSurface,
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
        ])
        .unwrap_or_else(|error| panic!("build server surface: {error}"));
        assert_eq!(surface.http_routes[0].path, "/healthz");
        assert!(
            surface
                .websocket_actions
                .contains(&super::RoomAction::ParticipantRoleUpdate)
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
}
