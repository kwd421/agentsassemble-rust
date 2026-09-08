//! One wire contract for the attendee server and its native client.
use agentsassemble_persistence::{
    AttendeeCleanupDelivery, AttendeeInterruptDelivery, AttendeeRuntimeReady, AttendeeTurnDelivery,
    AttendeeTurnReport, ProviderTurnStartAuthority,
};
use agentsassemble_protocol::CommandResolution;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const SOCKET_IDLE: std::time::Duration = std::time::Duration::from_mins(5);

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttendeeSocketRequest {
    ProviderRequestOpen {
        request_id: Uuid,
        request: Box<agentsassemble_persistence::OpenProviderRequest>,
    },
    ProviderRequestDelivered {
        request_id: Uuid,
        provider_request_id: Uuid,
        delivered: bool,
    },
    Ready {
        request_id: Uuid,
        report: Box<AttendeeRuntimeReady>,
    },
    Started {
        request_id: Uuid,
        authority: Box<ProviderTurnStartAuthority>,
        provider_turn_id: String,
    },
    Report {
        report: Box<AttendeeTurnReport>,
    },
}

impl AttendeeSocketRequest {
    #[must_use]
    pub fn request_id(&self) -> Uuid {
        match self {
            Self::Ready { request_id, .. }
            | Self::Started { request_id, .. }
            | Self::ProviderRequestOpen { request_id, .. }
            | Self::ProviderRequestDelivered { request_id, .. } => *request_id,
            Self::Report { report } => report.request_id,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttendeeSocketFrame {
    ProviderResponse {
        provider_request_id: Uuid,
        resolution: agentsassemble_domain::ProviderRequestResolution,
    },
    ProviderRequestClosed {
        provider_request_id: Uuid,
    },
    Connected {
        connection_id: Uuid,
    },
    Ack {
        request_id: Uuid,
        resolution: CommandResolution,
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sequence: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        deduplicated: Option<bool>,
    },
    Nack {
        request_id: Uuid,
        resolution: CommandResolution,
        error: AttendeeSocketFailure,
    },
    Turn {
        assignment: Box<AttendeeTurnDelivery>,
    },
    Stop {
        stop: AttendeeCleanupDelivery,
    },
    Interrupt {
        interrupt: Box<AttendeeInterruptDelivery>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeSocketFailure {
    pub code: String,
}
