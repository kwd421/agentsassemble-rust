use std::{
    fs::{File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::Path,
};

use serde_json::{Value, json};

use super::safe_room_command;

const APPROVAL_FILE: &str = "room-hook-approval";
const MAX_HOOK_INPUT_BYTES: u64 = 64 * 1024;
const MAX_APPROVAL_BYTES: u64 = 32;
const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HookApproval {
    RunCommand,
    ViewFile,
}

impl HookApproval {
    const fn encoded(self) -> &'static [u8] {
        match self {
            Self::RunCommand => b"run_command\n",
            Self::ViewFile => b"view_file\n",
        }
    }

    fn decode(encoded: &[u8]) -> Option<Self> {
        match encoded {
            b"run_command\n" => Some(Self::RunCommand),
            b"view_file\n" => Some(Self::ViewFile),
            _ => None,
        }
    }
}

pub(super) fn run_pre_hook(directory: &Path, command_prefix: &str) -> Result<(), &'static str> {
    let payload = read_hook_payload()?;
    let name = payload.pointer("/toolCall/name").and_then(Value::as_str);
    let raw_command = payload
        .pointer("/toolCall/args/CommandLine")
        .and_then(Value::as_str);
    let decoded_command = raw_command.and_then(decode_antigravity_string_argument);
    let command = decoded_command.as_deref().or(raw_command);
    let raw_path = payload
        .pointer("/toolCall/args/AbsolutePath")
        .and_then(Value::as_str);
    let decoded_path = raw_path.and_then(decode_antigravity_string_argument);
    let path = decoded_path.as_deref().or(raw_path);
    let response = pre_hook_response(directory, name, command, path, command_prefix)?;
    serde_json::to_writer(std::io::stdout(), &response)
        .map_err(|_| "hook response could not be written")
}

fn pre_hook_response(
    directory: &Path,
    name: Option<&str>,
    command: Option<&str>,
    path: Option<&str>,
    command_prefix: &str,
) -> Result<Value, &'static str> {
    reset_approval(directory).map_err(|_| "hook approval could not be reset")?;
    let approval = match name {
        Some("run_command")
            if command.is_some_and(|command| safe_room_command(command, command_prefix)) =>
        {
            Some(HookApproval::RunCommand)
        }
        Some("view_file") if path.is_some_and(|path| safe_media_view_path(directory, path)) => {
            Some(HookApproval::ViewFile)
        }
        _ => None,
    };
    let response = if let Some(approval) = approval {
        write_approval(directory, approval).map_err(|_| "hook approval could not be recorded")?;
        json!({
            "decision": "allow",
            "reason": "AgentsAssemble private room operation."
        })
    } else {
        json!({"decision": "deny", "reason": "AgentsAssemble did not approve this request."})
    };
    Ok(response)
}

pub(super) fn run_post_hook(directory: &Path) -> Result<(), &'static str> {
    let _ = read_hook_payload()?;
    reset_approval(directory).map_err(|_| "hook approval could not be reset")?;
    serde_json::to_writer(std::io::stdout(), &json!({}))
        .map_err(|_| "hook response could not be written")
}

pub(super) fn take_approval(directory: &Path) -> std::io::Result<Option<HookApproval>> {
    let path = directory.join(APPROVAL_FILE);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_APPROVAL_BYTES {
        return Err(std::io::Error::other("hook approval is invalid"));
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o077 != 0 {
        return Err(std::io::Error::other("hook approval is not private"));
    }
    let mut encoded = Vec::new();
    file.take(MAX_APPROVAL_BYTES + 1)
        .read_to_end(&mut encoded)?;
    let approval = HookApproval::decode(&encoded)
        .ok_or_else(|| std::io::Error::other("hook approval is invalid"))?;
    std::fs::remove_file(path)?;
    Ok(Some(approval))
}

pub(super) fn reset_approval(directory: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(directory.join(APPROVAL_FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn write_approval(directory: &Path, approval: HookApproval) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(directory.join(APPROVAL_FILE))?;
    file.write_all(approval.encoded())
}

fn safe_media_view_path(directory: &Path, requested: &str) -> bool {
    if requested.is_empty()
        || requested.len() > MAX_PATH_BYTES
        || requested.chars().any(char::is_control)
    {
        return false;
    }
    let requested = Path::new(requested);
    if !requested.is_absolute() {
        return false;
    }
    let media = directory.join(super::MEDIA_DIRECTORY);
    if super::require_private_directory(&media).is_err() {
        return false;
    }
    let (Ok(media), Ok(target)) = (media.canonicalize(), requested.canonicalize()) else {
        return false;
    };
    let Ok(relative) = target.strip_prefix(&media) else {
        return false;
    };
    let mut components = relative.components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return false;
    }
    let Some(parent) = target.parent() else {
        return false;
    };
    if super::require_private_directory(parent).is_err() {
        return false;
    }
    let Ok(metadata) = std::fs::symlink_metadata(&target) else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o077 != 0 {
        return false;
    }
    true
}

fn read_hook_payload() -> Result<Value, &'static str> {
    let mut encoded = String::new();
    std::io::stdin()
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_string(&mut encoded)
        .map_err(|_| "hook request could not be read")?;
    if encoded.len() as u64 > MAX_HOOK_INPUT_BYTES {
        return Err("hook request exceeded its bound");
    }
    serde_json::from_str(&encoded).map_err(|_| "hook request is invalid")
}

fn decode_antigravity_string_argument(value: &str) -> Option<String> {
    if !(value.starts_with('"') && value.ends_with('"')) {
        return None;
    }
    serde_json::from_str::<String>(value).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        HookApproval, decode_antigravity_string_argument, pre_hook_response, reset_approval,
        take_approval, write_approval,
    };
    use crate::ProviderAttachment;

    const HELPER: &str = "'/private/helper path/agentsassemble-room'";

    #[test]
    fn approval_receipt_is_private_one_use_state() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create hook root: {error}"));
        assert_eq!(take_approval(root.path()).unwrap_or(None), None);
        write_approval(root.path(), HookApproval::ViewFile)
            .unwrap_or_else(|error| panic!("write approval: {error}"));
        assert_eq!(
            std::fs::read(root.path().join(super::APPROVAL_FILE))
                .unwrap_or_else(|error| panic!("read approval: {error}")),
            HookApproval::ViewFile.encoded()
        );
        assert_eq!(
            take_approval(root.path()).unwrap_or(None),
            Some(HookApproval::ViewFile)
        );
        assert_eq!(take_approval(root.path()).unwrap_or(None), None);
        reset_approval(root.path()).unwrap_or_else(|error| panic!("reset approval: {error}"));
    }

    #[test]
    fn decodes_only_antigravity_double_serialized_strings() {
        assert_eq!(
            decode_antigravity_string_argument("\"agentsassemble-room help\"").as_deref(),
            Some("agentsassemble-room help")
        );
        assert_eq!(
            decode_antigravity_string_argument("\"agentsassemble-room read\\nprintenv\"")
                .as_deref(),
            Some("agentsassemble-room read\nprintenv")
        );
        assert!(decode_antigravity_string_argument("agentsassemble-room help").is_none());
        assert!(decode_antigravity_string_argument("\"unterminated").is_none());
    }

    #[test]
    fn pre_hook_allows_only_the_exact_helper_and_records_that_decision_once() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create hook root: {error}"));
        let allowed = pre_hook_response(
            root.path(),
            Some("run_command"),
            Some("'/private/helper path/agentsassemble-room' help"),
            None,
            HELPER,
        )
        .unwrap_or_else(|error| panic!("approve exact helper: {error}"));
        assert_eq!(allowed["decision"], "allow");
        assert!(allowed.get("overwrite").is_none());
        assert_eq!(
            take_approval(root.path()).unwrap_or(None),
            Some(HookApproval::RunCommand)
        );

        for (name, command) in [
            (Some("run_command"), Some("git status")),
            (Some("ask_permission"), None),
        ] {
            let denied = pre_hook_response(root.path(), name, command, None, HELPER)
                .unwrap_or_else(|error| panic!("deny unapproved request: {error}"));
            assert_eq!(denied["decision"], "deny");
            assert_eq!(take_approval(root.path()).unwrap_or(None), None);
        }
    }

    #[test]
    fn pre_hook_allows_only_a_current_private_staged_file() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create hook root: {error}"));
        super::super::reset_media_directory(root.path())
            .unwrap_or_else(|error| panic!("create media root: {error}"));
        let path = super::super::stage_attachment(
            root.path(),
            &ProviderAttachment {
                id: "ma_11111111111111111111111111111111".to_owned(),
                filename: "proof.txt".to_owned(),
                content_type: "text/plain".to_owned(),
                size: 5,
                is_image: false,
                content: b"proof".to_vec(),
            },
        )
        .unwrap_or_else(|error| panic!("stage media: {error}"));
        let allowed =
            pre_hook_response(root.path(), Some("view_file"), None, path.to_str(), HELPER)
                .unwrap_or_else(|error| panic!("approve staged file: {error}"));
        assert_eq!(allowed["decision"], "allow");
        assert_eq!(
            take_approval(root.path()).unwrap_or(None),
            Some(HookApproval::ViewFile)
        );

        let sibling = root.path().join("room-portal.json");
        std::fs::write(&sibling, b"private authority")
            .unwrap_or_else(|error| panic!("write sibling authority: {error}"));
        let denied = pre_hook_response(
            root.path(),
            Some("view_file"),
            None,
            sibling.to_str(),
            HELPER,
        )
        .unwrap_or_else(|error| panic!("deny unstaged file: {error}"));
        assert_eq!(denied["decision"], "deny");
        assert_eq!(take_approval(root.path()).unwrap_or(None), None);
    }
}
