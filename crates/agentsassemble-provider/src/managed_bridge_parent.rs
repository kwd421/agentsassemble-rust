//! Cancellation-safe provider interface: each pending operation has one wire send.
use super::{
    parent_actor::{Call, Operation},
    parent_process::Exit,
    platform::RuntimeProof,
    wire::{Continuity, Event, Facts, protocol_error},
};
use crate::{
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    room_portal::ProviderTurnOutcome,
};
use agentsassemble_domain::DurableAgentSession;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;

struct Pending {
    operation: Arc<Operation>,
    reply: oneshot::Receiver<Event>,
}
pub(super) struct ManagedDriver {
    calls: mpsc::Sender<Call>,
    facts: watch::Receiver<Facts>,
    failure: CancellationToken,
    actor: Option<AbortOnDropHandle<Exit>>,
    exit_verified: bool,
    proof: Arc<RuntimeProof>,
    pending: Option<Pending>,
    prepared: Option<Arc<ProviderTurnRequest>>,
    sent: Option<Arc<Operation>>,
    completed: Option<Result<ProviderTurnCompleted, DriverError>>,
}
impl ManagedDriver {
    pub(super) fn new(
        calls: mpsc::Sender<Call>,
        facts: watch::Receiver<Facts>,
        failure: CancellationToken,
        actor: AbortOnDropHandle<Exit>,
        proof: Arc<RuntimeProof>,
    ) -> Self {
        Self {
            calls,
            facts,
            failure,
            actor: Some(actor),
            proof,
            exit_verified: false,
            pending: None,
            prepared: None,
            sent: None,
            completed: None,
        }
    }
    async fn receive(&mut self) -> Result<Event, DriverError> {
        let pending = self.pending.as_mut().ok_or_else(protocol_error)?;
        let result = (&mut pending.reply).await;
        let pending = self.pending.take().ok_or_else(protocol_error)?;
        let event = result.map_err(|_| protocol_error())?;
        match (&*pending.operation, &event) {
            (Operation::Prepare(request), Event::Prepared { result: Ok(()), .. }) => {
                self.prepared = Some(request.clone());
                self.sent = None;
                self.completed = None;
            }
            (_, Event::Finished { result: Ok(_), .. } | Event::Aborted { result: Ok(()), .. }) => {
                self.prepared = None;
                self.sent = None;
                self.completed = None;
            }
            (_, Event::Turn { result, .. }) => self.completed = Some(result.clone()),
            _ => {}
        }
        Ok(event)
    }
    async fn rpc(&mut self, operation: Operation) -> Result<Event, DriverError> {
        if let Some(pending) = &self.pending {
            if *pending.operation == operation {
                return self.receive().await;
            }
            if matches!(operation, Operation::Stop)
                || (matches!(*pending.operation, Operation::Send(_))
                    && matches!(operation, Operation::Interrupt(_) | Operation::Abort))
            {
                self.pending.take();
            } else {
                previous_result(self.receive().await?)?;
            }
        }
        let operation = Arc::new(operation);
        let (reply, receive) = oneshot::channel();
        // This synchronous bounded enqueue has no cancellation point between sending and retaining custody.
        self.calls
            .try_send(Call {
                operation: operation.clone(),
                reply,
            })
            .map_err(|_| protocol_error())?;
        if matches!(*operation, Operation::Send(_)) {
            self.sent = Some(operation.clone());
        }
        self.pending = Some(Pending {
            operation,
            reply: receive,
        });
        self.receive().await
    }
    async fn prepare(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        if self.prepared.as_deref() == Some(request) {
            return Ok(());
        }
        let Event::Prepared { result, .. } = self
            .rpc(Operation::Prepare(Arc::new(request.clone())))
            .await?
        else {
            return Err(protocol_error());
        };
        result
    }
    fn require_turn(&self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        if self.prepared.as_deref() != Some(request) {
            return Err(protocol_error());
        }
        Ok(())
    }
    async fn join(&mut self) -> Result<(), DriverError> {
        if let Some(actor) = self.actor.as_mut() {
            let result = actor.await;
            self.actor.take();
            self.exit_verified = result.map_err(|_| protocol_error())?.verified;
        }
        if self.exit_verified && self.proof.is_gone() {
            Ok(())
        } else {
            Err(protocol_error())
        }
    }
}
impl ProviderDriver for ManagedDriver {
    fn runtime_failure_signal(&self) -> Option<CancellationToken> {
        Some(self.failure.clone())
    }
    fn retains_runtime_after_turn_interrupt(&self) -> bool {
        self.facts.has_changed().is_ok() && self.facts.borrow().retains_runtime_after_turn_interrupt
    }
    fn requires_restart(&self) -> bool {
        self.facts.has_changed().is_err()
            || matches!(self.facts.borrow().continuity, Continuity::RestartRequired)
    }
    fn attachment_replay_is_safe(&self) -> bool {
        self.facts.has_changed().is_ok() && self.facts.borrow().attachment_replay_is_safe
    }
    fn turn_failure_effect_uncertain(&self) -> bool {
        self.facts.has_changed().is_err() || self.facts.borrow().turn_failure_effect_uncertain
    }
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async move {
            let Event::Attached { result, .. } = self
                .rpc(Operation::Attach(Box::new(session.clone())))
                .await?
            else {
                return Err(protocol_error());
            };
            result
        })
    }
    fn begin_room_observation<'a>(
        &'a mut self,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(self.prepare(request))
    }
    fn send_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(async move {
            if request.room_observation.is_none() {
                self.prepare(request).await?;
            }
            self.require_turn(request)?;
            let operation = Operation::Send(Box::new(session.clone()));
            if let Some(result) = &self.completed {
                if self.sent.as_deref() != Some(&operation) {
                    return Err(protocol_error());
                }
                return result.clone();
            }
            if self.sent.is_some()
                && !self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| *pending.operation == operation)
            {
                return Err(protocol_error());
            }
            let Event::Turn { result, .. } = self.rpc(operation).await? else {
                return Err(protocol_error());
            };
            result
        })
    }
    fn interrupt_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            if request.room_observation.is_none() {
                self.prepare(request).await?;
            }
            self.require_turn(request)?;
            let Event::Interrupted { result, .. } = self
                .rpc(Operation::Interrupt(Box::new(session.clone())))
                .await?
            else {
                return Err(protocol_error());
            };
            result
        })
    }
    fn finish_room_observation<'a>(
        &'a mut self,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnOutcome, DriverError>> {
        Box::pin(async move {
            self.require_turn(request)?;
            let Event::Finished { result, .. } = self.rpc(Operation::Finish).await? else {
                return Err(protocol_error());
            };
            result
        })
    }
    fn abort_room_observation(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            let Event::Aborted { result, .. } = self.rpc(Operation::Abort).await? else {
                return Err(protocol_error());
            };
            result
        })
    }
    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async move {
            let Event::Alive { result, .. } = self.rpc(Operation::IsAlive).await? else {
                return Err(protocol_error());
            };
            result
        })
    }
    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            if !self
                .pending
                .as_ref()
                .is_some_and(|pending| matches!(*pending.operation, Operation::Stop))
                && self
                    .actor
                    .as_ref()
                    .is_none_or(AbortOnDropHandle::is_finished)
            {
                return self.join().await;
            }
            let result = match self.rpc(Operation::Stop).await {
                Ok(Event::Stopped { result, .. }) => result,
                _ => Err(protocol_error()),
            };
            let absent = self.join().await;
            result.and(absent)
        })
    }
}
fn previous_result(event: Event) -> Result<(), DriverError> {
    match event {
        Event::Attached { result, .. } => result.map(|_| ()),
        Event::Prepared { result, .. }
        | Event::Interrupted { result, .. }
        | Event::Aborted { result, .. }
        | Event::Stopped { result, .. } => result,
        Event::Turn { result, .. } => result.map(|_| ()),
        Event::Finished { result, .. } => result.map(|_| ()),
        Event::Alive { result, .. } => result.map(|_| ()),
        _ => Err(protocol_error()),
    }
}
