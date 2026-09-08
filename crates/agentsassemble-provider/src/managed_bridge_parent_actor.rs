//! Sole owner of private transport progress, including callbacks after a cancelled caller.
use super::{
    parent_callbacks::ParentCallbacks,
    pipe::Pipe,
    wire::{Command, Event, Facts, Reader, Writer, protocol_error},
};
use crate::driver::{DriverError, ProviderTurnRequest};
use agentsassemble_domain::DurableAgentSession;
use std::{collections::BTreeMap, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

#[derive(PartialEq, Eq)]
pub(super) enum Operation {
    Attach(Box<DurableAgentSession>),
    Prepare(Arc<ProviderTurnRequest>),
    Send(Box<DurableAgentSession>),
    Interrupt(Box<DurableAgentSession>),
    Finish,
    Abort,
    IsAlive,
    Stop,
}

impl Operation {
    fn command(&self, id: u64) -> Command {
        match self {
            Self::Attach(session) => Command::Attach {
                id,
                session: session.clone(),
            },
            Self::Prepare(request) => Command::Prepare {
                id,
                turn: request.as_ref().into(),
            },
            Self::Send(session) => Command::Send {
                id,
                session: session.clone(),
            },
            Self::Interrupt(session) => Command::Interrupt {
                id,
                session: session.clone(),
            },
            Self::Finish => Command::Finish { id },
            Self::Abort => Command::Abort { id },
            Self::IsAlive => Command::IsAlive { id },
            Self::Stop => Command::Stop { id },
        }
    }
    fn accepts(&self, event: &Event) -> bool {
        matches!(
            (self, event),
            (Self::Attach(_), Event::Attached { .. })
                | (Self::Prepare(_), Event::Prepared { .. })
                | (
                    Self::Send(_),
                    Event::Turn { .. } | Event::TurnCancelled { .. }
                )
                | (Self::Interrupt(_), Event::Interrupted { .. })
                | (Self::Finish, Event::Finished { .. })
                | (Self::Abort, Event::Aborted { .. })
                | (Self::IsAlive, Event::Alive { .. })
                | (Self::Stop, Event::Stopped { .. })
        )
    }
}

pub(super) struct Call {
    pub(super) operation: Arc<Operation>,
    pub(super) reply: oneshot::Sender<Event>,
}
struct Pending {
    call: Call,
    deadline: Option<Instant>,
}

pub(super) async fn serve<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: &mut Reader<R>,
    output: &mut Writer<W>,
    session_id: &str,
    mut calls: mpsc::Receiver<Call>,
    facts: &watch::Sender<Facts>,
) -> Result<(oneshot::Sender<Event>, Event), DriverError> {
    let mut pipe = Pipe::new(input, output);
    let mut callbacks = ParentCallbacks::new();
    let mut pending = BTreeMap::<u64, Pending>::new();
    let mut next_id = 1_u64;
    let mut context = None;
    loop {
        let deadline = pending.values().filter_map(|call| call.deadline).min();
        tokio::select! {
            event = pipe.read::<Event>() => {
                let event = event?.ok_or_else(protocol_error)?;
                match event {
                    Event::Facts { facts: value } => { facts.send_replace(value); }
                    Event::Callback { id, callback } => callbacks.receive(id, callback, session_id, context.clone())?,
                    event => {
                        let id = event_id(&event).ok_or_else(protocol_error)?;
                        let call = pending.remove(&id).ok_or_else(protocol_error)?.call;
                        if !call.operation.accepts(&event) { return Err(protocol_error()); }
                        match (&*call.operation, &event) {
                            (Operation::Prepare(request), Event::Prepared { result: Ok(()), .. }) => context = Some(request.clone()),
                            (_, Event::Finished { result: Ok(_), .. } | Event::Aborted { result: Ok(()), .. }) => context = None,
                            (_, Event::Stopped { .. }) => return Ok((call.reply, event)),
                            _ => {}
                        }
                        let _ = call.reply.send(event);
                    }
                }
            }
            reply = callbacks.next() => {
                let (callback_id, reply) = reply?;
                pipe.queue(&Command::Callback { id: allocate(&mut next_id)?, callback_id, reply })?;
            }
            call = calls.recv() => {
                let call = call.ok_or_else(protocol_error)?;
                // Native non-send operations only accept callback replies until their acknowledgement.
                if !matches!(*call.operation, Operation::Stop) && (pending.values().any(|pending| !matches!(*pending.call.operation, Operation::Send(_)))
                    || (!pending.is_empty() && !matches!(*call.operation, Operation::Interrupt(_) | Operation::Abort))) {
                    return Err(protocol_error());
                }
                if matches!(*call.operation, Operation::Stop) {
                    for pending in pending.values_mut() { pending.deadline = None; }
                }
                let deadline = (!matches!(*call.operation, Operation::Send(_))).then(|| Instant::now() + super::CONTROL_TIMEOUT);
                let id = allocate(&mut next_id)?;
                pipe.queue(&call.operation.command(id))?;
                pending.insert(id, Pending { call, deadline });
            }
            () = async {
                if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; }
                else { std::future::pending().await }
            } => return Err(protocol_error()),
        }
    }
}

fn allocate(next: &mut u64) -> Result<u64, DriverError> {
    let id = *next;
    *next = next.checked_add(1).ok_or_else(protocol_error)?;
    Ok(id)
}
fn event_id(event: &Event) -> Option<u64> {
    match event {
        Event::Attached { id, .. }
        | Event::Prepared { id, .. }
        | Event::Turn { id, .. }
        | Event::TurnCancelled { id }
        | Event::Interrupted { id, .. }
        | Event::Finished { id, .. }
        | Event::Aborted { id, .. }
        | Event::Alive { id, .. }
        | Event::Stopped { id, .. } => Some(*id),
        _ => None,
    }
}
