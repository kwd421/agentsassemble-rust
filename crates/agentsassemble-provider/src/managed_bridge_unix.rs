//! Unix inherited transport and the independent guardian's exact absence witness.
use agentsassemble_domain::DurableAgentSession;
use std::{future::Ready, sync::Arc};
use tokio::net::{
    UnixStream,
    unix::{OwnedReadHalf, OwnedWriteHalf},
};

use super::{Spawn, wire::protocol_error};
use crate::{driver::DriverError, provider_factory::ProductionDriverFactory};

pub(super) type Child = tokio::process::Child;
type Connection = Ready<Result<(OwnedReadHalf, OwnedWriteHalf), DriverError>>;

pub(super) struct RuntimeProof {
    room_id: String,
    session_id: String,
    handle_id: String,
    lease_token: String,
}

impl RuntimeProof {
    pub(super) fn is_gone(&self) -> bool {
        crate::runtime_absence::observation_proves_gone(
            &self.handle_id,
            &self.lease_token,
            &crate::runtime_lease::observe_runtime_lease(&self.room_id, &self.session_id),
            crate::runtime_absence::ObservationScope::LiveSlot,
        )
    }
}

pub(super) fn spawn(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
) -> Result<Spawn<Connection>, DriverError> {
    let (parent, child) = std::os::unix::net::UnixStream::pair().map_err(|_| protocol_error())?;
    parent.set_nonblocking(true).map_err(|_| protocol_error())?;
    let parent = UnixStream::from_std(parent).map_err(|_| protocol_error())?;
    let child = factory
        .guardian()?
        .managed_command(child.into())
        .map_err(|_| protocol_error())?
        .spawn()
        .map_err(|_| protocol_error())?;
    Ok(Spawn {
        child,
        connection: std::future::ready(Ok(parent.into_split())),
        proof: Arc::new(RuntimeProof {
            room_id: session.public.room_id.clone(),
            session_id: session.public.session_id.clone(),
            handle_id: session.runtime_handle_id.clone(),
            lease_token: session.runtime_lease_token.clone(),
        }),
    })
}

pub(super) async fn run_worker() -> Result<(), DriverError> {
    // Duplicate with CLOEXEC and close fd 0 before any native child can inherit it.
    let socket =
        rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3).map_err(|_| protocol_error())?;
    nix::unistd::close(0).map_err(|_| protocol_error())?;
    let socket = std::os::unix::net::UnixStream::from(socket);
    socket.set_nonblocking(true).map_err(|_| protocol_error())?;
    let socket = UnixStream::from_std(socket).map_err(|_| protocol_error())?;
    let (input, output) = socket.into_split();
    super::run(input, output).await
}

pub(super) fn exited_successfully(status: std::process::ExitStatus) -> bool {
    status.success()
}
