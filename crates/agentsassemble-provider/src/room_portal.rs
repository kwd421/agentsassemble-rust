use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use thiserror::Error;
use uuid::Uuid;

use agentsassemble_domain::VoteCommand;

#[cfg(windows)]
use crate::filesystem::BoundExecutable;
#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::room_attachment::{ProviderAttachmentReadIngress, valid_observation_attachments};
use crate::room_portal_mcp_transport::PortalServer;
#[cfg(any(unix, windows))]
use crate::room_portal_terminal::RoomPortalTerminalHelper;

#[path = "room_portal_attachment_budget.rs"]
mod attachment_budget;
pub(crate) use attachment_budget::{AttachmentReadBudget, reserve_attachment_read};
#[path = "room_portal_tool.rs"]
mod tool;
pub use tool::{
    ProviderRoomToolCommand, ProviderRoomToolError, ProviderRoomToolIngress,
    ProviderRoomToolRequest, ProviderRoomToolResult,
};

pub(super) const ROOM_PORTAL_TOKEN_ENV_PREFIX: &str = "AGENTSASSEMBLE_INTERNAL_ROOM_PORTAL_TOKEN_";

const MAX_ROOM_VIEW_BYTES: usize = 96 * 1024;
const MAX_TURN_ID_BYTES: usize = 128;
const MAX_AGENT_IDS: usize = 64;
pub(super) const MAX_MESSAGE_CHARS: usize = 12_000;
const MAX_ROOM_TOOL_RESULTS: usize = 32;
pub(crate) const CREATE_VOTE_TOOL: &str = "create_vote";
pub(crate) const CAST_VOTE_TOOL: &str = "cast_vote";
pub(crate) const WITHDRAW_VOTE_TOOL: &str = "withdraw_vote";
pub(crate) const CLOSE_VOTE_TOOL: &str = "close_vote";
pub(crate) const VOTE_TOOL_NAMES: [&str; 4] = [
    CREATE_VOTE_TOOL,
    CAST_VOTE_TOOL,
    WITHDRAW_VOTE_TOOL,
    CLOSE_VOTE_TOOL,
];

pub(crate) fn is_vote_tool(name: &str) -> bool {
    VOTE_TOOL_NAMES.contains(&name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderTurnOutcome {
    Message {
        content: String,
        target_agent_id: String,
    },
    Declined {
        reason_code: String,
    },
    Vote {
        command: VoteCommand,
    },
}

#[derive(Debug, Clone, Copy, Error)]
pub enum RoomPortalError {
    #[error("the room portal authority is unavailable")]
    Authority,
    #[error("the room portal observation is missing or inconsistent")]
    Observation,
    #[error("the provider did not read the assigned room observation")]
    ReceiptMissing,
    #[error("the provider did not stage exactly one terminal room action")]
    OutcomeMissing,
    #[error("the provider staged an invalid terminal room action")]
    OutcomeInvalid,
    #[error("the room portal MCP server failed")]
    Mcp,
}

pub(crate) struct RoomObservationStart<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub input_up_to_seq: i64,
    pub durable_turn_generation: u64,
    pub execution_id: &'a str,
    pub room_view: &'a str,
    pub attachment_ids: &'a [String],
    pub attachment_ingress: Option<ProviderAttachmentReadIngress>,
    pub allowed_agent_ids: &'a [String],
    pub tabletop_tools: bool,
    pub tool_ingress: Option<ProviderRoomToolIngress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TurnAuthority {
    pub(super) session_id: String,
    pub(super) turn_id: String,
    pub(super) input_up_to_seq: i64,
    pub(super) durable_turn_generation: u64,
    pub(super) execution_id: String,
    pub(super) allowed_agent_ids: Vec<String>,
}

#[derive(Debug)]
pub(super) enum StagedOutcome {
    Message {
        receipt_generation: Uuid,
        content: String,
        target_agent_id: String,
    },
    Declined {
        receipt_generation: Uuid,
        reason_code: String,
    },
    Vote {
        receipt_generation: Uuid,
        command: VoteCommand,
    },
}

#[derive(Debug)]
pub(super) struct ActiveObservation {
    pub(super) authority: TurnAuthority,
    pub(super) room_view: String,
    pub(super) attachment_ids: HashSet<String>,
    pub(super) attachment_ingress: Option<ProviderAttachmentReadIngress>,
    pub(super) attachment_reads: AttachmentReadBudget,
    pub(super) turn_generation: Uuid,
    pub(super) receipt_generation: Option<Uuid>,
    pub(super) outcome: Option<StagedOutcome>,
    pub(super) tabletop_tools: bool,
    pub(super) tool_ingress: Option<ProviderRoomToolIngress>,
    pub(super) tool_reservations: BTreeMap<Uuid, ToolReservationStatus>,
    pub(super) successful_tool_results: usize,
    pub(super) closing: bool,
}

impl ActiveObservation {
    pub(super) fn has_pending_operations(&self) -> bool {
        !self.tool_reservations.is_empty() || self.attachment_reads.has_pending()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolReservationStatus {
    Queued,
    Executing,
}

#[derive(Debug)]
pub(super) struct RoomToolReservation {
    state: Arc<Mutex<PortalState>>,
    turn_generation: Uuid,
    reservation_id: Uuid,
    resolved: bool,
}

impl RoomToolReservation {
    fn begin_execution(&mut self) -> Result<(), ProviderRoomToolError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| tool_error("room_unavailable", "Room tool authority is unavailable."))?;
        let active = state
            .active
            .as_mut()
            .filter(|active| active.turn_generation == self.turn_generation && !active.closing)
            .ok_or_else(|| tool_error("stale_provider_turn", "The room turn has ended."))?;
        let status = active
            .tool_reservations
            .get_mut(&self.reservation_id)
            .ok_or_else(|| tool_error("stale_provider_turn", "The room tool reservation ended."))?;
        if *status != ToolReservationStatus::Queued {
            return Err(tool_error(
                "room_tool_conflict",
                "The room tool reservation was already consumed.",
            ));
        }
        *status = ToolReservationStatus::Executing;
        Ok(())
    }

    fn resolve(&mut self, successful: bool) {
        if self.resolved {
            return;
        }
        self.resolved = true;
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let remove_tombstone = if let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.turn_generation == self.turn_generation)
        {
            let removed = active.tool_reservations.remove(&self.reservation_id);
            if successful && removed == Some(ToolReservationStatus::Executing) {
                active.successful_tool_results = active.successful_tool_results.saturating_add(1);
            }
            active.closing && !active.has_pending_operations()
        } else {
            false
        };
        if remove_tombstone {
            state.active = None;
        }
    }
}

impl Drop for RoomToolReservation {
    fn drop(&mut self) {
        self.resolve(false);
    }
}

#[derive(Debug, Default)]
pub(super) struct PortalState {
    pub(super) active: Option<ActiveObservation>,
}

pub(crate) struct RoomPortal {
    state: Arc<Mutex<PortalState>>,
    server: PortalServer,
    bearer_environment_name: String,
}

impl RoomPortal {
    pub(crate) async fn create() -> Result<Self, RoomPortalError> {
        let state = Arc::new(Mutex::new(PortalState::default()));
        let server = PortalServer::start(state.clone()).await?;
        let bearer_environment_name = format!(
            "{ROOM_PORTAL_TOKEN_ENV_PREFIX}{}",
            Uuid::new_v4().simple().to_string().to_ascii_uppercase()
        );
        Ok(Self {
            state,
            server,
            bearer_environment_name,
        })
    }

    pub(crate) fn append_codex_config(
        &self,
        arguments: &mut Vec<String>,
    ) -> Result<(), RoomPortalError> {
        self.require_server()?;
        let server = "mcp_servers.agentsassemble_room";
        push_codex_config(
            arguments,
            &format!("{server}.url"),
            &serde_json::to_string(self.server.endpoint())
                .map_err(|_| RoomPortalError::Authority)?,
        );
        push_codex_config(
            arguments,
            &format!("{server}.bearer_token_env_var"),
            &serde_json::to_string(&self.bearer_environment_name)
                .map_err(|_| RoomPortalError::Authority)?,
        );
        push_codex_config(
            arguments,
            &format!("{server}.default_tools_approval_mode"),
            &serde_json::to_string("approve").map_err(|_| RoomPortalError::Authority)?,
        );
        push_codex_config(
            arguments,
            "shell_environment_policy.ignore_default_excludes",
            "false",
        );
        push_codex_config(arguments, "features.plugins", "false");
        push_codex_config(arguments, "features.apps", "false");
        push_codex_config(arguments, "features.shell_snapshot", "false");
        push_codex_config(arguments, &format!("{server}.startup_timeout_sec"), "10");
        push_codex_config(arguments, &format!("{server}.tool_timeout_sec"), "30");
        Ok(())
    }

    pub(crate) fn provider_environment(&self) -> Vec<(String, String)> {
        vec![(
            self.bearer_environment_name.clone(),
            self.server.bearer_token().to_owned(),
        )]
    }

    #[cfg(unix)]
    pub(crate) fn create_terminal_helper(
        &self,
        guardian: &GuardianLaunch,
    ) -> Result<RoomPortalTerminalHelper, RoomPortalError> {
        self.require_server()?;
        RoomPortalTerminalHelper::create(
            guardian,
            self.server.endpoint(),
            self.server.bearer_token(),
        )
    }

    #[cfg(windows)]
    pub(crate) fn create_terminal_helper(
        &self,
        companion: &BoundExecutable,
    ) -> Result<RoomPortalTerminalHelper, RoomPortalError> {
        self.require_server()?;
        RoomPortalTerminalHelper::create(
            companion,
            self.server.endpoint(),
            self.server.bearer_token(),
        )
    }

    #[cfg(not(unix))]
    pub(crate) fn configure_environment(&self, command: &mut tokio::process::Command) {
        command.envs(self.provider_environment());
    }

    pub(crate) fn begin_observation(
        &self,
        observation: RoomObservationStart<'_>,
    ) -> Result<(), RoomPortalError> {
        let RoomObservationStart {
            session_id,
            turn_id,
            input_up_to_seq,
            durable_turn_generation,
            execution_id,
            room_view,
            attachment_ids,
            attachment_ingress,
            allowed_agent_ids,
            tabletop_tools,
            tool_ingress,
        } = observation;
        self.require_server()?;
        validate_turn_id(session_id)?;
        validate_turn_id(turn_id)?;
        validate_turn_id(execution_id)?;
        let unique_agent_ids = allowed_agent_ids.iter().collect::<HashSet<_>>();
        if input_up_to_seq <= 0
            || durable_turn_generation == 0
            || Uuid::parse_str(execution_id).is_err()
            || room_view.is_empty()
            || room_view.len() > MAX_ROOM_VIEW_BYTES
            || !valid_observation_attachments(
                room_view,
                attachment_ids,
                attachment_ingress.is_some(),
            )
            || allowed_agent_ids.len() > MAX_AGENT_IDS
            || unique_agent_ids.len() != allowed_agent_ids.len()
            || allowed_agent_ids.iter().any(|value| !valid_agent_id(value))
        {
            return Err(RoomPortalError::Observation);
        }
        let unique_attachment_ids = attachment_ids.iter().cloned().collect::<HashSet<_>>();
        let authority = TurnAuthority {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            input_up_to_seq,
            durable_turn_generation,
            execution_id: execution_id.to_owned(),
            allowed_agent_ids: allowed_agent_ids.to_vec(),
        };
        let mut state = self.lock_state()?;
        match state.active.as_ref() {
            Some(active)
                if active.authority == authority
                    && active.room_view.as_str() == room_view
                    && active.attachment_ids == unique_attachment_ids
                    && active.attachment_ingress == attachment_ingress
                    && active.tabletop_tools == tabletop_tools
                    && active.tool_ingress == tool_ingress =>
            {
                Ok(())
            }
            Some(_) => Err(RoomPortalError::Observation),
            None => {
                state.active = Some(ActiveObservation {
                    authority,
                    room_view: room_view.to_owned(),
                    attachment_ids: unique_attachment_ids,
                    attachment_ingress,
                    attachment_reads: AttachmentReadBudget::default(),
                    turn_generation: Uuid::new_v4(),
                    receipt_generation: None,
                    outcome: None,
                    tabletop_tools,
                    tool_ingress,
                    tool_reservations: BTreeMap::new(),
                    successful_tool_results: 0,
                    closing: false,
                });
                Ok(())
            }
        }
    }

    pub(crate) fn finish_observation(
        &self,
        turn_id: &str,
        input_up_to_seq: i64,
    ) -> Result<ProviderTurnOutcome, RoomPortalError> {
        self.require_server()?;
        let mut state = self.lock_state()?;
        let active = state.active.as_ref().ok_or(RoomPortalError::Observation)?;
        if active.authority.turn_id != turn_id
            || active.authority.input_up_to_seq != input_up_to_seq
            || active.closing
            || active.has_pending_operations()
        {
            return Err(RoomPortalError::Observation);
        }
        let receipt_generation = active
            .receipt_generation
            .ok_or(RoomPortalError::ReceiptMissing)?;
        if receipt_generation != active.turn_generation {
            return Err(RoomPortalError::OutcomeInvalid);
        }
        let result = match active
            .outcome
            .as_ref()
            .ok_or(RoomPortalError::OutcomeMissing)?
        {
            StagedOutcome::Message {
                receipt_generation: staged_generation,
                content,
                target_agent_id,
            } if *staged_generation == receipt_generation
                && canonical_message(content).is_some()
                && (target_agent_id.is_empty()
                    || active.authority.allowed_agent_ids.contains(target_agent_id)) =>
            {
                ProviderTurnOutcome::Message {
                    content: canonical_message(content).ok_or(RoomPortalError::OutcomeInvalid)?,
                    target_agent_id: target_agent_id.clone(),
                }
            }
            StagedOutcome::Declined {
                receipt_generation: staged_generation,
                reason_code,
            } if *staged_generation == receipt_generation && valid_decline_reason(reason_code) => {
                ProviderTurnOutcome::Declined {
                    reason_code: reason_code.clone(),
                }
            }
            StagedOutcome::Vote {
                receipt_generation: staged_generation,
                command,
            } if *staged_generation == receipt_generation => ProviderTurnOutcome::Vote {
                command: command.clone(),
            },
            _ => return Err(RoomPortalError::OutcomeInvalid),
        };
        state.active = None;
        Ok(result)
    }

    pub(crate) fn end_observation(&self) -> Result<(), RoomPortalError> {
        let mut state = self.lock_state()?;
        let Some(active) = state.active.as_mut() else {
            return Ok(());
        };
        active.closing = true;
        active
            .tool_reservations
            .retain(|_, status| *status == ToolReservationStatus::Executing);
        if !active.has_pending_operations() {
            state.active = None;
        }
        Ok(())
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.server.endpoint()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.server.is_running()
    }

    pub(crate) fn bearer_token(&self) -> &str {
        self.server.bearer_token()
    }

    #[cfg(test)]
    pub(crate) fn bearer_environment_name(&self) -> &str {
        &self.bearer_environment_name
    }

    #[cfg(test)]
    pub(crate) fn active_connection_count(&self) -> usize {
        self.server.active_connection_count()
    }

    fn require_server(&self) -> Result<(), RoomPortalError> {
        if self.server.is_running() {
            Ok(())
        } else {
            Err(RoomPortalError::Mcp)
        }
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, PortalState>, RoomPortalError> {
        self.state.lock().map_err(|_| RoomPortalError::Authority)
    }
}

pub(super) fn reserve_room_tool(
    state: &Arc<Mutex<PortalState>>,
) -> Result<
    (
        RoomToolAuthority,
        RoomToolReservation,
        ProviderRoomToolIngress,
    ),
    String,
> {
    reserve_tool(state, false)
}

pub(super) fn reserve_tabletop_tool(
    state: &Arc<Mutex<PortalState>>,
) -> Result<
    (
        RoomToolAuthority,
        RoomToolReservation,
        ProviderRoomToolIngress,
    ),
    String,
> {
    reserve_tool(state, true)
}

fn reserve_tool(
    state: &Arc<Mutex<PortalState>>,
    require_tabletop: bool,
) -> Result<
    (
        RoomToolAuthority,
        RoomToolReservation,
        ProviderRoomToolIngress,
    ),
    String,
> {
    let mut portal = state
        .lock()
        .map_err(|_| "The shared room authority is unavailable.".to_owned())?;
    let active = portal
        .active
        .as_mut()
        .ok_or_else(|| "No active room observation.".to_owned())?;
    if active.closing || active.outcome.is_some() {
        return Err("This turn already has a terminal room action.".to_owned());
    }
    require_current_receipt(active)?;
    if require_tabletop && !active.tabletop_tools {
        return Err("Room randomness is available only in tabletop mode.".to_owned());
    }
    let ingress = active
        .tool_ingress
        .clone()
        .ok_or_else(|| "The room tool owner is unavailable.".to_owned())?;
    if active.successful_tool_results + active.tool_reservations.len() >= MAX_ROOM_TOOL_RESULTS {
        return Err("This turn reached its room-tool result limit.".to_owned());
    }
    let reservation_id = Uuid::new_v4();
    active
        .tool_reservations
        .insert(reservation_id, ToolReservationStatus::Queued);
    let authority = RoomToolAuthority {
        session_id: active.authority.session_id.clone(),
        turn_id: active.authority.turn_id.clone(),
        input_up_to_seq: active.authority.input_up_to_seq,
        durable_turn_generation: active.authority.durable_turn_generation,
        execution_id: active.authority.execution_id.clone(),
    };
    Ok((
        authority,
        RoomToolReservation {
            state: state.clone(),
            turn_generation: active.turn_generation,
            reservation_id,
            resolved: false,
        },
        ingress,
    ))
}

pub(super) fn require_current_receipt(active: &ActiveObservation) -> Result<(), String> {
    (active.receipt_generation == Some(active.turn_generation))
        .then_some(())
        .ok_or_else(|| "Read the discussion before using a room tool.".to_owned())
}

#[derive(Debug)]
pub(super) struct RoomToolAuthority {
    session_id: String,
    turn_id: String,
    input_up_to_seq: i64,
    durable_turn_generation: u64,
    execution_id: String,
}

fn tool_error(code: &'static str, message: impl Into<String>) -> ProviderRoomToolError {
    ProviderRoomToolError {
        code,
        message: message.into(),
    }
}

fn push_codex_config(arguments: &mut Vec<String>, key: &str, value: &str) {
    arguments.push("-c".to_owned());
    arguments.push(format!("{key}={value}"));
}

fn validate_turn_id(value: &str) -> Result<(), RoomPortalError> {
    if value.is_empty()
        || value.len() > MAX_TURN_ID_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(RoomPortalError::Observation);
    }
    Ok(())
}

fn valid_agent_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TURN_ID_BYTES
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

pub(super) fn canonical_message(value: &str) -> Option<String> {
    if value.contains('\0') || value.chars().count() > MAX_MESSAGE_CHARS {
        return None;
    }
    let value = agentsassemble_domain::clean_message(value, MAX_MESSAGE_CHARS);
    agentsassemble_domain::has_visible_text(&value).then_some(value)
}

pub(super) fn valid_decline_reason(value: &str) -> bool {
    matches!(
        value,
        "nothing_useful_to_add" | "not_addressed" | "duplicate"
    )
}

#[cfg(test)]
#[path = "room_portal_tabletop_tests.rs"]
mod tabletop_tests;

#[cfg(test)]
#[path = "room_portal_attachment_tests.rs"]
mod attachment_tests;

#[cfg(test)]
#[path = "room_portal_search_tests.rs"]
mod search_tests;
