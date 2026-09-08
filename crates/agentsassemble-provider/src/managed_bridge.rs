//! Exact managed-child lifecycle. Durable runtime and cleanup authority stays in the parent.
use agentsassemble_domain::DurableAgentSession;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    credentials::ProviderCredentialStore,
    driver::DriverError,
    launch_error::DriverLaunchError,
    provider_factory::{DriverFactory, ProductionDriverFactory},
    runtime_lease::HeldRuntimeLease,
};

#[path = "managed_bridge_callbacks.rs"]
mod callbacks;
#[path = "managed_bridge_exchange.rs"]
mod exchange;
#[path = "managed_bridge_pipe.rs"]
mod pipe;
#[path = "managed_bridge_turn.rs"]
mod turn;
#[path = "managed_bridge_wire.rs"]
mod wire;
#[path = "managed_bridge_worker.rs"]
mod worker;
use wire::{Command, Event, Launch, Reader, Writer, protocol_error, read, write};

const WORKER_FLAG: &str = "--agentsassemble-managed-provider";

/// Runs only the explicitly selected private managed-provider child mode.
/// It has no listener, browser authority or public admission credential.
pub async fn run_managed_bridge_if_requested() -> Option<i32> {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(WORKER_FLAG)) {
        return None;
    }
    let result = run_socket().await;
    Some(i32::from(result.is_err()))
}

async fn run_socket() -> Result<(), DriverError> {
    // Duplicate with CLOEXEC and close fd 0 before any native child can inherit it.
    let socket =
        rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3).map_err(|_| protocol_error())?;
    nix::unistd::close(0).map_err(|_| protocol_error())?;
    let socket = std::os::unix::net::UnixStream::from(socket);
    socket.set_nonblocking(true).map_err(|_| protocol_error())?;
    let socket = tokio::net::UnixStream::from_std(socket).map_err(|_| protocol_error())?;
    let (input, output) = socket.into_split();
    run(input, output).await
}

async fn run<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    output: W,
) -> Result<(), DriverError> {
    let mut input = wire::reader(input);
    let mut output = wire::writer(output);
    let launch: Launch = read(&mut input).await?.ok_or_else(protocol_error)?;
    let Ok(runtime_lease) = HeldRuntimeLease::from_private_handoff(&launch.session) else {
        return write(
            &mut output,
            &Event::Ready {
                result: Err(DriverLaunchError::safe(DriverError::new(
                    "managed_bridge_authority_invalid",
                    "The managed provider launch authority is invalid.",
                ))),
            },
        )
        .await;
    };
    write(&mut output, &Event::Acquired).await?;
    let credentials = ProviderCredentialStore::from_private_handoff(launch.credential);
    #[cfg(test)]
    let mut factory = ProductionDriverFactory::local(credentials);
    #[cfg(not(test))]
    let mut factory = {
        let executable = crate::guardian::reexecution_path().map_err(|_| protocol_error())?;
        let mut factory = ProductionDriverFactory::with_guardian(&executable);
        factory.credentials = credentials;
        factory
    };
    factory.state_root = launch.state_root;
    run_launch(
        &mut input,
        &mut output,
        &launch.session,
        &runtime_lease,
        &factory,
    )
    .await
}

async fn run_launch<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: &mut Reader<R>,
    output: &mut Writer<W>,
    session: &DurableAgentSession,
    lease: &HeldRuntimeLease,
    factory: &dyn DriverFactory,
) -> Result<(), DriverError> {
    // A lost pipe does not abandon an in-flight native launch. Its owner first returns
    // the driver or its exact safe/uncertain launch failure, then performs cleanup.
    let launch = factory.launch(session, lease);
    tokio::pin!(launch);
    let (result, parent_gone) = tokio::select! {
        result = &mut launch => (result, false),
        _ = read::<_, Command>(input) => (launch.await, true),
    };
    let mut driver = match result {
        Ok(driver) => driver,
        Err(error) => {
            if parent_gone {
                return Err(error.error);
            }
            write(output, &Event::Ready { result: Err(error) }).await?;
            return Ok(());
        }
    };
    if parent_gone {
        return driver.stop().await.and(Err(protocol_error()));
    }
    let facts = wire::Facts::observe(driver.as_ref());
    let ready = async {
        write(output, &Event::Facts { facts }).await?;
        write(output, &Event::Ready { result: Ok(()) }).await
    }
    .await;
    if let Err(error) = ready {
        return driver.stop().await.and(Err(error));
    }
    let result = worker::serve(input, output, session, driver.as_mut()).await;
    match result {
        Ok(stopped) => stopped,
        Err(error) => driver.stop().await.and(Err(error)),
    }
}

fn same_runtime(left: &DurableAgentSession, right: &DurableAgentSession) -> bool {
    left.public.room_id == right.public.room_id
        && left.public.session_id == right.public.session_id
        && left.runtime_handle_id == right.runtime_handle_id
        && left.runtime_owner_id == right.runtime_owner_id
        && left.runtime_lease_token == right.runtime_lease_token
        && left.runtime_profile_key == right.runtime_profile_key
}

#[cfg(test)]
#[path = "managed_bridge_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "managed_bridge_process_tests.rs"]
mod process_tests;
