use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use thiserror::Error;

const MCP_FLAG: &str = "--agentsassemble-room-portal-mcp";
const ROOT_FLAG: &str = "--root";
const VIEW_FILE: &str = "view.txt";
const TURN_FILE: &str = "turn.json";
const RECEIPT_FILE: &str = "receipt.json";
const OUTCOME_FILE: &str = "outcome.json";
const MAX_PORTAL_FILE_BYTES: usize = 96 * 1024;
const MAX_TURN_ID_BYTES: usize = 128;
const MAX_AGENT_IDS: usize = 64;
const MAX_MESSAGE_CHARS: usize = 12_000;

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
    #[error("the room portal path or file authority is invalid")]
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

#[derive(Debug, Serialize, Deserialize)]
struct TurnAuthority {
    turn_id: String,
    input_up_to_seq: i64,
    allowed_agent_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ObservationReceipt {
    turn_id: String,
    observed_through_seq: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StagedOutcome {
    Message {
        turn_id: String,
        content: String,
        target_agent_id: String,
    },
    Declined {
        turn_id: String,
        reason_code: String,
    },
}

pub(crate) struct RoomPortal {
    root: TempDir,
    server_executable: PathBuf,
}

impl RoomPortal {
    pub(crate) fn create() -> Result<Self, RoomPortalError> {
        let root = tempfile::Builder::new()
            .prefix("agentsassemble-room-portal-")
            .tempdir()
            .map_err(|_| RoomPortalError::Authority)?;
        #[cfg(unix)]
        set_private_directory(root.path())?;
        let server_executable = env::current_exe().map_err(|_| RoomPortalError::Authority)?;
        if !server_executable.is_absolute() {
            return Err(RoomPortalError::Authority);
        }
        Ok(Self {
            root,
            server_executable,
        })
    }

    pub(crate) fn append_codex_config(
        &self,
        arguments: &mut Vec<String>,
    ) -> Result<(), RoomPortalError> {
        let server = "mcp_servers.agentsassemble_room";
        push_codex_config(
            arguments,
            &format!("{server}.command"),
            &serde_json::to_string(&self.server_executable)
                .map_err(|_| RoomPortalError::Authority)?,
        );
        let mcp_arguments = [
            MCP_FLAG.to_owned(),
            ROOT_FLAG.to_owned(),
            self.root.path().to_string_lossy().into_owned(),
        ];
        push_codex_config(
            arguments,
            &format!("{server}.args"),
            &serde_json::to_string(&mcp_arguments).map_err(|_| RoomPortalError::Authority)?,
        );
        push_codex_config(arguments, &format!("{server}.startup_timeout_sec"), "10");
        Ok(())
    }

    pub(crate) fn begin_observation(
        &self,
        turn_id: &str,
        input_up_to_seq: i64,
        room_view: &str,
        allowed_agent_ids: &[String],
    ) -> Result<(), RoomPortalError> {
        validate_turn_id(turn_id)?;
        let unique_agent_ids = allowed_agent_ids.iter().collect::<HashSet<_>>();
        if input_up_to_seq <= 0
            || room_view.is_empty()
            || room_view.len() > MAX_PORTAL_FILE_BYTES
            || allowed_agent_ids.len() > MAX_AGENT_IDS
            || unique_agent_ids.len() != allowed_agent_ids.len()
            || allowed_agent_ids.iter().any(|value| !valid_agent_id(value))
        {
            return Err(RoomPortalError::Observation);
        }
        if fs::symlink_metadata(self.path(TURN_FILE)).is_ok() {
            let active = read_json::<TurnAuthority>(&self.path(TURN_FILE))
                .map_err(|_| RoomPortalError::Observation)?;
            let view =
                read_bounded(&self.path(VIEW_FILE)).map_err(|_| RoomPortalError::Observation)?;
            return if active.turn_id == turn_id
                && active.input_up_to_seq == input_up_to_seq
                && active.allowed_agent_ids == allowed_agent_ids
                && view == room_view.as_bytes()
            {
                Ok(())
            } else {
                Err(RoomPortalError::Observation)
            };
        }
        self.end_observation()?;
        write_private_file(&self.path(VIEW_FILE), room_view.as_bytes())?;
        let turn_result = write_json(
            &self.path(TURN_FILE),
            &TurnAuthority {
                turn_id: turn_id.to_owned(),
                input_up_to_seq,
                allowed_agent_ids: allowed_agent_ids.to_vec(),
            },
        );
        if turn_result.is_err() {
            let _ = remove_if_present(&self.path(VIEW_FILE));
        }
        turn_result
    }

    pub(crate) fn finish_observation(
        &self,
        turn_id: &str,
        input_up_to_seq: i64,
    ) -> Result<ProviderTurnOutcome, RoomPortalError> {
        let authority = read_json::<TurnAuthority>(&self.path(TURN_FILE))
            .map_err(|_| RoomPortalError::Observation)?;
        if authority.turn_id != turn_id || authority.input_up_to_seq != input_up_to_seq {
            return Err(RoomPortalError::Observation);
        }
        let receipt = read_json::<ObservationReceipt>(&self.path(RECEIPT_FILE))
            .map_err(|_| RoomPortalError::ReceiptMissing)?;
        if receipt.turn_id != turn_id || receipt.observed_through_seq != input_up_to_seq {
            return Err(RoomPortalError::ReceiptMissing);
        }
        let outcome = read_json::<StagedOutcome>(&self.path(OUTCOME_FILE))
            .map_err(|_| RoomPortalError::OutcomeMissing)?;
        let result = match outcome {
            StagedOutcome::Message {
                turn_id: staged_turn,
                content,
                target_agent_id,
            } if staged_turn == turn_id
                && canonical_message(&content).is_some()
                && (target_agent_id.is_empty()
                    || authority.allowed_agent_ids.contains(&target_agent_id)) =>
            {
                ProviderTurnOutcome::Message {
                    content: canonical_message(&content).ok_or(RoomPortalError::OutcomeInvalid)?,
                    target_agent_id,
                }
            }
            StagedOutcome::Declined {
                turn_id: staged_turn,
                reason_code,
            } if staged_turn == turn_id && valid_decline_reason(&reason_code) => {
                ProviderTurnOutcome::Declined { reason_code }
            }
            _ => return Err(RoomPortalError::OutcomeInvalid),
        };
        self.end_observation()?;
        Ok(result)
    }

    pub(crate) fn end_observation(&self) -> Result<(), RoomPortalError> {
        for name in [TURN_FILE, VIEW_FILE, RECEIPT_FILE, OUTCOME_FILE] {
            remove_if_present(&self.path(name))?;
        }
        Ok(())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

fn push_codex_config(arguments: &mut Vec<String>, key: &str, value: &str) {
    arguments.push("-c".to_owned());
    arguments.push(format!("{key}={value}"));
}

#[derive(Debug, Clone)]
struct RoomPortalMcp {
    root: PathBuf,
    tool_router: ToolRouter<Self>,
}

impl RoomPortalMcp {
    fn new(root: PathBuf) -> Result<Self, RoomPortalError> {
        if !root.is_absolute() || !private_directory(&root) {
            return Err(RoomPortalError::Authority);
        }
        Ok(Self {
            root,
            tool_router: Self::tool_router(),
        })
    }

    fn turn(&self) -> Result<TurnAuthority, String> {
        read_json(&self.root.join(TURN_FILE)).map_err(|_| "No active room observation.".to_owned())
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PublishMessage {
    content: String,
    #[serde(default)]
    next_agent_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeclineToSpeak {
    reason_code: String,
}

#[tool_router]
impl RoomPortalMcp {
    #[tool(description = "Read the finalized messages in this turn's bounded shared-room view.")]
    fn read_discussion(&self) -> Result<String, String> {
        let turn = self.turn()?;
        let view = read_bounded(&self.root.join(VIEW_FILE))
            .map_err(|_| "The shared room view is unavailable.".to_owned())?;
        write_json(
            &self.root.join(RECEIPT_FILE),
            &ObservationReceipt {
                turn_id: turn.turn_id.clone(),
                observed_through_seq: turn.input_up_to_seq,
            },
        )
        .or_else(|_| {
            let existing = read_json::<ObservationReceipt>(&self.root.join(RECEIPT_FILE))?;
            if existing.turn_id == turn.turn_id
                && existing.observed_through_seq == turn.input_up_to_seq
            {
                Ok(())
            } else {
                Err(RoomPortalError::Authority)
            }
        })
        .map_err(|_| "The room observation receipt could not be recorded.".to_owned())?;
        String::from_utf8(view).map_err(|_| "The shared room view is invalid.".to_owned())
    }

    #[tool(
        description = "Publish one substantive message to the shared room, optionally handing the floor to one exact agent ID."
    )]
    fn publish_message(
        &self,
        Parameters(input): Parameters<PublishMessage>,
    ) -> Result<String, String> {
        let turn = self.turn()?;
        let content = canonical_message(&input.content)
            .ok_or_else(|| "The room publication is invalid.".to_owned())?;
        let target_agent_id = if turn.allowed_agent_ids.contains(&input.next_agent_id) {
            input.next_agent_id
        } else {
            String::new()
        };
        write_json(
            &self.root.join(OUTCOME_FILE),
            &StagedOutcome::Message {
                turn_id: turn.turn_id,
                content,
                target_agent_id,
            },
        )
        .map_err(|_| "This turn already has a terminal room action.".to_owned())?;
        Ok("Published to the shared room.".to_owned())
    }

    #[tool(
        description = "End this room turn without posting, using one supported reason code: nothing_useful_to_add, not_addressed, or duplicate."
    )]
    fn decline_to_speak(
        &self,
        Parameters(input): Parameters<DeclineToSpeak>,
    ) -> Result<String, String> {
        let turn = self.turn()?;
        if !valid_decline_reason(&input.reason_code) {
            return Err("The decline reason is unsupported.".to_owned());
        }
        write_json(
            &self.root.join(OUTCOME_FILE),
            &StagedOutcome::Declined {
                turn_id: turn.turn_id,
                reason_code: input.reason_code,
            },
        )
        .map_err(|_| "This turn already has a terminal room action.".to_owned())?;
        Ok("Declined this shared-room turn.".to_owned())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RoomPortalMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read the bounded shared-room view, then publish once or decline once.",
        )
    }
}

pub async fn run_room_portal_mcp_if_requested() -> Option<Result<(), RoomPortalError>> {
    let mut arguments = env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(MCP_FLAG)) {
        return None;
    }
    let result = async {
        if arguments.next().as_deref() != Some(std::ffi::OsStr::new(ROOT_FLAG)) {
            return Err(RoomPortalError::Authority);
        }
        let root = arguments.next().ok_or(RoomPortalError::Authority)?;
        if arguments.next().is_some() {
            return Err(RoomPortalError::Authority);
        }
        let server = RoomPortalMcp::new(PathBuf::from(root))?;
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|_| RoomPortalError::Mcp)?;
        service
            .waiting()
            .await
            .map(|_| ())
            .map_err(|_| RoomPortalError::Mcp)
    }
    .await;
    Some(result)
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

fn canonical_message(value: &str) -> Option<String> {
    if value.contains('\0') || value.chars().count() > MAX_MESSAGE_CHARS {
        return None;
    }
    let value = agentsassemble_domain::clean_message(value, MAX_MESSAGE_CHARS);
    agentsassemble_domain::has_visible_text(&value).then_some(value)
}

fn valid_decline_reason(value: &str) -> bool {
    matches!(
        value,
        "nothing_useful_to_add" | "not_addressed" | "duplicate"
    )
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), RoomPortalError> {
    let encoded = serde_json::to_vec(value).map_err(|_| RoomPortalError::Authority)?;
    if encoded.len() > MAX_PORTAL_FILE_BYTES {
        return Err(RoomPortalError::Authority);
    }
    write_private_file(path, &encoded)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), RoomPortalError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(path).map_err(|_| RoomPortalError::Authority)?;
    #[cfg(unix)]
    set_private_file(&file)?;
    file.write_all(bytes)
        .map_err(|_| RoomPortalError::Authority)?;
    file.flush().map_err(|_| RoomPortalError::Authority)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, RoomPortalError> {
    let bytes = read_bounded(path)?;
    serde_json::from_slice(&bytes).map_err(|_| RoomPortalError::Authority)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, RoomPortalError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RoomPortalError::Authority)?;
    if !metadata.file_type().is_file()
        || usize::try_from(metadata.len()).unwrap_or(usize::MAX) > MAX_PORTAL_FILE_BYTES
    {
        return Err(RoomPortalError::Authority);
    }
    let mut file = File::open(path).map_err(|_| RoomPortalError::Authority)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(u64::try_from(MAX_PORTAL_FILE_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|_| RoomPortalError::Authority)?;
    if bytes.len() > MAX_PORTAL_FILE_BYTES {
        return Err(RoomPortalError::Authority);
    }
    Ok(bytes)
}

fn remove_if_present(path: &Path) -> Result<(), RoomPortalError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(RoomPortalError::Authority),
    }
}

fn private_directory(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode().trailing_zeros() >= 6
    }
    #[cfg(not(unix))]
    true
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), RoomPortalError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| RoomPortalError::Authority)
}

#[cfg(unix)]
fn set_private_file(file: &File) -> Result<(), RoomPortalError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| RoomPortalError::Authority)
}

#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;

    use super::{DeclineToSpeak, ProviderTurnOutcome, PublishMessage, RoomPortal, RoomPortalMcp};

    #[test]
    fn publication_requires_read_receipt_and_one_terminal_action() {
        let portal = RoomPortal::create()
            .unwrap_or_else(|error| panic!("create room portal fixture: {error}"));
        portal
            .begin_observation(
                "turn-1",
                7,
                "Room: General\n#7 Human: hello",
                &["agent-2".to_owned()],
            )
            .unwrap_or_else(|error| panic!("begin room observation: {error}"));
        let mcp = RoomPortalMcp::new(portal.root.path().to_path_buf())
            .unwrap_or_else(|error| panic!("create portal MCP fixture: {error}"));
        mcp.publish_message(Parameters(PublishMessage {
            content: "  canonical reply  ".to_owned(),
            next_agent_id: "unknown-agent".to_owned(),
        }))
        .unwrap_or_else(|error| panic!("stage pre-read room publication: {error}"));
        portal
            .begin_observation(
                "turn-1",
                7,
                "Room: General\n#7 Human: hello",
                &["agent-2".to_owned()],
            )
            .unwrap_or_else(|error| panic!("resume exact room observation: {error}"));
        assert!(portal.finish_observation("turn-1", 7).is_err());
        let view = mcp
            .read_discussion()
            .unwrap_or_else(|error| panic!("read room discussion: {error}"));
        assert!(view.contains("Human: hello"));
        assert!(
            mcp.decline_to_speak(Parameters(DeclineToSpeak {
                reason_code: "duplicate".to_owned(),
            }))
            .is_err()
        );
        assert_eq!(
            portal
                .finish_observation("turn-1", 7)
                .unwrap_or_else(|error| panic!("finish room observation: {error}")),
            ProviderTurnOutcome::Message {
                content: "canonical reply".to_owned(),
                target_agent_id: String::new(),
            }
        );
    }

    #[test]
    fn decline_is_explicit_and_observation_scoped() {
        let portal = RoomPortal::create()
            .unwrap_or_else(|error| panic!("create decline portal fixture: {error}"));
        portal
            .begin_observation("turn-2", 9, "Room: General\n#9 Human: update", &[])
            .unwrap_or_else(|error| panic!("begin decline observation: {error}"));
        let mcp = RoomPortalMcp::new(portal.root.path().to_path_buf())
            .unwrap_or_else(|error| panic!("create decline MCP fixture: {error}"));
        mcp.decline_to_speak(Parameters(DeclineToSpeak {
            reason_code: "nothing_useful_to_add".to_owned(),
        }))
        .unwrap_or_else(|error| panic!("stage room decline: {error}"));
        assert!(portal.finish_observation("turn-2", 9).is_err());
        mcp.read_discussion()
            .unwrap_or_else(|error| panic!("read decline discussion: {error}"));
        assert_eq!(
            portal
                .finish_observation("turn-2", 9)
                .unwrap_or_else(|error| panic!("finish room decline: {error}")),
            ProviderTurnOutcome::Declined {
                reason_code: "nothing_useful_to_add".to_owned(),
            }
        );
    }
}
