use std::path::Path;

use tokio::io::unix::AsyncFd;

use crate::{
    antigravity_transport::AntigravityTerminal,
    filesystem::BoundExecutable,
    guardian::GuardianLaunch,
    launch_error::DriverLaunchError,
    runtime::{DriverError, DriverFuture},
    runtime_lease::HeldRuntimeLease,
    unix_custody::UnixProcessCustody,
};

pub(crate) async fn spawn_terminal(
    runtime_lease: &HeldRuntimeLease,
    guardian: &GuardianLaunch,
    executable: BoundExecutable,
    arguments: &[String],
    environment: &[(String, String)],
    workspace: &Path,
) -> Result<Box<dyn AntigravityTerminal>, DriverLaunchError> {
    let (process_group, pty) = UnixProcessCustody::start_pty(
        runtime_lease,
        guardian,
        &executable,
        arguments,
        environment,
        workspace,
    )
    .await?;
    Ok(Box::new(UnixAntigravityTerminal {
        process_group,
        terminal: pty.terminal,
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        _executable_guard: executable,
    }))
}

struct UnixAntigravityTerminal {
    process_group: UnixProcessCustody,
    terminal: AsyncFd<std::os::fd::OwnedFd>,
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    _executable_guard: BoundExecutable,
}

impl AntigravityTerminal for UnixAntigravityTerminal {
    fn read(&mut self) -> DriverFuture<'_, Result<Vec<u8>, DriverError>> {
        Box::pin(async move {
            loop {
                let mut ready = self
                    .terminal
                    .readable()
                    .await
                    .map_err(|_| terminal_error())?;
                let mut buffer = vec![0_u8; 16 * 1024];
                match ready.try_io(|inner| {
                    rustix::io::read(inner.get_ref(), &mut buffer).map_err(std::io::Error::from)
                }) {
                    Ok(Ok(0)) => return Err(runtime_exited()),
                    Ok(Ok(count)) => {
                        buffer.truncate(count);
                        return Ok(buffer);
                    }
                    Ok(Err(_)) => return Err(terminal_error()),
                    Err(_) => {}
                }
            }
        })
    }

    fn write<'a>(&'a mut self, mut data: &'a [u8]) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            while !data.is_empty() {
                let mut ready = self
                    .terminal
                    .writable()
                    .await
                    .map_err(|_| terminal_error())?;
                match ready.try_io(|inner| {
                    rustix::io::write(inner.get_ref(), data).map_err(std::io::Error::from)
                }) {
                    Ok(Ok(0) | Err(_)) => return Err(terminal_error()),
                    Ok(Ok(count)) => data = &data[count..],
                    Err(_) => {}
                }
            }
            Ok(())
        })
    }

    fn is_alive(&mut self) -> Result<bool, DriverError> {
        self.process_group.leader_is_running()
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.process_group.stop())
    }

    fn request_stop(&mut self) {
        self.process_group.request_stop();
    }
}

impl Drop for UnixAntigravityTerminal {
    fn drop(&mut self) {
        self.process_group.request_stop();
    }
}

const fn runtime_exited() -> DriverError {
    DriverError::new(
        "provider_runtime_exited",
        "The Antigravity interactive session exited unexpectedly.",
    )
}

const fn terminal_error() -> DriverError {
    DriverError::new(
        "provider_transport_failed",
        "The Antigravity terminal transport failed.",
    )
}
