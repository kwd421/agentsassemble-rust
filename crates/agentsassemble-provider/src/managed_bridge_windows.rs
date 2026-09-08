//! PID-authenticated private pipes and one lease-owned Windows Job per runtime.
use std::{future::Future, sync::Arc};

use agentsassemble_domain::DurableAgentSession;
use interprocess::os::windows::named_pipe::{
    PipeListenerOptions,
    pipe_mode::Bytes,
    tokio::{DuplexPipeStream, RecvPipeStream, SendPipeStream},
};

use super::{Spawn, wire::protocol_error};
use crate::{
    driver::DriverError, launch_error::DriverLaunchError,
    provider_factory::ProductionDriverFactory, runtime_lease::HeldRuntimeLease,
};

pub(super) use crate::runtime_lease::WindowsRuntimeCustody as RuntimeProof;
type Connection = Result<(RecvPipeStream<Bytes>, SendPipeStream<Bytes>), DriverError>;

// Armed before the library launch future: cancellation cannot abandon the Job.
// The runtime slot keeps another Arc for positive cleanup observation afterwards.
pub(super) struct Child {
    process: Option<processkit::RunningProcess>,
    proof: Arc<RuntimeProof>,
}

impl Drop for Child {
    fn drop(&mut self) {
        // A request is never absence proof. Failures remain visible to the lease probe.
        let _ = self.proof.group().kill_all();
    }
}

impl Child {
    pub(super) async fn wait(&mut self) -> Result<processkit::Outcome, DriverError> {
        let process = self.process.as_mut().ok_or_else(protocol_error)?;
        processkit::wait_any(&mut [process])
            .await
            .map(|(_, outcome)| outcome)
            .map_err(|_| protocol_error())
    }

    pub(super) async fn kill(&mut self) -> Result<(), DriverError> {
        self.proof
            .group()
            .kill_all()
            .map_err(|_| protocol_error())?;
        self.wait().await.map(|_| ())
    }
}

pub(super) fn exited_successfully(status: processkit::Outcome) -> bool {
    status.code() == Some(0)
}

pub(super) async fn spawn(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    lease: &HeldRuntimeLease,
) -> Result<Spawn<impl Future<Output = Connection>>, DriverLaunchError> {
    if session.runtime_lease_token != lease.token()
        || session.runtime_handle_id != lease.new_runtime_handle_id()
    {
        return Err(protocol_error().into());
    }
    let proof = lease
        .windows_custody()
        .map_err(|_| protocol_error())?
        .clone();
    let endpoint = format!(r"\\.\pipe\agentsassemble-managed-{}", uuid::Uuid::new_v4());
    let listener = PipeListenerOptions::new()
        .path(endpoint.clone())
        .accept_remote(false)
        .inheritable(false)
        .create_tokio_duplex::<Bytes>()
        .map_err(|_| protocol_error())?;
    let executable = factory.worker()?;
    let command = worker_command(executable.launch_path(), &endpoint);
    let mut child = Child {
        process: None,
        proof: proof.clone(),
    };
    // Command-driven start preserves CREATE_NO_WINDOW together with suspended,
    // assigned-to-Job spawn. Null output and no watchdogs add no pump/poll tasks.
    child.process = Some(
        proof
            .group()
            .start(&command)
            .await
            .map_err(|_| DriverLaunchError::uncertain(protocol_error()))?,
    );
    let pid = child
        .process
        .as_ref()
        .and_then(processkit::RunningProcess::pid)
        .ok_or_else(|| DriverLaunchError::uncertain(protocol_error()))?;
    let connection = async move {
        let stream = listener.accept().await.map_err(|_| protocol_error())?;
        if stream.peer_process_id().map_err(|_| protocol_error())? != pid {
            return Err(protocol_error());
        }
        Ok(stream.split())
    };
    Ok(Spawn {
        child,
        connection,
        proof,
    })
}

fn worker_command(executable: &str, endpoint: &str) -> processkit::Command {
    let command = processkit::Command::new(executable)
        .create_no_window()
        .env_clear()
        .envs(super::environment())
        .stdout(processkit::StdioMode::Null)
        .stderr(processkit::StdioMode::Null);
    #[cfg(test)]
    let command = command
        .args([
            "--exact",
            "managed_bridge::windows_tests::worker_entry",
            "--nocapture",
        ])
        .env("AGENTSASSEMBLE_TEST_MANAGED_WORKER", "1")
        .env("AGENTSASSEMBLE_TEST_MANAGED_PIPE", endpoint)
        .env(
            "AGENTSASSEMBLE_TEST_MANAGED_PARENT",
            std::process::id().to_string(),
        );
    #[cfg(not(test))]
    let command = command.args([
        super::WORKER_FLAG,
        endpoint,
        &std::process::id().to_string(),
    ]);
    command
}

pub(super) async fn run_worker() -> Result<(), DriverError> {
    #[cfg(not(test))]
    let (endpoint, parent) = (
        std::env::args_os().nth(2).ok_or_else(protocol_error)?,
        std::env::args().nth(3).ok_or_else(protocol_error)?,
    );
    #[cfg(test)]
    let (endpoint, parent) = (
        std::env::var_os("AGENTSASSEMBLE_TEST_MANAGED_PIPE").ok_or_else(protocol_error)?,
        std::env::var("AGENTSASSEMBLE_TEST_MANAGED_PARENT").map_err(|_| protocol_error())?,
    );
    let parent: u32 = parent.parse().map_err(|_| protocol_error())?;
    let stream = DuplexPipeStream::<Bytes>::connect_by_path_with_wait_mode(
        endpoint,
        interprocess::ConnectWaitMode::Timeout(std::time::Duration::ZERO),
    )
    .await
    .map_err(|_| protocol_error())?;
    if stream.peer_process_id().map_err(|_| protocol_error())? != parent {
        return Err(protocol_error());
    }
    let (input, output) = stream.split();
    super::run(input, output).await
}
