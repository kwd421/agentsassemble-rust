//! The built-in harness's one command tool: a shell command in the agent's
//! workspace, run only after the owner allows that exact command.
use std::time::Duration;

use agentsassemble_domain::DurableAgentSession;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    driver::{DriverError, ProviderTurnRequest},
    process::{ProbeFailure, run_in_directory},
    workspace_files,
    workspace_tools::owner_allows,
};

pub(crate) const NAME: &str = "run_workspace_command";
const MAX_COMMAND_CHARACTERS: usize = 4_000;
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TIMEOUT_SECONDS: u64 = 300;
/// Each stream the model sees; the whole run is still bounded by the process owner.
const MAX_STREAM_CHARACTERS: usize = 32 * 1024;

const FAILED: DriverError = DriverError::new(
    "workspace_command_failed",
    "The workspace command could not run.",
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    command: String,
    timeout_seconds: Option<u64>,
}

/// The shell this computer runs commands with, named the way the model should write them.
fn shell() -> (&'static str, &'static [&'static str], &'static str) {
    if cfg!(windows) {
        (
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command"],
            "Windows PowerShell",
        )
    } else if cfg!(target_os = "macos") {
        ("/bin/zsh", &["-c"], "zsh (macOS)")
    } else {
        ("/bin/sh", &["-c"], "sh")
    }
}

pub(crate) fn schema() -> Value {
    let (_, _, label) = shell();
    json!({"type":"function","function":{
        "name": NAME,
        "description": format!("Run one {label} command in the selected workspace after the owner approves it. Use it for builds, tests and inspection the file tools cannot do, and say in your message why it is needed."),
        "parameters":{"type":"object","properties":{
            "command":{"type":"string"},
            "timeout_seconds":{"type":"integer","minimum":1,"maximum":MAX_TIMEOUT_SECONDS}
        },"required":["command"],"additionalProperties":false}
    }})
}

/// Commands that erase a disk or a home folder are refused before the owner is
/// asked. This catches obvious accidents only; the owner's approval of the exact
/// command is the boundary.
fn obviously_destructive(command: &str) -> bool {
    const PATTERNS: [&str; 16] = [
        "rm -rf / ",
        "rm -rf /*",
        "rm -rf ~",
        "rm -rf $home",
        "rm -fr / ",
        "rm -fr ~",
        "mkfs",
        "diskutil erase",
        "diskutil partitiondisk",
        "dd of=/dev/",
        ":(){",
        "format c:",
        "format-volume",
        "clear-disk",
        "rd /s /q c:\\ ",
        "remove-item -recurse -force c:\\ ",
    ];
    let flat = command
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let padded = format!("{flat} ");
    PATTERNS.iter().any(|pattern| padded.contains(pattern))
}

fn bounded(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    let count = text.chars().count();
    if count <= MAX_STREAM_CHARACTERS {
        return (text.into_owned(), false);
    }
    // The end of a build or test log usually holds the failure; keep that part.
    (
        text.chars().skip(count - MAX_STREAM_CHARACTERS).collect(),
        true,
    )
}

fn refused(code: &str, reason: &str) -> String {
    json!({"error": code, "reason": reason}).to_string()
}

pub(crate) async fn execute(
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
    arguments: &str,
    effect_uncertain: &mut bool,
    cancellation: &CancellationToken,
) -> Result<String, DriverError> {
    if session.public.permission_mode != "workspace_write" {
        return Err(FAILED);
    }
    let arguments: Arguments = serde_json::from_str(arguments).map_err(|_| FAILED)?;
    let command = arguments.command.trim();
    if command.is_empty()
        || command.chars().count() > MAX_COMMAND_CHARACTERS
        || command
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Ok(refused(
            "command_invalid",
            "The command is empty, too long or has control characters.",
        ));
    }
    if obviously_destructive(command) {
        return Ok(refused(
            "command_refused",
            "This command could erase a disk or a home folder and is never run.",
        ));
    }
    let timeout = arguments
        .timeout_seconds
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
        .clamp(1, MAX_TIMEOUT_SECONDS);
    // The stored identity proves this is still the folder the owner chose.
    let workspace = session.workspace.clone();
    let identity = session.workspace_identity.clone();
    tokio::task::spawn_blocking(move || workspace_files::bind(&workspace, &identity).map(drop))
        .await
        .map_err(|_| FAILED)?
        .map_err(|_| FAILED)?;
    let description = format!("작업 폴더에서 이 명령을 실행합니다.\n\n{command}");
    if !owner_allows(session, request, "명령 실행", description).await? {
        return Ok(refused(
            "command_denied",
            "The owner did not allow this command.",
        ));
    }
    // An allowed command may change files before its result is observed.
    *effect_uncertain = true;
    let (program, flags, _) = shell();
    let mut args = flags.to_vec();
    args.push(command);
    let output = match run_in_directory(
        program,
        &args,
        std::path::Path::new(&session.workspace),
        Duration::from_secs(timeout),
        cancellation,
    )
    .await
    {
        Ok(output) => output,
        Err(ProbeFailure::Timeout) => {
            return Ok(
                json!({"error": "command_timed_out", "timeout_seconds": timeout}).to_string(),
            );
        }
        Err(ProbeFailure::CatalogTooLarge) => {
            return Ok(refused(
                "command_output_too_large",
                "The command printed more than 2 MB.",
            ));
        }
        Err(ProbeFailure::Missing) => {
            return Ok(refused(
                "command_shell_missing",
                "The shell was not found on this computer.",
            ));
        }
        Err(_) => return Err(FAILED),
    };
    let (stdout, stdout_truncated) = bounded(&output.stdout);
    let (stderr, stderr_truncated) = bounded(&output.stderr);
    Ok(json!({
        "exit_code": output.status.code(),
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
    })
    .to_string())
}

#[cfg(test)]
#[path = "workspace_command_tests.rs"]
mod tests;
