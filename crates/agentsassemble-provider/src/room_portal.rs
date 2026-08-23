use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard},
};

use thiserror::Error;
use uuid::Uuid;

use crate::room_portal_mcp::PortalServer;
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, room_portal_terminal::RoomPortalTerminalHelper};

pub(super) const ROOM_PORTAL_TOKEN_ENV_PREFIX: &str = "AGENTSASSEMBLE_INTERNAL_ROOM_PORTAL_TOKEN_";

const MAX_ROOM_VIEW_BYTES: usize = 96 * 1024;
const MAX_TURN_ID_BYTES: usize = 128;
const MAX_AGENT_IDS: usize = 64;
pub(super) const MAX_MESSAGE_CHARS: usize = 12_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderTurnOutcome {
    Message {
        content: String,
        target_agent_id: String,
    },
    Declined {
        reason_code: String,
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
    #[error("the provider did not stage exactly one room publication or decline")]
    OutcomeMissing,
    #[error("the provider staged an invalid room publication or decline")]
    OutcomeInvalid,
    #[error("the room portal MCP server failed")]
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TurnAuthority {
    pub(super) turn_id: String,
    pub(super) input_up_to_seq: i64,
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
}

#[derive(Debug)]
pub(super) struct ActiveObservation {
    pub(super) authority: TurnAuthority,
    pub(super) room_view: String,
    pub(super) turn_generation: Uuid,
    pub(super) receipt_generation: Option<Uuid>,
    pub(super) outcome: Option<StagedOutcome>,
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

    #[cfg(not(unix))]
    pub(crate) fn configure_environment(&self, command: &mut tokio::process::Command) {
        command.envs(self.provider_environment());
    }

    pub(crate) fn begin_observation(
        &self,
        turn_id: &str,
        input_up_to_seq: i64,
        room_view: &str,
        allowed_agent_ids: &[String],
    ) -> Result<(), RoomPortalError> {
        self.require_server()?;
        validate_turn_id(turn_id)?;
        let unique_agent_ids = allowed_agent_ids.iter().collect::<HashSet<_>>();
        if input_up_to_seq <= 0
            || room_view.is_empty()
            || room_view.len() > MAX_ROOM_VIEW_BYTES
            || allowed_agent_ids.len() > MAX_AGENT_IDS
            || unique_agent_ids.len() != allowed_agent_ids.len()
            || allowed_agent_ids.iter().any(|value| !valid_agent_id(value))
        {
            return Err(RoomPortalError::Observation);
        }
        let authority = TurnAuthority {
            turn_id: turn_id.to_owned(),
            input_up_to_seq,
            allowed_agent_ids: allowed_agent_ids.to_vec(),
        };
        let mut state = self.lock_state()?;
        match state.active.as_ref() {
            Some(active)
                if active.authority == authority && active.room_view.as_str() == room_view =>
            {
                Ok(())
            }
            Some(_) => Err(RoomPortalError::Observation),
            None => {
                state.active = Some(ActiveObservation {
                    authority,
                    room_view: room_view.to_owned(),
                    turn_generation: Uuid::new_v4(),
                    receipt_generation: None,
                    outcome: None,
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
            _ => return Err(RoomPortalError::OutcomeInvalid),
        };
        state.active = None;
        Ok(result)
    }

    pub(crate) fn end_observation(&self) -> Result<(), RoomPortalError> {
        self.lock_state()?.active = None;
        Ok(())
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.server.endpoint()
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
