//! Bound worker launch and exact child exit, without public listener or credential files.
use super::{
    parent::ManagedDriver,
    parent_actor,
    wire::{self, Event, Facts, Launch, protocol_error},
};
use crate::{
    driver::{DriverError, ProviderDriver},
    guardian::GuardianLaunch,
    launch_error::DriverLaunchError,
    runtime_lease::HeldRuntimeLease,
};
use agentsassemble_domain::DurableAgentSession;
use std::sync::Arc;
use tokio::{
    net::UnixStream,
    process::Child,
    sync::{mpsc, watch},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

pub(super) struct Exit {
    pub(super) verified: bool,
}

pub(super) async fn launch(
    guardian: &GuardianLaunch,
    launch: Launch,
    lease: &HeldRuntimeLease,
) -> Result<Box<dyn ProviderDriver>, DriverLaunchError> {
    let (parent, child) = std::os::unix::net::UnixStream::pair().map_err(|_| protocol_error())?;
    parent.set_nonblocking(true).map_err(|_| protocol_error())?;
    let parent = UnixStream::from_std(parent).map_err(|_| protocol_error())?;
    let mut child = guardian
        .managed_command(child.into())
        .map_err(|_| protocol_error())?
        .spawn()
        .map_err(|_| protocol_error())?;
    let (input, output) = parent.into_split();
    let mut input = wire::reader(input);
    let mut output = wire::writer(output);
    let ready = tokio::time::timeout(super::CONTROL_TIMEOUT, async {
        wire::write(&mut output, &launch)
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
        Ok(facts)
    })
    .await
    .unwrap_or_else(|_| Err(DriverLaunchError::uncertain(protocol_error())));
    let facts = match ready {
        Ok(facts) => facts,
        Err(error) => {
            // Close both ends before releasing pre-acquisition lifetime custody.
            drop(input);
            drop(output);
            lease.release_launch_lifetime();
            let reaped = child.kill().await.is_ok();
            return Err(if reaped && gone(&launch.session) {
                DriverLaunchError::safe(error.error)
            } else {
                DriverLaunchError::uncertain(error.error)
            });
        }
    };
    let (sender, receiver) = mpsc::channel(2);
    let (facts_tx, facts_rx) = watch::channel(facts);
    let session = Arc::new(*launch.session);
    let owner = session.clone();
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
        let exit = finish(&mut child, result, &facts_tx, &owner, &failure_owner).await;
        failure_guard.disarm();
        exit
    }));
    Ok(Box::new(ManagedDriver::new(
        sender, facts_rx, failure, actor, session,
    )))
}

async fn finish(
    child: &mut Child,
    result: Result<(tokio::sync::oneshot::Sender<Event>, Event), DriverError>,
    facts: &watch::Sender<Facts>,
    session: &DurableAgentSession,
    failure: &CancellationToken,
) -> Exit {
    if let Ok((reply, Event::Stopped { id, result })) = result {
        let status = tokio::time::timeout(super::CONTROL_TIMEOUT, child.wait()).await;
        let verified = matches!(status, Ok(Ok(_)));
        let exited_successfully = matches!(status, Ok(Ok(status)) if status.success());
        let result = result.and_then(|()| {
            if exited_successfully && gone(session) {
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

pub(super) fn gone(session: &DurableAgentSession) -> bool {
    crate::runtime_absence::observation_proves_gone(
        &session.runtime_handle_id,
        &session.runtime_lease_token,
        &crate::runtime_lease::observe_runtime_lease(
            &session.public.room_id,
            &session.public.session_id,
        ),
        crate::runtime_absence::ObservationScope::LiveSlot,
    )
}
fn failed_facts() -> Facts {
    Facts {
        retains_runtime_after_turn_interrupt: false,
        continuity: wire::Continuity::RestartRequired,
        attachment_replay_is_safe: false,
        turn_failure_effect_uncertain: true,
    }
}
