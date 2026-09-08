//! One native driver and one active turn; pipe cancellation never replays a native send.
use agentsassemble_domain::DurableAgentSession;
use tokio::io::{AsyncRead, AsyncWrite};

use super::{
    callbacks::Callbacks,
    pipe::Pipe,
    same_runtime,
    wire::{Command, Event, Facts, Reader, Writer, protocol_error},
};
use crate::driver::{DriverError, ProviderDriver, ProviderTurnRequest};

struct ActiveTurn {
    session: Box<DurableAgentSession>,
    request: ProviderTurnRequest,
    execution: Execution,
}

#[derive(PartialEq, Eq)]
enum Execution {
    Prepared,
    Entered,
    Returned,
}

pub(super) async fn serve<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: &mut Reader<R>,
    output: &mut Writer<W>,
    launched: &DurableAgentSession,
    driver: &mut dyn ProviderDriver,
) -> Result<Result<(), DriverError>, DriverError> {
    let mut pipe = Pipe::new(input, output);
    let mut expected = 1_u64;
    let mut callbacks = Callbacks::new();
    let mut active: Option<ActiveTurn> = None;
    loop {
        let command = next(&mut pipe, &mut callbacks).await?;
        check_sequence(&command, &mut expected)?;
        if let Command::Send { id, session } = command {
            if !same_runtime(launched, &session) {
                return Err(protocol_error());
            }
            let turn = active.as_mut().ok_or_else(protocol_error)?;
            turn.session = session;
            let command = send(&mut pipe, driver, turn, &mut callbacks, id, &mut expected).await?;
            if let Some(command) = command
                && let Some(stopped) = execute(
                    command,
                    &mut pipe,
                    launched,
                    driver,
                    &mut active,
                    &mut callbacks,
                    &mut expected,
                )
                .await?
            {
                return Ok(stopped);
            }
        } else if let Some(stopped) = execute(
            command,
            &mut pipe,
            launched,
            driver,
            &mut active,
            &mut callbacks,
            &mut expected,
        )
        .await?
        {
            return Ok(stopped);
        }
    }
}

async fn next<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    pipe: &mut Pipe<'_, R, W>,
    callbacks: &mut Callbacks,
) -> Result<Command, DriverError> {
    loop {
        tokio::select! {
            // Drain a returned callback into its native owner before the next control command.
            biased;
            event = callbacks.next() => {
                let (id, callback) = event?;
                pipe.queue(&Event::Callback { id, callback })?;
            }
            command = pipe.read() => return command?.ok_or_else(protocol_error),
        }
    }
}

fn check_sequence(command: &Command, expected: &mut u64) -> Result<(), DriverError> {
    if command.id() != *expected {
        return Err(protocol_error());
    }
    *expected = expected.checked_add(1).ok_or_else(protocol_error)?;
    Ok(())
}

fn respond<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    pipe: &mut Pipe<'_, R, W>,
    driver: &dyn ProviderDriver,
    event: &Event,
) -> Result<(), DriverError> {
    pipe.queue(&Event::Facts {
        facts: Facts::observe(driver),
    })?;
    pipe.queue(event)
}

async fn execute<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    command: Command,
    pipe: &mut Pipe<'_, R, W>,
    launched: &DurableAgentSession,
    driver: &mut dyn ProviderDriver,
    active: &mut Option<ActiveTurn>,
    callbacks: &mut Callbacks,
    expected: &mut u64,
) -> Result<Option<Result<(), DriverError>>, DriverError> {
    let id = command.id();
    let event = match command {
        Command::Attach { session, .. } => {
            if active.is_some() || !same_runtime(launched, &session) {
                return Err(protocol_error());
            }
            match drive(pipe, callbacks, expected, driver.attach_session(&session)).await? {
                Drive::Returned(result) => Event::Attached { id, result },
                Drive::Stop(id) => return stop(pipe, driver, active, callbacks, id).await,
            }
        }
        Command::Prepare { turn, .. } => {
            if active.as_ref().is_some_and(|turn| {
                turn.request.room_observation.is_some() || turn.execution != Execution::Returned
            }) {
                return Err(protocol_error());
            }
            let request = turn.into_request(callbacks);
            let result = if request.room_observation.is_some() {
                driver.begin_room_observation(&request).await
            } else {
                Ok(())
            };
            if result.is_ok() {
                *active = Some(ActiveTurn {
                    session: Box::new(launched.clone()),
                    request,
                    execution: Execution::Prepared,
                });
            }
            Event::Prepared { id, result }
        }
        Command::Interrupt { session, .. } => {
            if !same_runtime(launched, &session) {
                return Err(protocol_error());
            }
            let turn = active.as_mut().ok_or_else(protocol_error)?;
            turn.session = session;
            match drive(
                pipe,
                callbacks,
                expected,
                driver.interrupt_turn(&turn.session, &turn.request),
            )
            .await?
            {
                Drive::Returned(result) => Event::Interrupted { id, result },
                Drive::Stop(id) => return stop(pipe, driver, active, callbacks, id).await,
            }
        }

        Command::Finish { .. } => {
            let turn = active
                .as_ref()
                .filter(|turn| turn.execution != Execution::Prepared)
                .ok_or_else(protocol_error)?;
            let result = driver.finish_room_observation(&turn.request).await;
            if result.is_ok() {
                *active = None;
            }
            Event::Finished { id, result }
        }
        Command::Abort { .. } => {
            let result = driver.abort_room_observation().await;
            if result.is_ok() {
                *active = None;
            }
            Event::Aborted { id, result }
        }
        Command::IsAlive { .. } => Event::Alive {
            id,
            result: driver.is_alive().await,
        },
        Command::Stop { .. } => return stop(pipe, driver, active, callbacks, id).await,
        Command::Callback {
            callback_id, reply, ..
        } => {
            callbacks.reply(callback_id, reply)?;
            return Ok(None);
        }
        Command::Send { .. } => return Err(protocol_error()),
    };
    respond(pipe, driver, &event)?;
    Ok(None)
}

async fn send<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    pipe: &mut Pipe<'_, R, W>,
    driver: &mut dyn ProviderDriver,
    turn: &mut ActiveTurn,
    callbacks: &mut Callbacks,
    id: u64,
    expected: &mut u64,
) -> Result<Option<Command>, DriverError> {
    enum Exit {
        Completed(Result<crate::driver::ProviderTurnCompleted, DriverError>),
        Control(Command),
    }
    if turn.execution != Execution::Prepared {
        return Err(protocol_error());
    }
    turn.execution = Execution::Entered;
    let result = {
        let send = driver.send_turn(&turn.session, &turn.request);
        tokio::pin!(send);
        loop {
            tokio::select! {
                biased;
                event = callbacks.next() => {
                    let (id, callback) = event?;
                    pipe.queue(&Event::Callback { id, callback })?;
                }
                command = pipe.read::<Command>() => {
                    let command = command?.ok_or_else(protocol_error)?;
                    check_sequence(&command, expected)?;
                    match command {
                        Command::Callback { callback_id, reply, .. } => callbacks.reply(callback_id, reply)?,
                        Command::Interrupt { .. } | Command::Abort { .. } | Command::Stop { .. } => {
                            break Exit::Control(command);
                        }
                        _ => return Err(protocol_error()),
                    }
                }
                result = &mut send => break Exit::Completed(result),
            }
        }
    };
    match result {
        Exit::Completed(result) => {
            turn.execution = Execution::Returned;
            respond(pipe, driver, &Event::Turn { id, result })?;
            Ok(None)
        }
        Exit::Control(command) => {
            // The native send future has been dropped before cancellation is reported.
            pipe.queue(&Event::TurnCancelled { id })?;
            Ok(Some(command))
        }
    }
}

async fn stop<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    pipe: &mut Pipe<'_, R, W>,
    driver: &mut dyn ProviderDriver,
    active: &mut Option<ActiveTurn>,
    callbacks: &mut Callbacks,
    id: u64,
) -> Result<Option<Result<(), DriverError>>, DriverError> {
    *active = None;
    *callbacks = Callbacks::new();
    let result = driver.stop().await;
    respond(
        pipe,
        driver,
        &Event::Stopped {
            id,
            result: result.clone(),
        },
    )?;
    pipe.flush().await?;
    Ok(Some(result))
}

enum Drive<T> {
    Returned(Result<T, DriverError>),
    Stop(u64),
}

async fn drive<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, T>(
    pipe: &mut Pipe<'_, R, W>,
    callbacks: &mut Callbacks,
    expected: &mut u64,
    mut operation: crate::driver::DriverFuture<'_, Result<T, DriverError>>,
) -> Result<Drive<T>, DriverError> {
    loop {
        tokio::select! {
            biased;
            event = callbacks.next() => {
                let (id, callback) = event?;
                pipe.queue(&Event::Callback { id, callback })?;
            }
            command = pipe.read::<Command>() => {
                let command = command?.ok_or_else(protocol_error)?;
                check_sequence(&command, expected)?;
                match command {
                    Command::Callback { callback_id, reply, .. } => callbacks.reply(callback_id, reply)?,
                    Command::Stop { id } => return Ok(Drive::Stop(id)),
                    _ => return Err(protocol_error()),
                }
            }
            result = &mut operation => return Ok(Drive::Returned(result)),
        }
    }
}
