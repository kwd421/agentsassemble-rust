use std::{process::Stdio, time::Duration};

use futures_util::StreamExt;
use process_wrap::tokio::ChildWrapper;
use rustix::process::{Pid, WaitId, WaitIdOptions};
use tokio::{io::AsyncWriteExt, process::ChildStdin};
use tokio_util::codec::{FramedRead, LinesCodec};

use crate::{
    guardian::GuardianLaunch, runtime::DriverError, runtime_lease::HeldRuntimeLease,
    unix_process_tree::CapturedDescendants,
};

const HELPER_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HELPER_LINE_BYTES: usize = 1024;
const MAX_HELPER_LINES: usize = 32;
const READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_ANCHOR=";

pub(crate) struct UnixProcessCustody {
    anchor_pid: Pid,
    provider_pid: Option<Pid>,
    guardian: tokio::process::Child,
    guardian_input: Option<ChildStdin>,
    armed: bool,
}

impl UnixProcessCustody {
    pub(crate) async fn start(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
    ) -> Result<Self, DriverError> {
        let mut command = launch
            .guardian_command(runtime_lease.path(), runtime_lease.token())
            .map_err(|_| custody_error())?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut guardian) = command.spawn() else {
            return Err(custody_error());
        };
        let Some(guardian_input) = guardian.stdin.take() else {
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        };
        let Some(guardian_output) = guardian.stdout.take() else {
            drop(guardian_input);
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        };
        let Ok(Ok(anchor_pid)) = tokio::time::timeout(
            HELPER_TIMEOUT,
            read_ready(FramedRead::new(
                guardian_output,
                LinesCodec::new_with_max_length(MAX_HELPER_LINE_BYTES),
            )),
        )
        .await
        else {
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        };
        Ok(Self {
            anchor_pid,
            provider_pid: None,
            guardian,
            guardian_input: Some(guardian_input),
            armed: true,
        })
    }

    pub(crate) fn attach(&self, command: &mut tokio::process::Command) {
        command.process_group(self.anchor_pid.as_raw_pid());
    }

    pub(crate) fn bind_provider(&mut self, raw_pid: Option<u32>) -> Result<(), DriverError> {
        let pid = raw_pid
            .and_then(|value| i32::try_from(value).ok())
            .and_then(Pid::from_raw)
            .ok_or_else(custody_error)?;
        if rustix::process::getpgid(Some(pid)).map_err(|_| custody_error())? != self.anchor_pid {
            return Err(custody_error());
        }
        self.provider_pid = Some(pid);
        Ok(())
    }

    pub(crate) fn leader_is_running(&self) -> Result<bool, DriverError> {
        let pid = self.provider_pid.ok_or_else(custody_error)?;
        rustix::process::waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|status| status.is_none())
        .map_err(|_| {
            DriverError::new(
                "provider_health_unknown",
                "The Codex app-server leader state could not be observed.",
            )
        })
    }

    pub(crate) async fn stop(&mut self, child: &mut dyn ChildWrapper) -> Result<(), DriverError> {
        let provider_pid = self.provider_pid.ok_or_else(custody_error)?;
        let descendants = CapturedDescendants::freeze(provider_pid, self.anchor_pid);
        self.request_stop();
        descendants.kill();
        tokio::time::timeout(HELPER_TIMEOUT, async {
            let (guardian, provider) = tokio::join!(self.guardian.wait(), child.wait());
            let guardian = guardian.map_err(|_| stop_error())?;
            provider.map_err(|_| stop_error())?;
            if !guardian.success() {
                return Err(stop_error());
            }
            wait_for_group_absence(self.anchor_pid).await?;
            descendants.confirm_gone().await.map_err(|_| stop_error())
        })
        .await
        .map_err(|_| stop_error())??;
        self.armed = false;
        Ok(())
    }

    pub(crate) fn request_stop(&mut self) {
        if self.armed {
            drop(self.guardian_input.take());
        }
    }
}

async fn wait_for_group_absence(process_group: Pid) -> Result<(), DriverError> {
    loop {
        match rustix::process::test_kill_process_group(process_group) {
            Err(rustix::io::Errno::SRCH) => return Ok(()),
            Ok(()) | Err(rustix::io::Errno::PERM) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(_) => return Err(stop_error()),
        }
    }
}

impl Drop for UnixProcessCustody {
    fn drop(&mut self) {
        self.request_stop();
    }
}

async fn read_ready(
    mut lines: FramedRead<tokio::process::ChildStdout, LinesCodec>,
) -> Result<Pid, DriverError> {
    for _ in 0..MAX_HELPER_LINES {
        let line = lines
            .next()
            .await
            .ok_or_else(custody_error)?
            .map_err(|_| custody_error())?;
        if let Some(value) = line.strip_prefix(READY_PREFIX) {
            return value
                .parse::<i32>()
                .ok()
                .and_then(Pid::from_raw)
                .ok_or_else(custody_error);
        }
    }
    Err(custody_error())
}

async fn terminate_failed_guardian(guardian: &mut tokio::process::Child) {
    if let Some(mut input) = guardian.stdin.take() {
        let _ = input.shutdown().await;
    }
    let _ = guardian.kill().await;
    let _ = guardian.wait().await;
}

const fn custody_error() -> DriverError {
    DriverError::new(
        "provider_custody_unavailable",
        "The provider process custody helper could not be established.",
    )
}

const fn stop_error() -> DriverError {
    DriverError::new(
        "provider_stop_unconfirmed",
        "The Codex app-server process tree shutdown could not be confirmed.",
    )
}
