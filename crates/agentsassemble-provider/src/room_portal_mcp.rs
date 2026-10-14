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
    require_current_receipt, reserve_attachment_read, reserve_room_tool, reserve_tabletop_tool,
    valid_decline_reason,
};
use crate::room_portal_tool_contract::{
    CastVote, ChooseRandom, CreateVote, PassTurn, PublishMessage, ReadAttachment,
    ReadMessageContext, ReadRoomStatus, RollDice, SearchMessages, UploadAttachment, VoteTarget,
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
            ProviderRoomToolResult::ConversationStatus(status) => serde_json::to_string(&status)
                .map_err(|_| "The room status could not be encoded.".to_owned()),
            ProviderRoomToolResult::SearchMessages(page) => {
                Ok(crate::room_portal_render::search_page(&page))
            }
            ProviderRoomToolResult::MessageContext(context) => {
                Ok(crate::room_portal_render::message_context(&context))
            }
            ProviderRoomToolResult::UploadAttachment(_)
            | ProviderRoomToolResult::Random(_)
            | ProviderRoomToolResult::DelegatedActions(_)
            | ProviderRoomToolResult::MessagePin(_)
            | ProviderRoomToolResult::DirectAction(_) => {
                Err("The room tool owner returned a mismatched result.".to_owned())
            }
        }
    }

    async fn execute_direct_action(
        &self,
        request: agentsassemble_domain::DirectRoomAction,
    ) -> Result<String, String> {
        let (authority, reservation, ingress) = reserve_room_tool(&self.state)?;
        let result = ingress
            .submit(
                authority,
                ProviderRoomToolRequest::DirectAction(request),
                reservation,
            )
            .await
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        let ProviderRoomToolResult::DirectAction(result) = result else {
            return Err("Unexpected delegated action result.".into());
        };
        serde_json::to_string(&result).map_err(|_| "Action result encoding failed.".into())
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
    #[tool(
        description = "Start an existing agent on behalf of your owner. Conversation-only agents start directly; workspace-write and full-access targets need the exact human owner's confirmation within five minutes and nothing changes before approval. Retry the same request ID to read the outcome. Requires current director role and current owner agent-control authority."
    )]
    async fn start_agent(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::StartAgent>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::StartAgent(request))
            .await
    }

    #[tool(
        description = "Stop an existing agent through its current lifecycle owner. Requires current director role and current owner agent-control authority."
    )]
    async fn stop_agent(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::StopAgent>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::StopAgent(request))
            .await
    }

    #[tool(
        description = "Create a text channel on behalf of your owner. Requires current director role and owner authority."
    )]
    async fn create_text_channel(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::CreateTextChannel>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::CreateTextChannel(
            request,
        ))
        .await
    }

    #[tool(
        description = "Request creation of a stopped agent owned by your owner. Nothing is created until the exact human owner approves within five minutes; the approval re-validates the catalog selection and a full-access choice needs the owner's explicit acknowledgement in the decision. Starting the new agent later is a separate step. Retry the same request ID to read the outcome. Requires current director role and current owner agent-control authority."
    )]
    async fn request_create_agent(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::RequestCreateAgent>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::RequestCreateAgent(
            request,
        ))
        .await
    }

    #[tool(
        description = "Request reconfiguration of an existing stopped agent owned by your owner (model, reasoning, permission level, working folder). Nothing changes until the exact human owner approves within five minutes; the approval re-validates the target and a resulting full-access configuration needs the owner's explicit acknowledgement in the decision. Retry the same request ID to read the outcome. Requires current director role and current owner agent-control authority."
    )]
    async fn request_reconfigure_agent(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::RequestReconfigureAgent>,
    ) -> Result<String, String> {
        self.execute_direct_action(
            agentsassemble_domain::DirectRoomAction::RequestReconfigureAgent(request),
        )
        .await
    }

    #[tool(
        description = "Request removal of an existing agent from the room on behalf of your owner. Nothing changes until the exact human owner approves within five minutes; approval ends that agent's session, revokes its access and stops a running runtime. Retry the same request ID to read the outcome. Requires current director role and current owner room-manage authority."
    )]
    async fn request_remove_agent(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::RequestRemoveAgent>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::RequestRemoveAgent(
            request,
        ))
        .await
    }

    #[tool(
        description = "Request removal of a human participant from the room on behalf of your owner. Nothing changes until the exact human owner approves within five minutes; approval ends that person's access and closes their live room session. The owner themself, the host, an AI and anyone your owner could not remove are refused. Retry the same request ID to read the outcome. Requires current director role and current owner room-manage authority."
    )]
    async fn request_kick_participant(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::RequestKickParticipant>,
    ) -> Result<String, String> {
        self.execute_direct_action(
            agentsassemble_domain::DirectRoomAction::RequestKickParticipant(request),
        )
        .await
    }

    #[tool(
        description = "Request deletion of a text channel and its message text, pins and search records. Nothing changes until the exact human owner approves within five minutes. Retry the same request ID to read its outcome."
    )]
    async fn request_delete_text_channel(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::DeleteTextChannel>,
    ) -> Result<String, String> {
        self.execute_direct_action(
            agentsassemble_domain::DirectRoomAction::RequestDeleteTextChannel(request),
        )
        .await
    }

    #[tool(
        description = "Rename a text channel at the expected settings revision on behalf of your owner. Requires current director role and owner authority."
    )]
    async fn rename_text_channel(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::RenameTextChannel>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::RenameTextChannel(
            request,
        ))
        .await
    }

    #[tool(
        description = "Mute or unmute a participant on behalf of your owner. Requires current director role and owner authority."
    )]
    async fn set_participant_mute(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::SetParticipantMute>,
    ) -> Result<String, String> {
        self.execute_direct_action(agentsassemble_domain::DirectRoomAction::SetParticipantMute(
            request,
        ))
        .await
    }

    #[tool(
        description = "Pin or unpin a room message on behalf of your owner. Only a current director AI whose owner can currently pin may use this tool; no operator rights are granted."
    )]
    async fn set_message_pin(
        &self,
        Parameters(request): Parameters<agentsassemble_domain::SetMessagePin>,
    ) -> Result<String, String> {
        let (authority, reservation, ingress) = reserve_room_tool(&self.state)?;
        let result = ingress
            .submit(
                authority,
                ProviderRoomToolRequest::SetMessagePin(request),
                reservation,
            )
            .await
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        let ProviderRoomToolResult::MessagePin(result) = result else {
            return Err("Unexpected pin result.".into());
        };
        serde_json::to_string(&result).map_err(|_| "Pin result encoding failed.".into())
    }

    #[tool(
        description = "Read current agent activity and open polls with your own choices. Optional observation; does not end your turn. Follow a nonzero next_before_seq even when the poll page is empty."
    )]
    async fn read_room_status(
        &self,
        Parameters(input): Parameters<ReadRoomStatus>,
    ) -> Result<String, String> {
        self.execute_room_read(ProviderRoomToolRequest::ReadRoomStatus {
            before_seq: input.before_seq,
        })
        .await
    }

    #[tool(
        description = "Read the recent messages in the room. Other room tools work after this. Does not end your turn."
    )]
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

    #[tool(
        description = "Read a room attachment by ID. Up to 10 MiB is inline; larger published files (up to 50 MiB) are saved atomically in your writable working folder and return a path. Read-only agents receive attachment_too_large. Per turn: 10 distinct IDs, 2 attempts each, 200 MiB total successful reads. Does not end your turn."
    )]
    async fn read_attachment(
        &self,
        Parameters(input): Parameters<ReadAttachment>,
    ) -> Result<rmcp::model::CallToolResult, String> {
        let (authority, ingress, mut reservation) =
            reserve_attachment_read(&self.state, &input.attachment_id)?;
        let attachment = ingress
            .read(authority, input.attachment_id.clone())
            .await
            .map_err(|error| {
                if matches!(
                    error.code.as_ref(),
                    "attachment_too_large" | "attachment_download_budget_exhausted"
                ) {
                    format!("{}: {}", error.code, error.message)
                } else {
                    error.message
                }
            })?;
        let result = attachment_tool_result(&attachment)?;
        if let Err(error) = reservation.complete(attachment.size) {
            // Over the turn's byte budget: the agent gets no path, so the copy must not stay.
            if let Some(path) = &attachment.path {
                crate::room_file_workspace::discard_saved_attachment(path);
            }
            return Err(error);
        }
        Ok(result)
    }

    #[tool(
        description = "Upload a regular file from a path inside your working folder as a pending room attachment. Requires workspace access. Uses the current room per-file limit (10/25/50 MiB), storage quota and three-active-transfers-per-principal admission. Pass the returned ID in publish_message attachment_ids within one hour. Does not end your turn."
    )]
    async fn upload_attachment(
        &self,
        Parameters(input): Parameters<UploadAttachment>,
    ) -> Result<String, String> {
        let (authority, reservation, ingress) = reserve_room_tool(&self.state)?;
        let result = ingress
            .submit(
                authority,
                ProviderRoomToolRequest::UploadAttachment {
                    path: input.path,
                    content_type: input.content_type,
                },
                reservation,
            )
            .await
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        let ProviderRoomToolResult::UploadAttachment(result) = result else {
            return Err("Unexpected upload result.".into());
        };
        Ok(result.to_string())
    }

    #[tool(description = "Search the room's full message history. Does not end your turn.")]
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

    #[tool(description = "Read the messages around one search result. Does not end your turn.")]
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
        description = "Post a message and optional attachment_ids returned by upload_attachment to the room, optionally handing the floor to one agent by its ID. Ends your turn."
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
        let message = agentsassemble_domain::MessageSend::from_payload(
            &json!({"content":input.content,"attachment_ids":input.attachment_ids}),
        )
        .map_err(|error| error.message)?;
        let content = message.content;
        if input.reply_to_event_id.as_ref().is_some_and(|id| {
            uuid::Uuid::parse_str(id).map_or(true, |parsed| parsed.to_string() != *id)
        }) {
            return Err("Reply target must be an event UUID from the room.".to_owned());
        }
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
            attachment_ids: message.attachment_ids,
            target_agent_id,
            reply_to_event_id: input.reply_to_event_id,
        });
        Ok("Published to the shared room.".to_owned())
    }

    #[tool(
        description = "Pass this turn without posting. Takes one reason code: nothing_useful_to_add, not_addressed, or duplicate. Ends your turn."
    )]
    fn pass_turn(&self, Parameters(input): Parameters<PassTurn>) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
        let active = terminal_observation(&mut state)?;
        if !valid_decline_reason(&input.reason_code) {
            return Err("The pass reason is unsupported.".to_owned());
        }
        active.outcome = Some(StagedOutcome::Declined {
            receipt_generation: active.turn_generation,
            reason_code: input.reason_code,
        });
        Ok("Passed this shared-room turn.".to_owned())
    }

    #[tool(description = "Create a single-choice room poll. Ends your turn.")]
    fn create_vote(&self, Parameters(input): Parameters<CreateVote>) -> Result<String, String> {
        self.stage_vote(&json!({
            "kind": "vote",
            "vote_question": input.question,
            "vote_options": input.options,
            "vote_duration_seconds": input.duration_seconds,
        }))
    }

    #[tool(description = "Cast or replace your ballot in a room poll. Ends your turn.")]
    fn cast_vote(&self, Parameters(input): Parameters<CastVote>) -> Result<String, String> {
        self.stage_vote(&json!({
            "kind": "vote_cast",
            "vote_id": input.vote_id,
            "vote_choice": input.choice,
        }))
    }

    #[tool(description = "Withdraw your ballot from a room poll. Ends your turn.")]
    fn withdraw_vote(&self, Parameters(input): Parameters<VoteTarget>) -> Result<String, String> {
        self.stage_vote(&json!({"kind": "vote_withdraw", "vote_id": input.vote_id}))
    }

    #[tool(description = "Close a poll you created. Ends your turn.")]
    fn close_vote(&self, Parameters(input): Parameters<VoteTarget>) -> Result<String, String> {
        self.stage_vote(&json!({"kind": "vote_close", "vote_id": input.vote_id}))
    }

    #[tool(
        description = "Roll dice with server-side randomness (tabletop mode). Results are shown in the room. Does not end your turn."
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
        description = "Pick one of the given options with server-side randomness (tabletop mode). Results are shown in the room. Does not end your turn."
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
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        let available = match crate::room_portal::reserve_tool(&self.state, false, false) {
            Ok((authority, reservation, ingress)) => {
                match ingress
                    .submit(
                        authority,
                        ProviderRoomToolRequest::DelegatedActions,
                        reservation,
                    )
                    .await
                {
                    Ok(ProviderRoomToolResult::DelegatedActions(value)) => value,
                    Ok(_) => {
                        return Err(rmcp::ErrorData::internal_error(
                            "Invalid delegation discovery result",
                            None,
                        ));
                    }
                    Err(error) => return Err(rmcp::ErrorData::internal_error(error.message, None)),
                }
            }
            Err(_) => Vec::new(),
        };
        let tools = self
            .tool_router
            .list_all()
            .into_iter()
            .filter(|tool| {
                let name = tool.name.as_ref();
                !(name == "set_message_pin"
                    || agentsassemble_domain::DirectRoomAction::NAMES.contains(&name))
                    || available.iter().any(|action| action == name)
            })
            .collect();
        Ok(rmcp::model::ListToolsResult::with_all_items(tools)
            .with_ttl_ms(0)
            .with_cache_scope(rmcp::model::CacheScope::Private))
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read the bounded shared-room view, then publish, vote, or decline exactly once.",
        )
    }
}

#[cfg(test)]
#[path = "room_portal_mcp_tests.rs"]
mod tests;
