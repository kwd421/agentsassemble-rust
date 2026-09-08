//! Exact managed-child lifecycle. Durable runtime and cleanup authority stays in the parent.
use agentsassemble_domain::DurableAgentSession;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    credentials::ProviderCredentialStore, driver::DriverError, launch_error::DriverLaunchError,
    provider_factory::ProductionDriverFactory, runtime_lease::HeldRuntimeLease,
};

#[path = "managed_bridge_callbacks.rs"]
mod callbacks;
#[path = "managed_bridge_exchange.rs"]
mod exchange;
#[path = "managed_bridge_parent.rs"]
mod parent;
#[path = "managed_bridge_parent_actor.rs"]
mod parent_actor;
#[path = "managed_bridge_parent_callbacks.rs"]
mod parent_callbacks;
#[path = "managed_bridge_parent_process.rs"]
mod parent_process;
#[path = "managed_bridge_pipe.rs"]
mod pipe;
#[cfg(unix)]
#[path = "managed_bridge_unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "managed_bridge_windows.rs"]
mod platform;
#[path = "managed_bridge_turn.rs"]
mod turn;
#[path = "managed_bridge_wire.rs"]
mod wire;
#[path = "managed_bridge_worker.rs"]
mod worker;
use wire::{Command, Event, Launch, Reader, Writer, protocol_error, read, write};

const CONTROL_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(1);

const WORKER_FLAG: &str = "--agentsassemble-managed-provider";

struct Spawn<C> {
    child: platform::Child,
    connection: C,
    proof: std::sync::Arc<platform::RuntimeProof>,
}

/// Runs only the explicitly selected private managed-provider child mode.
/// It has no listener, browser authority or public admission credential.
pub async fn run_managed_bridge_if_requested() -> Option<i32> {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(WORKER_FLAG)) {
        return None;
    }
    let result = platform::run_worker().await;
    Some(i32::from(result.is_err()))
}

async fn run<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    output: W,
) -> Result<(), DriverError> {
    let mut input = wire::reader(input);
    let mut output = wire::writer(output);
    let launch: Launch = read(&mut input).await?.ok_or_else(protocol_error)?;
    #[cfg(unix)]
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
    #[cfg(any(test, windows))]
    let mut factory = ProductionDriverFactory::local(credentials);
    #[cfg(all(not(test), unix))]
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
        #[cfg(unix)]
        &runtime_lease,
        &factory,
    )
    .await
}

async fn run_launch<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: &mut Reader<R>,
    output: &mut Writer<W>,
    session: &DurableAgentSession,
    #[cfg(unix)] lease: &HeldRuntimeLease,
    factory: &ProductionDriverFactory,
) -> Result<(), DriverError> {
    // A lost pipe does not abandon an in-flight native launch. Its owner first returns
    // the driver or its exact safe/uncertain launch failure, then performs cleanup.
    let launch = factory.launch_native(
        session,
        #[cfg(unix)]
        lease,
    );
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

#[cfg(all(test, unix))]
#[path = "managed_bridge_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "managed_bridge_process_tests.rs"]
mod process_tests;

pub(crate) async fn launch_managed(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    lease: &HeldRuntimeLease,
) -> Result<Box<dyn crate::driver::ProviderDriver>, DriverLaunchError> {
    let credential_id = crate::registration::require_runtime_registration(session)?
        .remote_spec
        .and_then(crate::remote_openai_spec::RemoteOpenAiSpec::credential_id);
    let credential = if let Some(provider) = credential_id {
        Some(crate::credentials::private_handoff::SelectedCredential {
            provider,
            secret: factory
                .credentials
                .secret(provider)
                .await
                .map(|secret| secret.expose().to_owned()),
        })
    } else {
        None
    };
    parent_process::launch(
        factory,
        Launch {
            session: Box::new(session.clone()),
            credential,
            state_root: factory.state_root.clone(),
        },
        lease,
    )
    .await
}

/// Native configuration locations cross only this private worker boundary.
pub(crate) fn environment() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut environment = crate::process::sanitized_environment();
    for name in [
        crate::codex::config::HOME_ENV,
        crate::grok::HOME_ENV,
        crate::grok_acp::AUTH_PATH_ENV,
        crate::claude_sdk_assets::RUNTIME_ENV,
    ] {
        if let Some(value) = std::env::var_os(name) {
            environment.push((name.into(), value));
        }
    }
    environment
}

#[cfg(all(test, windows))]
#[path = "managed_bridge_windows_tests.rs"]
mod windows_tests;
