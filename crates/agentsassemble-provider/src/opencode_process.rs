use std::{path::Path, process::Stdio};

#[cfg(windows)]
use process_wrap::tokio::JobObject;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::{sync::oneshot, task::JoinHandle};

use crate::{
    filesystem::BoundExecutable,
    launch_cleanup,
    launch_error::DriverLaunchError,
    opencode_protocol::{spawn_error, stop_error},
    opencode_startup::{drain_output, observe_startup},
    process::sanitize_environment,
    room_portal::RoomPortal,
    runtime::DriverError,
};

pub(crate) async fn start(
    executable: &BoundExecutable,
    arguments: &[String],
    workspace: &Path,
    environment: &[(String, String)],
    ready_line: String,
    room_portal: &mut RoomPortal,
) -> Result<
    (
        Box<dyn ChildWrapper>,
        JoinHandle<()>,
        JoinHandle<()>,
        oneshot::Receiver<bool>,
    ),
    DriverLaunchError,
> {
    let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
        command
            .args(arguments)
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    sanitize_environment(command.command_mut());
    command.command_mut().envs(environment.iter().cloned());
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    command.wrap(JobObject);
    let Ok(mut child) = command.spawn() else {
        let failure = DriverLaunchError::safe(spawn_error());
        return Err(launch_cleanup::portal(room_portal, failure).await);
    };
    drop(command);
    let Some(stdout) = child.stdout().take() else {
        let process = stop(child.as_mut()).await;
        let failure = DriverLaunchError::safe(spawn_error());
        return Err(launch_cleanup::owned_and_portal(room_portal, process, failure).await);
    };
    let Some(stderr) = child.stderr().take() else {
        let process = stop(child.as_mut()).await;
        let failure = DriverLaunchError::safe(spawn_error());
        return Err(launch_cleanup::owned_and_portal(room_portal, process, failure).await);
    };
    let (stdout_task, startup) = observe_startup(stdout, ready_line);
    Ok((
        child,
        stdout_task,
        tokio::spawn(drain_output(stderr)),
        startup,
    ))
}

pub(crate) async fn stop(child: &mut dyn ChildWrapper) -> Result<(), DriverError> {
    crate::process::stop_child(child)
        .await
        .map_err(|_| stop_error())
}
