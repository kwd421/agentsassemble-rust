//! Turn data crosses the private pipe; live ingress handles remain on their owning side.
use serde::{Deserialize, Serialize};

use crate::driver::{ProviderRoomObservation, ProviderTurnRequest};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TurnInput {
    pub(super) turn_id: String,
    pub(super) turn_generation: u64,
    pub(super) execution_id: String,
    pub(super) input: String,
    pub(super) requests: bool,
    pub(super) observation: Option<Observation>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub(super) session_id: String,
    pub(super) input_up_to_seq: i64,
    pub(super) view: String,
    pub(super) attachment_ids: Vec<String>,
    pub(super) attachments: bool,
    pub(super) allowed_agent_ids: Vec<String>,
    pub(super) tabletop_tools: bool,
    pub(super) tools: bool,
}

impl TurnInput {
    pub(super) fn into_request(self, relay: &super::callbacks::Callbacks) -> ProviderTurnRequest {
        ProviderTurnRequest {
            request_ingress: self.requests.then(|| relay.requests.clone()),
            turn_id: self.turn_id,
            turn_generation: self.turn_generation,
            execution_id: self.execution_id,
            input: self.input,
            room_observation: self.observation.map(|observation| ProviderRoomObservation {
                session_id: observation.session_id,
                input_up_to_seq: observation.input_up_to_seq,
                view: observation.view,
                attachment_ids: observation.attachment_ids,
                attachment_ingress: observation.attachments.then(|| relay.attachments.clone()),
                allowed_agent_ids: observation.allowed_agent_ids,
                tabletop_tools: observation.tabletop_tools,
                room_tool_ingress: observation.tools.then(|| relay.tools.clone()),
            }),
        }
    }
}
