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

const HELPER_NAME: &str = "agentsassemble-room";
#[cfg(unix)]
const HELPER_FILE_NAME: &str = HELPER_NAME;
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
            path_environment,
        })
    }

    pub(crate) fn provider_environment(&self) -> Vec<(String, String)> {
        let _ = self.executable.path();
        vec![("PATH".to_owned(), self.path_environment.clone())]
    }
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
        println!(
            "agentsassemble-room read | speak <message> | speak-to <agent-id> <message> | decline <reason>"
        );
        return Ok(());
    }
    if action == "hook" {
        if arguments.next().is_some() {
            return Err("usage: agentsassemble-room hook");
        }
        return run_hook();
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

fn run_hook() -> Result<(), &'static str> {
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
    let response = if name == Some("run_command") && command.is_some_and(safe_room_command) {
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

#[must_use]
pub fn run_antigravity_hook_if_requested() -> Option<i32> {
    let mut arguments = env::args().skip(1);
    if arguments.next().as_deref() != Some("--agentsassemble-antigravity-hook") {
        return None;
    }
    if arguments.next().is_some() {
        return Some(2);
    }
    Some(match run_hook() {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            2
        }
    })
}

fn decode_antigravity_string_argument(value: &str) -> Option<String> {
    if !(value.starts_with('"') && value.ends_with('"')) {
        return None;
    }
    serde_json::from_str::<String>(value).ok()
}

pub(crate) fn safe_room_command(command: &str) -> bool {
    if command.contains(['\r', '\n']) || shell_metacharacter_outside_single_quotes(command) {
        return false;
    }
    let Some(parts) = shlex::split(command) else {
        return false;
    };
    if parts.len() < 2 || parts[0] != HELPER_NAME {
        return false;
    }
    match parts[1].as_str() {
        "help" | "read" => parts.len() == 2,
        "decline" => {
            parts.len() == 3
                && matches!(
                    parts[2].as_str(),
                    "nothing_useful_to_add" | "not_addressed" | "duplicate"
                )
        }
        "speak" => parts.len() >= 3 && parts[2..].iter().all(|part| !part.starts_with('~')),
        "speak-to" => {
            parts.len() >= 4
                && valid_agent_id(&parts[2])
                && parts[3..].iter().all(|part| !part.starts_with('~'))
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
    use super::{decode_antigravity_string_argument, safe_room_command};

    #[test]
    fn hook_allows_only_one_exact_room_helper_command() {
        for command in [
            "agentsassemble-room help",
            "agentsassemble-room read",
            "agentsassemble-room speak 'hello room'",
            "agentsassemble-room speak-to agent-2 'your turn'",
            "agentsassemble-room decline duplicate",
        ] {
            assert!(
                safe_room_command(command),
                "safe command rejected: {command}"
            );
        }
        for command in [
            "agentsassemble-room read && env",
            "agentsassemble-room speak \"$HOME\"",
            "agentsassemble-room read\nuname",
            "which agentsassemble-room",
            "/tmp/agentsassemble-room read",
        ] {
            assert!(
                !safe_room_command(command),
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
