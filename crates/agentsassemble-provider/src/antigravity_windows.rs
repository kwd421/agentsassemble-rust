use std::path::Path;

use conpty_oxide::tokio::{Child, Command, OwnedReadHalf, OwnedWriteHalf};
use conpty_oxide::{ConPtyBackend, ErrorKind, SessionOptions, Size};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    antigravity_transport::AntigravityTerminal,
    filesystem::BoundExecutable,
    launch_error::DriverLaunchError,
    process::sanitized_environment,
    runtime::{DriverError, DriverFuture},
};

pub(crate) fn spawn_terminal(
    executable: BoundExecutable,
    arguments: &[String],
    environment: &[(String, String)],
    workspace: &Path,
) -> Result<Box<dyn AntigravityTerminal>, DriverLaunchError> {
    let backend = ConPtyBackend::system().map_err(|_| custody_error())?;
    let size = Size::try_new(120, 40).map_err(|_| custody_error())?;
    let mut command = Command::new(executable.launch_path());
    command
        .args(arguments)
        .current_dir(workspace)
        .env_clear()
        .envs(sanitized_environment())
        .envs(environment.iter().cloned());
    let session = command
        .spawn_with(SessionOptions::new().size(size).backend(backend))
        .map_err(classify_spawn_error)?;
    let parts = session.into_parts();
    Ok(Box::new(WindowsAntigravityTerminal {
        child: Some(parts.child),
        output: parts.output,
        input: parts.input,
        _controller: parts.controller,
        _executable_guard: executable,
    }))
}

struct WindowsAntigravityTerminal {
    child: Option<Child>,
    output: OwnedReadHalf,
    input: OwnedWriteHalf,
    _controller: conpty_oxide::PtyController,
    _executable_guard: BoundExecutable,
}

impl AntigravityTerminal for WindowsAntigravityTerminal {
    fn read(&mut self) -> DriverFuture<'_, Result<Vec<u8>, DriverError>> {
        Box::pin(async move {
            let mut buffer = vec![0_u8; 16 * 1024];
            let count = self
                .output
                .read(&mut buffer)
                .await
                .map_err(|_| terminal_error())?;
            if count == 0 {
                return Err(runtime_exited());
            }
            buffer.truncate(count);
            Ok(buffer)
        })
    }

    fn write<'a>(&'a mut self, data: &'a [u8]) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            self.input
                .write_all(data)
                .await
                .map_err(|_| terminal_error())?;
            self.input.flush().await.map_err(|_| terminal_error())
        })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async move {
            let child = self.child.as_mut().ok_or_else(runtime_exited)?;
            child
                .try_wait()
                .map(|status| status.is_none())
                .map_err(|_| terminal_error())
        })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            let Some(mut child) = self.child.take() else {
                return Ok(());
            };
            child.kill().map_err(|_| terminal_error())?;
            child.wait().await.map_err(|_| terminal_error())?;
            Ok(())
        })
    }

    fn request_stop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
    }
}

impl Drop for WindowsAntigravityTerminal {
    fn drop(&mut self) {
        self.request_stop();
    }
}

fn classify_spawn_error(error: conpty_oxide::Error) -> DriverLaunchError {
    if error.kind() == ErrorKind::Spawn {
        DriverLaunchError::safe(DriverError::new(
            "provider_spawn_failed",
            "The Antigravity executable could not be launched.",
        ))
    } else {
        custody_error()
    }
}

const fn custody_error() -> DriverLaunchError {
    DriverLaunchError::safe(DriverError::new(
        "provider_custody_unavailable",
        "The Windows ConPTY process custody boundary is unavailable.",
    ))
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

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Write};
    use std::time::Duration;

    use super::spawn_terminal;
    use crate::filesystem::{bind_executable, executable_identity};

    const FIXTURE_ENVIRONMENT: &str = "AGENTSASSEMBLE_CONPTY_TEST_FIXTURE";

    #[test]
    fn conpty_fixture() {
        if std::env::var_os(FIXTURE_ENVIRONMENT).is_none() {
            return;
        }
        let mut output = std::io::stdout().lock();
        writeln!(output, "AA-CONPTY-READY").expect("write ConPTY fixture readiness");
        output.flush().expect("flush ConPTY fixture readiness");
        for line in std::io::stdin().lock().lines() {
            let line = line.expect("read ConPTY fixture input");
            if line.trim_end() == "quit" {
                writeln!(output, "AA-CONPTY-BYE").expect("write ConPTY fixture exit");
                output.flush().expect("flush ConPTY fixture exit");
                return;
            }
            writeln!(output, "AA-CONPTY-ECHO:{}", line.trim_end())
                .expect("write ConPTY fixture echo");
            output.flush().expect("flush ConPTY fixture echo");
        }
    }

    #[tokio::test]
    async fn managed_conpty_is_one_resident_bidirectional_terminal() {
        let executable = std::env::current_exe()
            .and_then(|path| path.canonicalize())
            .expect("resolve test executable");
        let encoded = executable
            .to_str()
            .expect("test executable path is Unicode")
            .to_owned();
        let identity = executable_identity(encoded.clone())
            .await
            .expect("hash test executable");
        let executable = bind_executable(encoded, identity)
            .await
            .expect("bind test executable");
        let workspace = tempfile::tempdir().expect("create ConPTY workspace");
        let arguments = vec![
            "--exact".to_owned(),
            "antigravity_windows::tests::conpty_fixture".to_owned(),
            "--nocapture".to_owned(),
        ];
        let environment = vec![(FIXTURE_ENVIRONMENT.to_owned(), "v1".to_owned())];
        let mut terminal = spawn_terminal(executable, &arguments, &environment, workspace.path())
            .expect("spawn managed ConPTY fixture");

        read_until(&mut *terminal, b"AA-CONPTY-READY").await;
        terminal.write(b"first\r").await.expect("write first turn");
        read_until(&mut *terminal, b"AA-CONPTY-ECHO:first").await;
        assert!(terminal.is_alive().await.expect("probe resident fixture"));
        terminal
            .write(b"second\r")
            .await
            .expect("write second turn");
        read_until(&mut *terminal, b"AA-CONPTY-ECHO:second").await;
        terminal.write(b"quit\r").await.expect("write fixture exit");
        read_until(&mut *terminal, b"AA-CONPTY-BYE").await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while terminal.is_alive().await.expect("probe fixture exit") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("ConPTY fixture did not exit");
        terminal
            .stop()
            .await
            .expect("retire completed ConPTY fixture");
    }

    async fn read_until(
        terminal: &mut dyn crate::antigravity_transport::AntigravityTerminal,
        marker: &[u8],
    ) {
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut observed = Vec::new();
            while !observed
                .windows(marker.len())
                .any(|window| window == marker)
            {
                observed.extend(terminal.read().await.expect("read managed ConPTY output"));
                assert!(observed.len() <= 256 * 1024, "ConPTY fixture output grew");
            }
        })
        .await
        .expect("ConPTY fixture marker timed out");
    }
}
