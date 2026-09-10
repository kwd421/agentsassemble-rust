//! Serial control response custody outlives a bounded native caller's wait.
use std::{
    io::Write,
    sync::mpsc::RecvTimeoutError,
    time::{Duration, Instant},
};

use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};

use super::{RuntimeOutput, RuntimeProcess, control::TicketFailure};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) type PendingResponse =
    Box<dyn FnOnce(LocalControlResponse) -> Result<(), TicketFailure> + Send>;

pub(super) fn request_control<T: 'static>(
    runtime: &mut RuntimeProcess,
    request: &LocalControlRequest,
    decode: impl FnOnce(LocalControlResponse) -> Result<T, TicketFailure> + Send + 'static,
) -> Result<T, TicketFailure> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    require_live_child(runtime)?;
    if runtime.pending_response.is_some() {
        let response = receive(runtime, deadline)?;
        let previous = runtime.pending_response.take().ok_or_else(|| {
            TicketFailure::Broken("local control response lost its request owner".into())
        })?;
        // Validate the original identity, purpose and fields with the same decoder
        // used by its caller. A late grant is discarded, never lent to a new request.
        previous(response)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
    }
    let mut encoded = serde_json::to_vec(request).map_err(|error| {
        TicketFailure::Broken(format!("cannot encode local ticket request: {error}"))
    })?;
    encoded.push(b'\n');
    let control = runtime
        .control
        .as_mut()
        .ok_or_else(|| TicketFailure::Broken("local runtime control pipe is closed".into()))?;
    control
        .write_all(&encoded)
        .and_then(|()| control.flush())
        .map_err(|error| {
            TicketFailure::Broken(format!("cannot write local ticket request: {error}"))
        })?;
    match receive(runtime, deadline) {
        Ok(response) => decode(response),
        Err(error @ TicketFailure::Unavailable(_)) => {
            runtime.pending_response = Some(Box::new(move |response| decode(response).map(drop)));
            Err(error)
        }
        Err(error) => Err(error),
    }
}

fn receive(
    runtime: &mut RuntimeProcess,
    deadline: Instant,
) -> Result<LocalControlResponse, TicketFailure> {
    let response = match runtime
        .output
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        Ok(response) => response.map_err(TicketFailure::Broken)?,
        Err(RecvTimeoutError::Timeout) => {
            require_live_child(runtime)?;
            return Err(unavailable());
        }
        Err(RecvTimeoutError::Disconnected) => {
            return Err(TicketFailure::Broken(
                "local runtime control output is closed".into(),
            ));
        }
    };
    match response {
        RuntimeOutput::Control(response) => Ok(*response),
        RuntimeOutput::Startup(_) => Err(TicketFailure::Broken(
            "local runtime returned a duplicate startup record".into(),
        )),
    }
}

fn require_live_child(runtime: &mut RuntimeProcess) -> Result<(), TicketFailure> {
    if runtime
        .child
        .try_wait()
        .map_err(|error| TicketFailure::Broken(format!("cannot inspect local runtime: {error}")))?
        .is_some()
    {
        return Err(TicketFailure::Broken(
            "the owned Rust runtime exited before ticket issuance".into(),
        ));
    }
    Ok(())
}

fn unavailable() -> TicketFailure {
    TicketFailure::Unavailable(
        "local runtime control response is still pending; the owned runtime remains running".into(),
    )
}

#[cfg(test)]
#[path = "control_exchange_tests.rs"]
mod tests;
