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
    pub(super) session_instructions: Option<String>,
    pub(super) requests: bool,
    pub(super) observation: Option<Observation>,
    #[serde(default)]
    pub(super) context_tokens: Option<u64>,
    #[serde(default)]
    pub(super) owner_started: bool,
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
            context_tokens: self.context_tokens,
            owner_started: self.owner_started,
            request_ingress: self.requests.then(|| relay.requests.clone()),
            turn_id: self.turn_id,
            turn_generation: self.turn_generation,
            execution_id: self.execution_id,
            input: self.input,
            session_instructions: self.session_instructions,
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

impl From<&ProviderTurnRequest> for TurnInput {
    fn from(request: &ProviderTurnRequest) -> Self {
        Self {
            turn_id: request.turn_id.clone(),
            turn_generation: request.turn_generation,
            execution_id: request.execution_id.clone(),
            input: request.input.clone(),
            session_instructions: request.session_instructions.clone(),
            requests: request.request_ingress.is_some(),
            context_tokens: request.context_tokens,
            owner_started: request.owner_started,
            observation: request
                .room_observation
                .as_ref()
                .map(|observation| Observation {
                    session_id: observation.session_id.clone(),
                    input_up_to_seq: observation.input_up_to_seq,
                    view: observation.view.clone(),
                    attachment_ids: observation.attachment_ids.clone(),
                    attachments: observation.attachment_ingress.is_some(),
                    allowed_agent_ids: observation.allowed_agent_ids.clone(),
                    tabletop_tools: observation.tabletop_tools,
                    tools: observation.room_tool_ingress.is_some(),
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TurnInput;
    use crate::driver::ProviderTurnRequest;

    /// The window and the owner-started flag survive the pipe to the child.
    #[test]
    fn the_child_receives_the_window_and_owner_started() {
        let request = ProviderTurnRequest {
            context_tokens: Some(131_072),
            owner_started: true,
            request_ingress: None,
            turn_id: "turn".to_owned(),
            turn_generation: 1,
            execution_id: "execution".to_owned(),
            input: "input".to_owned(),
            session_instructions: None,
            room_observation: None,
        };
        let encoded = serde_json::to_string(&TurnInput::from(&request))
            .unwrap_or_else(|error| panic!("{error}"));
        let decoded: TurnInput =
            serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decoded.context_tokens, Some(131_072));
        assert!(decoded.owner_started);
    }
}
