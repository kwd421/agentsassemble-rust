use std::{
    env,
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[cfg(windows)]
use crate::filesystem::BoundExecutable;
#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{filesystem::PrivateExecutable, room_portal::RoomPortalError};

#[cfg(unix)]
const HELPER_FILE_NAME: &str = "agentsassemble-room";
#[cfg(windows)]
const HELPER_FILE_NAME: &str = "agentsassemble-room.exe";
const AUTHORITY_FILE: &str = "room-portal.json";
const MAX_AUTHORITY_BYTES: u64 = 4 * 1024;
const MAX_HOOK_INPUT_BYTES: u64 = 64 * 1024;
const HELPER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize, Serialize)]
struct HelperAuthority {
    endpoint: String,
    bearer_token: String,
}

pub(crate) struct RoomPortalTerminalHelper {
    executable: PrivateExecutable,
    command_prefix: String,
    hook_command: String,
    path_environment: String,
}

impl RoomPortalTerminalHelper {
    #[cfg(unix)]
    pub(crate) fn create(
        guardian: &GuardianLaunch,
        endpoint: &str,
        bearer_token: &str,
    ) -> Result<Self, RoomPortalError> {
        let executable = guardian
            .stage_companion(HELPER_FILE_NAME)
            .map_err(|_| RoomPortalError::Authority)?;
        Self::from_executable(executable, endpoint, bearer_token)
    }

    #[cfg(windows)]
    pub(crate) fn create(
        companion: &BoundExecutable,
        endpoint: &str,
        bearer_token: &str,
    ) -> Result<Self, RoomPortalError> {
        let executable = companion
            .stage_private_companion(HELPER_FILE_NAME)
            .map_err(|_| RoomPortalError::Authority)?;
        Self::from_executable(executable, endpoint, bearer_token)
    }

    fn from_executable(
        executable: PrivateExecutable,
        endpoint: &str,
        bearer_token: &str,
    ) -> Result<Self, RoomPortalError> {
        write_authority(
            executable.directory(),
            &HelperAuthority {
                endpoint: endpoint.to_owned(),
                bearer_token: bearer_token.to_owned(),
            },
        )?;
        let command_prefix = absolute_helper_command(executable.path())?;
        let hook_command = format!("{command_prefix} hook");
        let current_path = env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![executable.directory().to_path_buf()];
        paths.extend(env::split_paths(&current_path));
        let path_environment = env::join_paths(paths)
            .map_err(|_| RoomPortalError::Authority)?
            .into_string()
            .map_err(|_| RoomPortalError::Authority)?;
        if path_environment.len() > 4096 {
            return Err(RoomPortalError::Authority);
        }
        Ok(Self {
            executable,
            command_prefix,
            hook_command,
            path_environment,
        })
    }

    pub(crate) fn command_prefix(&self) -> &str {
        &self.command_prefix
    }

    pub(crate) fn hook_command(&self) -> &str {
        &self.hook_command
    }

    pub(crate) fn provider_environment(&self) -> Vec<(String, String)> {
        let _ = self.executable.path();
        vec![("PATH".to_owned(), self.path_environment.clone())]
    }
}

#[cfg(unix)]
fn absolute_helper_command(executable: &Path) -> Result<String, RoomPortalError> {
    let executable = executable.to_str().ok_or(RoomPortalError::Authority)?;
    if executable
        .chars()
        .any(|character| character.is_control() || matches!(character, '`' | '<' | '>'))
    {
        return Err(RoomPortalError::Authority);
    }
    shlex::try_join([executable]).map_err(|_| RoomPortalError::Authority)
}

#[cfg(windows)]
fn absolute_helper_command(executable: &Path) -> Result<String, RoomPortalError> {
    let executable = executable.to_str().ok_or(RoomPortalError::Authority)?;
    if executable.chars().any(|character| {
        !(character.is_alphanumeric()
            || matches!(character, ' ' | '-' | '_' | '.' | ':' | '\\' | '/' | '~'))
    }) {
        return Err(RoomPortalError::Authority);
    }
    Ok(format!(r#""{executable}""#))
}

fn write_authority(directory: &Path, authority: &HelperAuthority) -> Result<(), RoomPortalError> {
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let path = directory.join(AUTHORITY_FILE);
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|_| RoomPortalError::Authority)?;
    serde_json::to_writer(&mut file, authority).map_err(|_| RoomPortalError::Authority)?;
    file.write_all(b"\n")
        .map_err(|_| RoomPortalError::Authority)?;
    file.sync_all().map_err(|_| RoomPortalError::Authority)?;
    #[cfg(unix)]
    if file
        .metadata()
        .map_err(|_| RoomPortalError::Authority)?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err(RoomPortalError::Authority);
    }
    Ok(())
}

pub async fn run_room_helper_if_requested() -> Option<i32> {
    let executable = env::current_exe().ok()?;
    if executable.file_name() != Some(OsStr::new(HELPER_FILE_NAME)) {
        return None;
    }
    Some(
        match tokio::time::timeout(HELPER_TIMEOUT, run_helper(executable)).await {
            Ok(Ok(())) => 0,
            Ok(Err(message)) => {
                eprintln!("{message}");
                2
            }
            Err(_) => {
                eprintln!("room helper timed out");
                2
            }
        },
    )
}

async fn run_helper(executable: PathBuf) -> Result<(), &'static str> {
    let mut arguments = env::args().skip(1);
    let action = arguments.next().unwrap_or_else(|| "help".to_owned());
    if action == "help" {
        if arguments.next().is_some() {
            return Err("usage: agentsassemble-room help");
        }
        let helper = absolute_helper_command(&executable)
            .map_err(|_| "room helper invocation is unavailable")?;
        println!(
            "{helper} read | speak <message> | speak-to <agent-id> <message> | decline <reason>"
        );
        return Ok(());
    }
    if action == "hook" {
        if arguments.next().is_some() {
            return Err("usage: agentsassemble-room hook");
        }
        let helper = absolute_helper_command(&executable)
            .map_err(|_| "room helper invocation is unavailable")?;
        return run_hook(&helper);
    }
    let authority = read_authority(
        executable
            .parent()
            .ok_or("room helper authority is unavailable")?,
    )?;
    let (tool, payload) = match action.as_str() {
        "read" if arguments.next().is_none() => ("read_discussion", json!({})),
        "speak" => {
            let content = arguments.collect::<Vec<_>>().join(" ").trim().to_owned();
            if content.is_empty() {
                return Err("usage: agentsassemble-room speak <message>");
            }
            (
                "publish_message",
                json!({"content": content, "next_agent_id": ""}),
            )
        }
        "speak-to" => {
            let target = arguments
                .next()
                .filter(|value| valid_agent_id(value))
                .ok_or("usage: agentsassemble-room speak-to <agent-id> <message>")?;
            let content = arguments.collect::<Vec<_>>().join(" ").trim().to_owned();
            if content.is_empty() {
                return Err("usage: agentsassemble-room speak-to <agent-id> <message>");
            }
            (
                "publish_message",
                json!({"content": content, "next_agent_id": target}),
            )
        }
        "decline" => {
            let reason = arguments
                .next()
                .filter(|value| {
                    matches!(
                        value.as_str(),
                        "nothing_useful_to_add" | "not_addressed" | "duplicate"
                    )
                })
                .ok_or("usage: agentsassemble-room decline <reason>")?;
            if arguments.next().is_some() {
                return Err("usage: agentsassemble-room decline <reason>");
            }
            ("decline_to_speak", json!({"reason_code": reason}))
        }
        _ => return Err("unsupported room helper command"),
    };
    let result = call_tool(&authority, tool, payload).await?;
    if result.is_error == Some(true) {
        return Err("room helper action was rejected");
    }
    if tool == "read_discussion" {
        let content = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .ok_or("room helper returned no discussion")?;
        print!("{content}");
    } else if tool == "decline_to_speak" {
        println!("{{\"declined\":true}}");
    } else {
        println!("room message staged");
    }
    Ok(())
}

async fn call_tool(
    authority: &HelperAuthority,
    name: &'static str,
    arguments: Value,
) -> Result<rmcp::model::CallToolResult, &'static str> {
    if !valid_authority(authority) {
        return Err("room helper authority is invalid");
    }
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(authority.endpoint.as_str())
                .auth_header(&authority.bearer_token),
        ))
        .await
        .map_err(|_| "room helper could not connect")?;
    let arguments = serde_json::from_value::<Map<String, Value>>(arguments)
        .map_err(|_| "room helper arguments are invalid")?;
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .map_err(|_| "room helper request failed");
    let _ = client.cancel().await;
    result
}

fn read_authority(directory: &Path) -> Result<HelperAuthority, &'static str> {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join(AUTHORITY_FILE);
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| "room helper authority is unavailable")?;
    if !metadata.is_file() || metadata.len() > MAX_AUTHORITY_BYTES {
        return Err("room helper authority is invalid");
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err("room helper authority is not private");
    }
    let mut encoded = String::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_AUTHORITY_BYTES + 1)
                .read_to_string(&mut encoded)
        })
        .map_err(|_| "room helper authority is unavailable")?;
    if encoded.len() as u64 > MAX_AUTHORITY_BYTES {
        return Err("room helper authority is invalid");
    }
    serde_json::from_str(&encoded).map_err(|_| "room helper authority is invalid")
}

fn valid_authority(authority: &HelperAuthority) -> bool {
    authority.bearer_token.len() <= 128
        && !authority.bearer_token.is_empty()
        && authority.endpoint.parse::<hyper::Uri>().is_ok_and(|url| {
            url.scheme_str() == Some("http")
                && url.host() == Some("127.0.0.1")
                && url.port_u16().is_some()
                && url.path().starts_with("/portal/")
        })
}

fn run_hook(command_prefix: &str) -> Result<(), &'static str> {
    let mut encoded = String::new();
    std::io::stdin()
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_string(&mut encoded)
        .map_err(|_| "hook request could not be read")?;
    if encoded.len() as u64 > MAX_HOOK_INPUT_BYTES {
        return Err("hook request exceeded its bound");
    }
    let payload: Value = serde_json::from_str(&encoded).map_err(|_| "hook request is invalid")?;
    let name = payload.pointer("/toolCall/name").and_then(Value::as_str);
    let raw_command = payload
        .pointer("/toolCall/args/CommandLine")
        .and_then(Value::as_str);
    let decoded_command = raw_command.and_then(decode_antigravity_string_argument);
    let command = decoded_command.as_deref().or(raw_command);
    let response = if name == Some("run_command")
        && command.is_some_and(|command| safe_room_command(command, command_prefix))
    {
        json!({
            "decision": "allow",
            "reason": "AgentsAssemble room tool command.",
            "overwrite": {"BypassSandbox": true}
        })
    } else {
        json!({"decision": "deny", "reason": "AgentsAssemble did not approve this request."})
    };
    serde_json::to_writer(std::io::stdout(), &response)
        .map_err(|_| "hook response could not be written")?;
    Ok(())
}

fn decode_antigravity_string_argument(value: &str) -> Option<String> {
    if !(value.starts_with('"') && value.ends_with('"')) {
        return None;
    }
    serde_json::from_str::<String>(value).ok()
}

pub(crate) fn safe_room_command(command: &str, command_prefix: &str) -> bool {
    let Some(arguments) = command
        .strip_prefix(command_prefix)
        .and_then(|suffix| suffix.strip_prefix(' '))
    else {
        return false;
    };
    if arguments.contains(['\r', '\n']) || shell_metacharacter_outside_single_quotes(arguments) {
        return false;
    }
    let Some(parts) = shlex::split(arguments) else {
        return false;
    };
    if parts.is_empty() {
        return false;
    }
    match parts[0].as_str() {
        "help" | "read" => parts.len() == 1,
        "decline" => {
            parts.len() == 2
                && matches!(
                    parts[1].as_str(),
                    "nothing_useful_to_add" | "not_addressed" | "duplicate"
                )
        }
        "speak" => parts.len() >= 2 && parts[1..].iter().all(|part| !part.starts_with('~')),
        "speak-to" => {
            parts.len() >= 3
                && valid_agent_id(&parts[1])
                && parts[2..].iter().all(|part| !part.starts_with('~'))
        }
        _ => false,
    }
}

fn shell_metacharacter_outside_single_quotes(command: &str) -> bool {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && !single {
            escaped = true;
            continue;
        }
        if character == '\'' && !double {
            single = !single;
            continue;
        }
        if character == '"' && !single {
            double = !double;
            continue;
        }
        if !single
            && matches!(
                character,
                '$' | '`' | ';' | '&' | '|' | '<' | '>' | '(' | ')'
            )
        {
            return true;
        }
    }
    single || double || escaped
}

fn valid_agent_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{absolute_helper_command, decode_antigravity_string_argument, safe_room_command};

    const HELPER: &str = "'/private/helper path/agentsassemble-room'";

    #[cfg(unix)]
    #[test]
    fn hook_command_binds_the_absolute_private_helper() {
        let executable = std::path::Path::new("/private/helper path/agentsassemble-room");
        let command = absolute_helper_command(executable)
            .unwrap_or_else(|error| panic!("build absolute hook command: {error}"));
        assert_eq!(
            shlex::split(&command),
            Some(vec![executable.to_string_lossy().into_owned(),])
        );
        assert_ne!(command, "agentsassemble-room");
    }

    #[cfg(windows)]
    #[test]
    fn hook_command_binds_the_absolute_private_helper() {
        let executable = std::path::Path::new(r"C:\private helper\agentsassemble-room.exe");
        assert_eq!(
            absolute_helper_command(executable)
                .unwrap_or_else(|error| panic!("build absolute hook command: {error}")),
            r#""C:\private helper\agentsassemble-room.exe""#
        );
    }

    #[cfg(windows)]
    #[test]
    fn workspace_shadow_cannot_replace_the_bound_helper() {
        use std::process::Command;

        use crate::antigravity_hook::AntigravityHookRegistration;

        let root =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create shadow test root: {error}"));
        let workspace = root.path().join("workspace");
        let private = root.path().join("private helper");
        std::fs::create_dir(&workspace)
            .unwrap_or_else(|error| panic!("create shadow workspace: {error}"));
        std::fs::create_dir(&private)
            .unwrap_or_else(|error| panic!("create private helper directory: {error}"));
        let decoy = workspace.join("agentsassemble-room.cmd");
        let helper = private.join("agentsassemble-room.cmd");
        let decoy_marker = root.path().join("decoy.marker");
        let private_marker = root.path().join("private.marker");
        std::fs::write(&decoy, "@echo off\r\necho decoy>>\"%DECOY_MARKER%\"\r\n")
            .unwrap_or_else(|error| panic!("write shadow helper: {error}"));
        std::fs::write(
            &helper,
            "@echo off\r\necho private>>\"%PRIVATE_MARKER%\"\r\n",
        )
        .unwrap_or_else(|error| panic!("write bound helper: {error}"));
        let prefix = absolute_helper_command(&helper)
            .unwrap_or_else(|error| panic!("quote bound helper: {error}"));
        let hook_command = format!("{prefix} hook");
        let registration = AntigravityHookRegistration::register(&workspace, &hook_command)
            .unwrap_or_else(|error| panic!("register bound hook: {error}"));
        let hooks: serde_json::Value = serde_json::from_slice(
            &std::fs::read(workspace.join(".agents").join("hooks.json"))
                .unwrap_or_else(|error| panic!("read bound hook: {error}")),
        )
        .unwrap_or_else(|error| panic!("decode bound hook: {error}"));
        let installed =
            hooks["agentsassemble-room-requests"]["PreToolUse"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap_or_else(|| panic!("installed hook command is missing"));
        run_windows_command(
            installed,
            &workspace,
            &private,
            &private_marker,
            &decoy_marker,
        );
        let approved = format!("{prefix} read");
        assert!(safe_room_command(&approved, &prefix));
        assert!(!safe_room_command("agentsassemble-room read", &prefix));
        run_windows_command(
            &approved,
            &workspace,
            &private,
            &private_marker,
            &decoy_marker,
        );
        assert!(!decoy_marker.exists(), "workspace helper was executed");
        let executions = std::fs::read_to_string(&private_marker)
            .unwrap_or_else(|error| panic!("read private helper marker: {error}"));
        assert_eq!(executions.lines().count(), 2);
        drop(registration);

        fn run_windows_command(
            command: &str,
            workspace: &std::path::Path,
            private: &std::path::Path,
            private_marker: &std::path::Path,
            decoy_marker: &std::path::Path,
        ) {
            let path = std::env::join_paths(std::iter::once(private.to_path_buf()).chain(
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
            ))
            .unwrap_or_else(|error| panic!("build shadow test PATH: {error}"));
            let status = Command::new("cmd.exe")
                .args(["/D", "/S", "/C", command])
                .current_dir(workspace)
                .env("PATH", path)
                .env("PRIVATE_MARKER", private_marker)
                .env("DECOY_MARKER", decoy_marker)
                .status()
                .unwrap_or_else(|error| panic!("run bound helper command: {error}"));
            assert!(status.success(), "bound helper command failed: {status}");
        }
    }

    #[test]
    fn hook_allows_only_one_exact_room_helper_command() {
        for command in [
            format!("{HELPER} help"),
            format!("{HELPER} read"),
            format!("{HELPER} speak 'hello room'"),
            format!("{HELPER} speak-to agent-2 'your turn'"),
            format!("{HELPER} decline duplicate"),
        ] {
            assert!(
                safe_room_command(&command, HELPER),
                "safe command rejected: {command}"
            );
        }
        for command in [
            format!("{HELPER} read && env"),
            format!("{HELPER} speak \"$HOME\""),
            format!("{HELPER} read\nuname"),
            "agentsassemble-room read".to_owned(),
            "/tmp/agentsassemble-room read".to_owned(),
        ] {
            assert!(
                !safe_room_command(&command, HELPER),
                "unsafe command allowed: {command}"
            );
        }
    }

    #[test]
    fn hook_decodes_antigravity_double_serialized_string_arguments() {
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
}
