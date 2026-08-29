use std::{
    env,
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
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
use serde_json::{Map, Value};

#[path = "room_portal_terminal_command.rs"]
mod command;
#[path = "room_portal_terminal_hook.rs"]
mod hook;

#[cfg(windows)]
use crate::filesystem::BoundExecutable;
#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    filesystem::PrivateExecutable,
    room_attachment::{ProviderAttachment, attachment_from_tool_result},
    room_portal::RoomPortalError,
};
use command::helper_tool;
pub(crate) use command::safe_room_command;
pub(crate) use hook::HookApproval;

#[cfg(unix)]
const HELPER_FILE_NAME: &str = "agentsassemble-room";
#[cfg(windows)]
const HELPER_FILE_NAME: &str = "agentsassemble-room.exe";
const AUTHORITY_FILE: &str = "room-portal.json";
const MEDIA_DIRECTORY: &str = "room-media";
const MAX_AUTHORITY_BYTES: u64 = 4 * 1024;
const HELPER_TIMEOUT: Duration = Duration::from_secs(10);
const HELPER_COMMAND_ENV: &str = "AGENTSASSEMBLE_ROOM_HELPER_COMMAND";

#[derive(Deserialize, Serialize)]
struct HelperAuthority {
    endpoint: String,
    bearer_token: String,
}

pub(crate) struct RoomPortalTerminalHelper {
    executable: Arc<PrivateExecutable>,
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
        let executable = Arc::new(executable);
        write_authority(
            executable.directory(),
            &HelperAuthority {
                endpoint: endpoint.to_owned(),
                bearer_token: bearer_token.to_owned(),
            },
        )?;
        reset_media_directory(executable.directory())?;
        hook::reset_approval(executable.directory()).map_err(|_| RoomPortalError::Authority)?;
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

    pub(crate) fn hook_executable_owner(&self) -> Arc<dyn Send + Sync> {
        self.executable.clone()
    }

    pub(crate) fn provider_environment(&self) -> Vec<(String, String)> {
        let _ = self.executable.path();
        vec![
            ("PATH".to_owned(), self.path_environment.clone()),
            (HELPER_COMMAND_ENV.to_owned(), self.command_prefix.clone()),
        ]
    }

    pub(crate) fn reset_turn_state(&self) -> Result<(), RoomPortalError> {
        hook::reset_approval(self.executable.directory())
            .map_err(|_| RoomPortalError::Authority)?;
        reset_media_directory(self.executable.directory())
    }

    pub(crate) fn take_hook_approval(&self) -> Result<Option<HookApproval>, RoomPortalError> {
        hook::take_approval(self.executable.directory()).map_err(|_| RoomPortalError::Authority)
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
        return print_helper_help(&executable, &mut arguments);
    }
    if action == "hook" {
        let helper = env::var(HELPER_COMMAND_ENV)
            .ok()
            .ok_or("room helper invocation is unavailable")?;
        let directory =
            hook_session_directory(&helper).ok_or("room helper authority is unavailable")?;
        return match arguments.next().as_deref() {
            Some("pre") if arguments.next().is_none() => hook::run_pre_hook(&directory, &helper),
            Some("post") if arguments.next().is_none() => hook::run_post_hook(&directory),
            _ => Err("usage: agentsassemble-room hook <pre|post>"),
        };
    }
    let directory = executable
        .parent()
        .ok_or("room helper authority is unavailable")?;
    let authority = read_authority(directory)?;
    let (tool, payload) = helper_tool(&action, arguments)?;
    let result = call_tool(&authority, tool, payload).await?;
    render_helper_result(directory, tool, &result)
}

fn print_helper_help(
    executable: &Path,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<(), &'static str> {
    if arguments.next().is_some() {
        return Err("usage: agentsassemble-room help");
    }
    let helper =
        absolute_helper_command(executable).map_err(|_| "room helper invocation is unavailable")?;
    println!(
        "{helper} read | media <attachment-id> | speak <message> | speak-to <agent-id> <message> | decline <reason> | roll <NdS+M> | choose <json-options>"
    );
    Ok(())
}

fn render_helper_result(
    directory: &Path,
    tool: &str,
    result: &rmcp::model::CallToolResult,
) -> Result<(), &'static str> {
    if result.is_error == Some(true) {
        return Err("room helper action was rejected");
    }
    if tool == "read_attachment" {
        let attachment = attachment_from_tool_result(result)?;
        let path = stage_attachment(directory, &attachment)?;
        println!(
            "{}",
            path.to_str()
                .ok_or("room helper media path is unavailable")?
        );
    } else if tool == "read_discussion" {
        let content = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .ok_or("room helper returned no discussion")?;
        print!("{content}");
    } else if tool == "decline_to_speak" {
        println!("{{\"declined\":true}}");
    } else if matches!(tool, "roll_dice" | "choose_random") {
        let content = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .ok_or("room helper returned no random result")?;
        println!("{content}");
    } else {
        println!("room message staged");
    }
    Ok(())
}

fn reset_media_directory(directory: &Path) -> Result<(), RoomPortalError> {
    recreate_private_directory(&directory.join(MEDIA_DIRECTORY))
        .map_err(|_| RoomPortalError::Authority)
}

fn stage_attachment(
    directory: &Path,
    attachment: &ProviderAttachment,
) -> Result<PathBuf, &'static str> {
    if !attachment.is_valid() {
        return Err("room helper returned an invalid attachment");
    }
    let media = directory.join(MEDIA_DIRECTORY);
    require_private_directory(&media).map_err(|_| "room helper media directory is invalid")?;
    let attachment_directory = media.join(&attachment.id);
    recreate_private_directory(&attachment_directory)
        .map_err(|_| "room helper could not stage attachment")?;
    let target = attachment_directory.join(&attachment.filename);
    let temporary = attachment_directory.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "room helper could not stage attachment")?;
    if file.write_all(&attachment.content).is_err() || file.sync_all().is_err() {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err("room helper could not stage attachment");
    }
    drop(file);
    if std::fs::rename(&temporary, &target).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err("room helper could not stage attachment");
    }
    Ok(target)
}

fn recreate_private_directory(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)?;
        }
        Ok(_) => return Err(std::io::Error::other("private path is not a directory")),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::create_dir(path)?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

fn require_private_directory(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other("private path is not a directory"));
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o077 != 0 {
        return Err(std::io::Error::other("private directory is not private"));
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

fn helper_executable_from_command_prefix(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > 4096 {
        return None;
    }
    #[cfg(unix)]
    let path = match shlex::split(value).as_deref() {
        Some([path]) => PathBuf::from(path),
        _ => return None,
    };
    #[cfg(windows)]
    let path = PathBuf::from(
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))?,
    );
    (path.is_absolute()
        && path.file_name() == Some(OsStr::new(HELPER_FILE_NAME))
        && absolute_helper_command(&path).is_ok_and(|canonical| canonical == value))
    .then_some(path)
}

fn hook_session_directory(command_prefix: &str) -> Option<PathBuf> {
    let executable = helper_executable_from_command_prefix(command_prefix)?;
    let metadata = std::fs::symlink_metadata(&executable).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o077 != 0 {
        return None;
    }
    let directory = executable.parent()?;
    require_private_directory(directory).ok()?;
    Some(directory.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{
        absolute_helper_command, helper_executable_from_command_prefix, reset_media_directory,
        safe_room_command, stage_attachment,
    };
    use crate::ProviderAttachment;

    const HELPER: &str = "'/private/helper path/agentsassemble-room'";
    const ATTACHMENT_ID: &str = "ma_11111111111111111111111111111111";

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
        assert!(helper_executable_from_command_prefix(&command).is_some());
        assert!(helper_executable_from_command_prefix("agentsassemble-room").is_none());
        assert_ne!(command, "agentsassemble-room");
    }

    #[cfg(windows)]
    #[test]
    fn hook_command_binds_the_absolute_private_helper() {
        let executable = std::path::Path::new(r"C:\private helper\agentsassemble-room.exe");
        let command = absolute_helper_command(executable)
            .unwrap_or_else(|error| panic!("build absolute hook command: {error}"));
        assert_eq!(command, r#""C:\private helper\agentsassemble-room.exe""#);
        assert!(helper_executable_from_command_prefix(&command).is_some());
        assert!(helper_executable_from_command_prefix("agentsassemble-room").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn workspace_shadow_cannot_replace_shared_multi_session_helpers() {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::sync::Arc;

        use crate::antigravity_hook::AntigravityHookRegistration;

        let root =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create shadow test root: {error}"));
        let workspace = root.path().join("workspace");
        let private_a = root.path().join("private helper a");
        let private_b = root.path().join("private helper b");
        std::fs::create_dir(&workspace)
            .unwrap_or_else(|error| panic!("create shadow workspace: {error}"));
        std::fs::create_dir(&private_a)
            .unwrap_or_else(|error| panic!("create first private helper directory: {error}"));
        std::fs::create_dir(&private_b)
            .unwrap_or_else(|error| panic!("create second private helper directory: {error}"));
        let decoy = workspace.join("agentsassemble-room.cmd");
        let helper_a = private_a.join("agentsassemble-room.cmd");
        let helper_b = private_b.join("agentsassemble-room.cmd");
        let decoy_marker = root.path().join("decoy.marker");
        let marker_a = root.path().join("private-a.marker");
        let marker_b = root.path().join("private-b.marker");
        std::fs::write(&decoy, "@echo off\r\necho decoy>>\"%DECOY_MARKER%\"\r\n")
            .unwrap_or_else(|error| panic!("write shadow helper: {error}"));
        std::fs::write(
            &helper_a,
            "@echo off\r\necho private-a>>\"%PRIVATE_A_MARKER%\"\r\n",
        )
        .unwrap_or_else(|error| panic!("write first bound helper: {error}"));
        std::fs::write(
            &helper_b,
            "@echo off\r\necho private-b>>\"%PRIVATE_B_MARKER%\"\r\n",
        )
        .unwrap_or_else(|error| panic!("write second bound helper: {error}"));
        let prefix_a = absolute_helper_command(&helper_a)
            .unwrap_or_else(|error| panic!("quote first bound helper: {error}"));
        let prefix_b = absolute_helper_command(&helper_b)
            .unwrap_or_else(|error| panic!("quote second bound helper: {error}"));
        let registration_a = AntigravityHookRegistration::register(
            &workspace,
            &format!("{prefix_a} hook"),
            Arc::new(()),
        )
        .unwrap_or_else(|error| panic!("register first bound hook: {error}"));
        let registration_b = AntigravityHookRegistration::register(
            &workspace,
            &format!("{prefix_b} hook"),
            Arc::new(()),
        )
        .unwrap_or_else(|error| panic!("register second bound hook: {error}"));
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
            &[&private_a, &private_b],
            &prefix_a,
            &marker_a,
            &marker_b,
            &decoy_marker,
        );
        let approved_a = format!("{prefix_a} read");
        assert!(safe_room_command(&approved_a, &prefix_a));
        assert!(!safe_room_command("agentsassemble-room read", &prefix_a));
        run_windows_command(
            &approved_a,
            &workspace,
            &[&private_a, &private_b],
            &prefix_a,
            &marker_a,
            &marker_b,
            &decoy_marker,
        );
        drop(registration_a);
        assert!(workspace.join(".agents").join("hooks.json").exists());
        let approved_b = format!("{prefix_b} read");
        assert!(safe_room_command(&approved_b, &prefix_b));
        run_windows_command(
            &approved_b,
            &workspace,
            &[&private_a, &private_b],
            &prefix_b,
            &marker_a,
            &marker_b,
            &decoy_marker,
        );
        assert!(!decoy_marker.exists(), "workspace helper was executed");
        assert_eq!(marker_lines(&marker_a), 2);
        assert_eq!(marker_lines(&marker_b), 1);
        drop(registration_b);
        assert!(!workspace.join(".agents").join("hooks.json").exists());

        fn run_windows_command(
            command: &str,
            workspace: &std::path::Path,
            private: &[&std::path::Path],
            command_prefix: &str,
            marker_a: &std::path::Path,
            marker_b: &std::path::Path,
            decoy_marker: &std::path::Path,
        ) {
            let path = std::env::join_paths(private.iter().map(|path| path.to_path_buf()).chain(
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
            ))
            .unwrap_or_else(|error| panic!("build shadow test PATH: {error}"));
            let shell_command = format!(r#""{command}""#);
            let mut process = Command::new("cmd.exe");
            process.args(["/D", "/S", "/C"]);
            process.raw_arg(shell_command);
            let status = process
                .current_dir(workspace)
                .env("PATH", path)
                .env(super::HELPER_COMMAND_ENV, command_prefix)
                .env("PRIVATE_A_MARKER", marker_a)
                .env("PRIVATE_B_MARKER", marker_b)
                .env("DECOY_MARKER", decoy_marker)
                .status()
                .unwrap_or_else(|error| panic!("run bound helper command: {error}"));
            assert!(status.success(), "bound helper command failed: {status}");
        }

        fn marker_lines(path: &std::path::Path) -> usize {
            std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("read helper marker: {error}"))
                .lines()
                .count()
        }
    }

    #[test]
    fn hook_allows_only_one_exact_room_helper_command() {
        for command in [
            format!("{HELPER} help"),
            format!("{HELPER} read"),
            format!("{HELPER} media {ATTACHMENT_ID}"),
            format!("{HELPER} speak 'hello room'"),
            format!("{HELPER} speak-to agent-2 'your turn'"),
            format!("{HELPER} decline duplicate"),
            format!("{HELPER} roll '2d6+1'"),
            format!(r#"{HELPER} choose '["north","south"]'"#),
        ] {
            assert!(
                safe_room_command(&command, HELPER),
                "safe command rejected: {command}"
            );
        }
        for command in [
            format!("{HELPER} read && env"),
            format!("{HELPER} media ma_1111111111111111111111111111111Z"),
            format!("{HELPER} media {ATTACHMENT_ID} extra"),
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
    fn media_command_stages_one_private_file_and_reset_removes_it() {
        let (tool, payload) =
            super::command::helper_tool("media", [ATTACHMENT_ID.to_owned()].into_iter())
                .unwrap_or_else(|error| panic!("parse media helper command: {error}"));
        assert_eq!(tool, "read_attachment");
        assert_eq!(payload["attachment_id"], ATTACHMENT_ID);

        let root = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create media staging root: {error}"));
        reset_media_directory(root.path())
            .unwrap_or_else(|error| panic!("create private media directory: {error}"));
        let path = stage_attachment(
            root.path(),
            &ProviderAttachment {
                id: ATTACHMENT_ID.to_owned(),
                filename: "diagram.png".to_owned(),
                content_type: "image/png".to_owned(),
                size: 4,
                is_image: true,
                content: vec![1, 2, 3, 4],
            },
        )
        .unwrap_or_else(|error| panic!("stage exact media: {error}"));
        assert_eq!(
            std::fs::read(&path).unwrap_or_else(|error| panic!("read staged media: {error}")),
            [1, 2, 3, 4]
        );
        assert!(path.starts_with(root.path().join(super::MEDIA_DIRECTORY)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .unwrap_or_else(|error| panic!("read staged media mode: {error}"))
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }

        reset_media_directory(root.path())
            .unwrap_or_else(|error| panic!("reset private media directory: {error}"));
        assert!(!path.exists());
        #[cfg(unix)]
        {
            let media = root.path().join(super::MEDIA_DIRECTORY);
            std::fs::remove_dir(&media)
                .unwrap_or_else(|error| panic!("remove empty media directory: {error}"));
            let outside = root.path().join("outside");
            std::fs::create_dir(&outside)
                .unwrap_or_else(|error| panic!("create outside directory: {error}"));
            let marker = outside.join("keep");
            std::fs::write(&marker, b"owned elsewhere")
                .unwrap_or_else(|error| panic!("write outside marker: {error}"));
            std::os::unix::fs::symlink(&outside, &media)
                .unwrap_or_else(|error| panic!("create media symlink: {error}"));
            assert!(reset_media_directory(root.path()).is_err());
            assert!(marker.exists(), "media reset followed an unowned symlink");
        }
    }
}
