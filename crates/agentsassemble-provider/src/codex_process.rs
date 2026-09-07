use std::{io, path::Path, process::Stdio};

#[cfg(windows)]
use process_wrap::tokio::JobObject;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use crate::{
    filesystem::BoundExecutable, launch_cleanup, launch_error::DriverLaunchError,
    process::sanitize_environment, room_portal::RoomPortal, runtime::DriverError,
};

pub(crate) async fn start(
    executable: &BoundExecutable,
    arguments: &[String],
    workspace: &Path,
    room_portal: &mut RoomPortal,
    codex_home: &str,
) -> Result<(Box<dyn ChildWrapper>, ChildStdin, ChildStdout, ChildStderr), DriverLaunchError> {
    let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
        command
            .args(arguments)
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    sanitize_environment(command.command_mut());
    command.command_mut().env("CODEX_HOME", codex_home);
    room_portal.configure_environment(command.command_mut());
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    command.wrap(JobObject);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let failure = DriverLaunchError::safe(spawn_error(&error));
            return Err(launch_cleanup::portal(room_portal, failure).await);
        }
    };
    drop(command);
    let Some(stdin) = child.stdin().take() else {
        return Err(failed_pipe(child.as_mut(), room_portal).await);
    };
    let Some(stdout) = child.stdout().take() else {
        return Err(failed_pipe(child.as_mut(), room_portal).await);
    };
    let Some(stderr) = child.stderr().take() else {
        return Err(failed_pipe(child.as_mut(), room_portal).await);
    };
    Ok((child, stdin, stdout, stderr))
}

pub(crate) async fn stop(child: &mut dyn ChildWrapper) -> Result<(), DriverError> {
    match crate::process::stop_child(child).await {
        Ok(()) => Ok(()),
        Err(crate::process::ChildStopFailure::Deadline) => Err(DriverError::new(
            "provider_stop_unconfirmed",
            "The Codex app-server exceeded its shutdown deadline.",
        )),
        Err(crate::process::ChildStopFailure::Unconfirmed) => Err(DriverError::new(
            "provider_stop_unconfirmed",
            "The Codex app-server shutdown could not be confirmed.",
        )),
    }
}

async fn failed_pipe(
    child: &mut dyn ChildWrapper,
    room_portal: &mut RoomPortal,
) -> DriverLaunchError {
    let process = stop(child).await;
    let failure = DriverLaunchError::safe(DriverError::new(
        "provider_protocol_invalid",
        "The Codex app-server returned an invalid protocol message.",
    ));
    launch_cleanup::owned_and_portal(room_portal, process, failure).await
}

fn spawn_error(error: &io::Error) -> DriverError {
    if error.kind() == io::ErrorKind::NotFound {
        DriverError::new(
            "provider_executable_missing",
            "The Codex executable is no longer available.",
        )
    } else {
        DriverError::new(
            "provider_spawn_failed",
            "The Codex app-server process could not be started.",
        )
    }
}
