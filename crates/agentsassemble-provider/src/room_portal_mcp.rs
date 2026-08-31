use std::sync::{Arc, Mutex};

use agentsassemble_domain::{RoomRandomRequest, VoteCommand};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde_json::json;

use crate::room_attachment::attachment_tool_result;
use crate::room_portal::{
    ActiveObservation, PortalState, ProviderRoomToolRequest, ProviderRoomToolResult, StagedOutcome,
    canonical_message, require_current_receipt, reserve_attachment_read, reserve_room_tool,
    reserve_tabletop_tool, valid_decline_reason,
};
use crate::room_portal_tool_contract::{
    CastVote, ChooseRandom, CreateVote, DeclineToSpeak, PublishMessage, ReadAttachment,
    ReadMessageContext, RollDice, SearchMessages, VoteTarget,
};

#[derive(Debug, Clone)]
pub(super) struct RoomPortalMcp {
    state: Arc<Mutex<PortalState>>,
    tool_router: ToolRouter<Self>,
}

impl RoomPortalMcp {
    pub(super) fn new(state: Arc<Mutex<PortalState>>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

impl RoomPortalMcp {
    async fn execute_room_random(&self, request: RoomRandomRequest) -> Result<String, String> {
        let (authority, reservation, ingress) = reserve_tabletop_tool(&self.state)?;
        let result = ingress
            .submit(
                authority,
                ProviderRoomToolRequest::Random(request),
                reservation,
            )
            .await
            .map_err(|error| error.message)?;
        let ProviderRoomToolResult::Random(result) = result else {
            return Err("The room tool owner returned a mismatched result.".to_owned());
        };
        serde_json::to_string(&result)
            .map_err(|_| "The room tool result could not be encoded.".to_owned())
    }

    async fn execute_room_read(&self, request: ProviderRoomToolRequest) -> Result<String, String> {
        let (authority, reservation, ingress) = reserve_room_tool(&self.state)?;
        let result = ingress
            .submit(authority, request, reservation)
            .await
            .map_err(|error| error.message)?;
        match result {
            ProviderRoomToolResult::SearchMessages(page) => serde_json::to_string(&page),
            ProviderRoomToolResult::MessageContext(context) => serde_json::to_string(&context),
            ProviderRoomToolResult::Random(_) => {
                return Err("The room tool owner returned a mismatched result.".to_owned());
            }
        }
        .map_err(|_| "The room tool result could not be encoded.".to_owned())
    }

    fn stage_vote(&self, payload: &serde_json::Value) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = terminal_observation(&mut state)?;
        let command = VoteCommand::from_payload(payload).map_err(|error| error.message)?;
        active.outcome = Some(StagedOutcome::Vote {
            receipt_generation: active.turn_generation,
            command,
        });
        Ok("Staged a vote action for the shared room.".to_owned())
    }
}

fn terminal_observation(state: &mut PortalState) -> Result<&mut ActiveObservation, String> {
    let active = state
        .active
        .as_mut()
        .ok_or_else(|| "No active room observation.".to_owned())?;
    require_current_receipt(active)?;
    if active.closing || active.outcome.is_some() {
        return Err("This turn already has a terminal room action.".to_owned());
    }
    if active.has_pending_operations() {
        return Err("Wait for pending room tools before completing this turn.".to_owned());
    }
    Ok(active)
}

#[tool_router]
impl RoomPortalMcp {
    #[tool(description = "Read the finalized messages in this turn's bounded shared-room view.")]
    fn read_discussion(&self) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = state
            .active
            .as_mut()
            .ok_or_else(|| "No active room observation.".to_owned())?;
        active.receipt_generation = Some(active.turn_generation);
        Ok(active.room_view.clone())
    }

    #[tool(description = "Read one attachment listed in this exact room turn.")]
    async fn read_attachment(
        &self,
        Parameters(input): Parameters<ReadAttachment>,
    ) -> Result<rmcp::model::CallToolResult, String> {
        let (authority, ingress, mut reservation) =
            reserve_attachment_read(&self.state, &input.attachment_id)?;
        let attachment = ingress
            .read(authority, input.attachment_id.clone())
            .await
            .map_err(|error| error.message)?;
        let result = attachment_tool_result(&attachment)?;
        reservation.complete(attachment.size)?;
        Ok(result)
    }

    #[tool(
        description = "Search complete canonical lobby-message history for this exact room turn. Read the discussion first."
    )]
    async fn search_messages(
        &self,
        Parameters(input): Parameters<SearchMessages>,
    ) -> Result<String, String> {
        self.execute_room_read(ProviderRoomToolRequest::SearchMessages {
            query: input.query,
            cursor: input.cursor,
        })
        .await
    }

    #[tool(
        description = "Read the bounded chronological lobby context around one search result event. Read the discussion first."
    )]
    async fn read_message_context(
        &self,
        Parameters(input): Parameters<ReadMessageContext>,
    ) -> Result<String, String> {
        self.execute_room_read(ProviderRoomToolRequest::ReadMessageContext {
            event_id: input.event_id,
        })
        .await
    }

    #[tool(
        description = "Publish one substantive message to the shared room, optionally handing the floor to one exact agent ID. Read the discussion first."
    )]
    fn publish_message(
        &self,
        Parameters(input): Parameters<PublishMessage>,
    ) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = terminal_observation(&mut state)?;
        let content = canonical_message(&input.content)
            .ok_or_else(|| "The room publication is invalid.".to_owned())?;
        let target_agent_id = if active
            .authority
            .allowed_agent_ids
            .contains(&input.next_agent_id)
        {
            input.next_agent_id
        } else {
            String::new()
        };
        active.outcome = Some(StagedOutcome::Message {
            receipt_generation: active.turn_generation,
            content,
            target_agent_id,
        });
        Ok("Published to the shared room.".to_owned())
    }

    #[tool(
        description = "End this room turn without posting, using one supported reason code: nothing_useful_to_add, not_addressed, or duplicate. Read the discussion first."
    )]
    fn decline_to_speak(
        &self,
        Parameters(input): Parameters<DeclineToSpeak>,
    ) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = terminal_observation(&mut state)?;
        if !valid_decline_reason(&input.reason_code) {
            return Err("The decline reason is unsupported.".to_owned());
        }
        active.outcome = Some(StagedOutcome::Declined {
            receipt_generation: active.turn_generation,
            reason_code: input.reason_code,
        });
        Ok("Declined this shared-room turn.".to_owned())
    }

    #[tool(
        description = "Create one bounded single-choice room poll and end this turn. Read the discussion first."
    )]
    fn create_vote(&self, Parameters(input): Parameters<CreateVote>) -> Result<String, String> {
        self.stage_vote(&json!({
            "kind": "vote",
            "vote_question": input.question,
            "vote_options": input.options,
            "vote_duration_seconds": input.duration_seconds,
        }))
    }

    #[tool(
        description = "Cast or replace this Agent Session's ballot and end this turn. Read the discussion first."
    )]
    fn cast_vote(&self, Parameters(input): Parameters<CastVote>) -> Result<String, String> {
        self.stage_vote(&json!({
            "kind": "vote_cast",
            "vote_id": input.vote_id,
            "vote_choice": input.choice,
        }))
    }

    #[tool(
        description = "Withdraw this Agent Session's ballot and end this turn. Read the discussion first."
    )]
    fn withdraw_vote(&self, Parameters(input): Parameters<VoteTarget>) -> Result<String, String> {
        self.stage_vote(&json!({"kind": "vote_withdraw", "vote_id": input.vote_id}))
    }

    #[tool(
        description = "Close a poll created by this Agent Session and end this turn. Read the discussion first."
    )]
    fn close_vote(&self, Parameters(input): Parameters<VoteTarget>) -> Result<String, String> {
        self.stage_vote(&json!({"kind": "vote_close", "vote_id": input.vote_id}))
    }

    #[tool(
        description = "Roll bounded server-owned dice in tabletop mode. Read the discussion first."
    )]
    async fn roll_dice(&self, Parameters(input): Parameters<RollDice>) -> Result<String, String> {
        let request = RoomRandomRequest::parse(
            "room.random.roll",
            &json!({"notation": input.notation, "reason": input.reason}),
        )
        .map_err(|error| error.message)?;
        self.execute_room_random(request).await
    }

    #[tool(
        description = "Choose one bounded option with server-owned randomness in tabletop mode. Read the discussion first."
    )]
    async fn choose_random(
        &self,
        Parameters(input): Parameters<ChooseRandom>,
    ) -> Result<String, String> {
        let request = RoomRandomRequest::parse(
            "room.random.choose",
            &json!({"options": input.options, "reason": input.reason}),
        )
        .map_err(|error| error.message)?;
        self.execute_room_random(request).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RoomPortalMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read the bounded shared-room view, then publish, vote, or decline exactly once.",
        )
    }
}

#[cfg(test)]
#[path = "room_portal_mcp_tests.rs"]
mod tests;
