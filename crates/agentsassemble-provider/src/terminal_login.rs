#[cfg(any(target_os = "macos", windows, test))]
use std::time::Duration;
use std::{io, process::Stdio};

#[cfg(any(target_os = "macos", windows, test))]
use tokio::process::Child;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::{ProviderLoginError, catalog::provider_executable, process::sanitize_environment};

pub(crate) async fn launch(
    executable: &str,
    arguments: &[&str],
    cancellation: &CancellationToken,
) -> Result<(), ProviderLoginError> {
    if cancellation.is_cancelled() {
        return Err(ProviderLoginError::Cancelled);
    }
    #[cfg(target_os = "macos")]
    {
        let (launcher, _) = provider_executable("osascript", cancellation)
            .await
            .map_err(super::provider_login::login_failure)?;
        let shell_command =
            shlex::try_join(std::iter::once(executable).chain(arguments.iter().copied()))
                .map_err(|_| ProviderLoginError::Failed)?;
        let mut command = private_command(&launcher);
        command.args(["-e", MACOS_SCRIPT, "--", &shell_command]);
        return run_launcher(command, cancellation).await;
    }
    #[cfg(windows)]
    {
        let (launcher, _) = provider_executable("powershell.exe", cancellation)
            .await
            .map_err(super::provider_login::login_failure)?;
        let mut command = private_command(&launcher);
        command.args(["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT]);
        command.env("AGENTSASSEMBLE_LOGIN_EXECUTABLE", executable);
        command.env(
            "AGENTSASSEMBLE_LOGIN_ARGUMENTS",
            serde_json::to_string(arguments).map_err(|_| ProviderLoginError::Failed)?,
        );
        return run_launcher(command, cancellation).await;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        launch_linux(executable, arguments, cancellation).await
    }
    #[cfg(not(any(unix, windows)))]
    Err(ProviderLoginError::Unsupported)
}

#[cfg(target_os = "macos")]
const MACOS_SCRIPT: &str = "on run argv\n tell application id \"com.apple.Terminal\"\n activate\n do script (item 1 of argv)\n end tell\nend run";

#[cfg(windows)]
const WINDOWS_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; Start-Process -FilePath $env:AGENTSASSEMBLE_LOGIN_EXECUTABLE -ArgumentList @(ConvertFrom-Json $env:AGENTSASSEMBLE_LOGIN_ARGUMENTS) -WindowStyle Normal";

fn private_command(program: &str) -> Command {
    let mut command = Command::new(program);
    sanitize_environment(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(any(target_os = "macos", windows, test))]
async fn run_launcher(
    mut command: Command,
    cancellation: &CancellationToken,
) -> Result<(), ProviderLoginError> {
    if cancellation.is_cancelled() {
        return Err(ProviderLoginError::Cancelled);
    }
    // A launcher receipt owns only this helper. Its new terminal is operator-owned.
    command.kill_on_drop(true);
    let child = command.spawn().map_err(|error| spawn_error(&error))?;
    wait_launcher(child, cancellation).await
}

#[cfg(any(target_os = "macos", windows, test))]
async fn wait_launcher(
    mut child: Child,
    cancellation: &CancellationToken,
) -> Result<(), ProviderLoginError> {
    let outcome = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(10), child.wait()) => Some(result),
        () = cancellation.cancelled() => None,
    };
    if matches!(outcome, Some(Ok(Ok(status))) if status.success()) {
        return Ok(());
    }
    if !matches!(outcome, Some(Ok(Ok(_)))) {
        match tokio::time::timeout(Duration::from_secs(5), child.kill()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => return Err(ProviderLoginError::CleanupUnconfirmed),
        }
    }
    Err(ProviderLoginError::HandoffUnconfirmed)
}

fn spawn_error(error: &io::Error) -> ProviderLoginError {
    if error.kind() == io::ErrorKind::NotFound {
        ProviderLoginError::Missing
    } else {
        ProviderLoginError::Failed
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn launch_linux(
    executable: &str,
    arguments: &[&str],
    cancellation: &CancellationToken,
) -> Result<(), ProviderLoginError> {
    for (terminal, separator) in [
        ("x-terminal-emulator", "-e"),
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
        ("xfce4-terminal", "-x"),
    ] {
        let launcher = match provider_executable(terminal, cancellation).await {
            Ok((path, _)) => path,
            Err(crate::process::ProbeFailure::Missing) => continue,
            Err(error) => return Err(super::provider_login::login_failure(error)),
        };
        if cancellation.is_cancelled() {
            return Err(ProviderLoginError::Cancelled);
        }
        let mut command = private_command(&launcher);
        for name in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "XAUTHORITY",
            "XDG_CURRENT_DESKTOP",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .arg(separator)
            .arg(executable)
            .args(arguments)
            .process_group(0)
            .kill_on_drop(false);
        let terminal = command.spawn().map_err(|error| spawn_error(&error))?;
        // Native exec succeeded: custody transfers to the operator's graphical terminal.
        // Tokio reaps completed children; the app never kills a handed-off terminal.
        drop(terminal);
        return Ok(());
    }
    Err(ProviderLoginError::Missing)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn launcher_receipt_is_distinct_from_authentication_and_cancel_reaps_only_the_helper() {
        assert_eq!(
            run_launcher(private_command("/usr/bin/true"), &CancellationToken::new()).await,
            Ok(())
        );
        assert_eq!(
            run_launcher(private_command("/usr/bin/false"), &CancellationToken::new()).await,
            Err(ProviderLoginError::HandoffUnconfirmed)
        );
        let mut command = private_command("/bin/sh");
        command.args(["-c", "exec sleep 30"]).kill_on_drop(true);
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("fixture launcher: {error}"));
        let pid = child.id().unwrap_or_else(|| panic!("fixture launcher PID"));
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            wait_launcher(child, &cancellation).await,
            Err(ProviderLoginError::HandoffUnconfirmed)
        );
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap_or_else(|error| panic!("helper absence: {error}"));
        assert!(!status.success());
    }
}
