//! Bound worker launch and exact child exit, without public listener or credential files.
use super::{
    Spawn,
    parent::ManagedDriver,
    parent_actor,
    platform::{self, Child, RuntimeProof},
    wire::{self, Event, Facts, Launch, protocol_error},
};
use crate::{
    driver::{DriverError, ProviderDriver},
    launch_error::DriverLaunchError,
    provider_factory::ProductionDriverFactory,
    runtime_lease::HeldRuntimeLease,
};
use std::{future::Future, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, watch},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

pub(super) struct Exit {
    pub(super) verified: bool,
}

pub(super) async fn launch(
    factory: &ProductionDriverFactory,
    launch: Launch,
    lease: &HeldRuntimeLease,
) -> Result<Box<dyn ProviderDriver>, DriverLaunchError> {
    let Spawn {
        mut child,
        connection,
        proof,
    } = platform::spawn(factory, &launch.session)?;
    let ready = tokio::time::timeout(super::CONTROL_TIMEOUT, connect(connection, &launch, lease))
        .await
        .unwrap_or_else(|_| Err(DriverLaunchError::uncertain(protocol_error())));
    let (mut input, mut output, facts) = match ready {
        Ok(connected) => connected,
        Err(error) => {
            // Failed/cancelled handshakes have already dropped both pipe halves.
            lease.release_launch_lifetime();
            let reaped = child.kill().await.is_ok();
            return Err(if reaped && proof.is_gone() {
                DriverLaunchError::safe(error.error)
            } else {
                DriverLaunchError::uncertain(error.error)
            });
        }
    };
    let (sender, receiver) = mpsc::channel(2);
    let (facts_tx, facts_rx) = watch::channel(facts);
    let session = Arc::new(*launch.session);
    let owner = session;
    let cleanup = proof.clone();
    let failure = CancellationToken::new();
    let failure_owner = failure.clone();
    let failure_guard = failure_owner.clone().drop_guard();
    let actor = AbortOnDropHandle::new(tokio::spawn(async move {
        let result = parent_actor::serve(
            &mut input,
            &mut output,
            &owner.public.session_id,
            receiver,
            &facts_tx,
        )
        .await;
        drop(input);
        drop(output);
        let exit = finish(&mut child, result, &facts_tx, &cleanup, &failure_owner).await;
        failure_guard.disarm();
        exit
    }));
    Ok(Box::new(ManagedDriver::new(
        sender, facts_rx, failure, actor, proof,
    )))
}

async fn connect<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    connection: impl Future<Output = Result<(R, W), DriverError>>,
    launch: &Launch,
    lease: &HeldRuntimeLease,
) -> Result<(wire::Reader<R>, wire::Writer<W>, Facts), DriverLaunchError> {
    let (input, output) = connection.await.map_err(DriverLaunchError::uncertain)?;
    let mut input = wire::reader(input);
    let mut output = wire::writer(output);
    wire::write(&mut output, launch)
        .await
        .map_err(DriverLaunchError::uncertain)?;
    match wire::read(&mut input)
        .await
        .map_err(DriverLaunchError::uncertain)?
    {
        Some(Event::Acquired) => lease.release_launch_lifetime(),
        Some(Event::Ready { result: Err(error) }) => return Err(error),
        _ => return Err(DriverLaunchError::uncertain(protocol_error())),
    }
    let facts = match wire::read(&mut input)
        .await
        .map_err(DriverLaunchError::uncertain)?
    {
        Some(Event::Facts { facts }) => facts,
        Some(Event::Ready { result: Err(error) }) => return Err(error),
        _ => return Err(DriverLaunchError::uncertain(protocol_error())),
    };
    let Some(Event::Ready { result }) = wire::read(&mut input)
        .await
        .map_err(DriverLaunchError::uncertain)?
    else {
        return Err(DriverLaunchError::uncertain(protocol_error()));
    };
    result?;
    Ok((input, output, facts))
}

async fn finish(
    child: &mut Child,
    result: Result<(tokio::sync::oneshot::Sender<Event>, Event), DriverError>,
    facts: &watch::Sender<Facts>,
    proof: &RuntimeProof,
    failure: &CancellationToken,
) -> Exit {
    if let Ok((reply, Event::Stopped { id, result })) = result {
        let status = tokio::time::timeout(super::CONTROL_TIMEOUT, child.wait()).await;
        let verified = matches!(status, Ok(Ok(_)));
        let exited_successfully = matches!(status, Ok(Ok(status)) if status.success());
        let result = result.and_then(|()| {
            if exited_successfully && proof.is_gone() {
                Ok(())
            } else {
                Err(protocol_error())
            }
        });
        if result.is_err() {
            facts.send_replace(failed_facts());
            failure.cancel();
        }
        if !verified {
            let _ = child.kill().await;
        }
        let _ = reply.send(Event::Stopped { id, result });
        Exit { verified }
    } else {
        facts.send_replace(failed_facts());
        failure.cancel();
        Exit {
            verified: child.kill().await.is_ok(),
        }
    }
}

fn failed_facts() -> Facts {
    Facts {
        retains_runtime_after_turn_interrupt: false,
        continuity: wire::Continuity::RestartRequired,
        attachment_replay_is_safe: false,
        turn_failure_effect_uncertain: true,
    }
}
