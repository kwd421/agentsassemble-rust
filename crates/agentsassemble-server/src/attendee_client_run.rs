//! Event-driven external session: retained local execution survives network replacement.
use crate::{
    AttendeeClientError, AttendeeExecution, AttendeeInterrupt, AttendeeJoined, AttendeeRuntime,
    AttendeeSocket, AttendeeSocketFrame as Frame, AttendeeSocketRequest as Request,
    RoomAttendeeClient,
};
use agentsassemble_persistence::AttendeeCleanupDelivery;
use agentsassemble_protocol::CommandResolution;
use agentsassemble_provider::{ProviderAttachmentReadIngress, ProviderRoomToolIngress};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "attendee_client_run_tools.rs"]
mod tools;
use tools::Tools;

#[path = "attendee_client_run_requests.rs"]
mod requests;
use requests::Requests;

const RETRY_DELAY: Duration = Duration::from_secs(1);
const ACK_TIMEOUT: Duration = Duration::from_secs(30);

struct Session<'a> {
    client: &'a RoomAttendeeClient,
    runtime: &'a mut AttendeeRuntime,
    execution: Option<AttendeeExecution>,
    interrupt: Option<AttendeeInterrupt>,
    tools: Tools,
    requests: Requests,
    pending: Option<Request>,
    connection_ready: bool,
}

/// Runs one already-admitted client's provider until cancellation, expiry or remote stop.
/// The caller must stop the runtime and complete cleanup on every return, including errors.
///
/// # Errors
/// Returns exact custody/protocol/provider failures without exposing private diagnostics.
pub async fn run_attendee_session(
    client: &RoomAttendeeClient,
    joined: &AttendeeJoined,
    runtime: &mut AttendeeRuntime,
    cancellation: &CancellationToken,
) -> Result<Option<AttendeeCleanupDelivery>, AttendeeClientError> {
    if joined.expires_at <= chrono::Utc::now() {
        return Err(error("attendee_session_expired"));
    }
    let ready = tokio::select! {
        biased;
        () = cancellation.cancelled() => return Ok(None),
        ready = runtime.start() => ready?,
    };
    let mut session = Session {
        client,
        runtime,
        execution: None,
        interrupt: None,
        tools: Tools::new(),
        requests: Requests::new(),
        connection_ready: false,
        pending: Some(Request::Ready {
            request_id: Uuid::new_v4(),
            report: Box::new(ready),
        }),
    };
    let remaining = (joined.expires_at - chrono::Utc::now())
        .to_std()
        .unwrap_or_default();
    tokio::select! {
        () = cancellation.cancelled() => Ok(None),
        () = tokio::time::sleep(remaining) => Err(error("attendee_session_expired")),
        result = session.run() => result,
    }
}

impl Session<'_> {
    async fn run(&mut self) -> Result<Option<AttendeeCleanupDelivery>, AttendeeClientError> {
        loop {
            match self.client.connect().await {
                Ok(mut socket) => match self.connected(&mut socket).await {
                    Ok(stop) => return Ok(stop),
                    Err(failure) if failure.is_retryable() => {
                        eprintln!("Attendee connection lost; reconnecting ({})", failure.code);
                    }
                    Err(failure) => return Err(failure),
                },
                Err(failure) if failure.is_retryable() => {
                    eprintln!("Attendee reconnect pending ({})", failure.code);
                }
                Err(failure) => return Err(failure),
            }
            self.requests.clear();
            // Disconnection is the event that checks stop custody; this is not a room-state poll.
            match self.client.cleanup().await {
                Ok(Some(stop)) => return Ok(Some(stop)),
                Ok(None) => {}
                Err(failure) if failure.is_retryable() => {}
                Err(failure) => return Err(failure),
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }

    async fn connected(
        &mut self,
        socket: &mut AttendeeSocket,
    ) -> Result<Option<AttendeeCleanupDelivery>, AttendeeClientError> {
        self.connection_ready = false;
        // A completed result must resolve before Ready advertises its changed provider session.
        if self.pending.is_none() && self.interrupt.is_none() {
            self.pending = match self.next_report() {
                Some(report) => Some(report),
                None => Some(self.ready_request().await?),
            };
        }
        let mut sent = None;
        let mut ack_deadline = tokio::time::Instant::now() + ACK_TIMEOUT;
        let period = crate::attendee_wire::SOCKET_IDLE / 2;
        let mut keepalive = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        let connection = socket.connection_id();
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            if let Some(request) = &self.pending
                && sent != Some(request.request_id())
            {
                socket.send(request).await?;
                sent = Some(request.request_id());
                ack_deadline = tokio::time::Instant::now() + ACK_TIMEOUT;
            }
            self.requests.send(socket).await?;
            tokio::select! {
                () = requests::wait_timeout(self.requests.deadline()) => return Err(error("attendee_provider_ack_timeout")),
                result = self.requests.step() => result?,
                frame = socket.receive() => {
                    if let Some(stop) = self.frame(frame?).await? { return Ok(Some(stop)); }
                }
                () = tokio::time::sleep_until(ack_deadline), if self.pending.is_some() => {
                    return Err(error("attendee_ack_unresolved"));
                }
                _ = keepalive.tick() => socket.ping().await?,
                result = complete_execution(&mut self.execution), if self.interrupt.is_none() => {
                    result?;
                    if self.pending.is_none() { self.pending = self.next_report(); }
                }
                result = report_interrupt(self.client, connection, &mut self.interrupt) => {
                    result?;
                    let interrupt = self.interrupt.take().ok_or_else(|| error("attendee_interrupt_missing"))?;
                    let gone = interrupt.report().is_some_and(|report| matches!(report.runtime, agentsassemble_persistence::AttendeeInterruptedRuntime::Gone));
                    interrupt.acknowledge(self.runtime, self.execution.as_mut()).await?;
                    if gone { return Ok(None); }
                    self.execution = None;
                    self.tools.clear();
                    self.requests.clear();
                    self.pending = Some(self.ready_request().await?);
                }
                result = self.tools.step(self.client, connection) => result?,
            }
        }
    }

    async fn frame(
        &mut self,
        frame: Frame,
    ) -> Result<Option<AttendeeCleanupDelivery>, AttendeeClientError> {
        match frame {
            frame @ (Frame::ProviderResponse { .. } | Frame::ProviderRequestClosed { .. }) => {
                self.requests.frame(frame)?;
            }
            frame @ (Frame::Ack { request_id, .. } | Frame::Nack { request_id, .. })
                if self.requests.owns_reply(request_id) =>
            {
                self.requests.frame(frame)?;
            }
            Frame::Stop { stop } => return Ok(Some(stop)),
            Frame::Connected { .. } => return Err(error("unexpected_attendee_connection")),
            Frame::Nack {
                request_id,
                resolution,
                error: failure,
            } => {
                if self
                    .pending
                    .as_ref()
                    .is_none_or(|request| request.request_id() != request_id)
                {
                    return Err(error("attendee_ack_mismatch"));
                }
                return Err(AttendeeClientError {
                    code: failure.code,
                    resolution: Some(resolution),
                });
            }
            Frame::Ack {
                request_id,
                resolution,
                ..
            } => {
                self.acknowledge(request_id, resolution).await?;
            }
            Frame::Turn { assignment } => {
                if let Some(execution) = &self.execution {
                    if !execution.matches_delivery(&assignment) {
                        return Err(error("attendee_execution_mismatch"));
                    }
                } else if self.interrupt.is_none() {
                    self.execution = Some(
                        self.runtime
                            .execute(
                                *assignment,
                                self.tools.ingress.clone(),
                                self.tools.attachments.clone(),
                                Some(self.requests.ingress.clone()),
                            )
                            .await?,
                    );
                }
            }
            Frame::Interrupt { interrupt } => {
                self.requests.clear();
                if let Some(owned) = &self.interrupt {
                    if !owned.matches_delivery(&interrupt) {
                        return Err(error("attendee_interrupt_mismatch"));
                    }
                } else {
                    self.interrupt = Some(
                        self.runtime
                            .interrupt(*interrupt, self.execution.as_ref())?,
                    );
                }
            }
        }
        Ok(None)
    }

    async fn acknowledge(
        &mut self,
        request_id: Uuid,
        resolution: CommandResolution,
    ) -> Result<(), AttendeeClientError> {
        if resolution != CommandResolution::Committed {
            return Err(error("invalid_attendee_ack"));
        }
        let pending = self
            .pending
            .take()
            .ok_or_else(|| error("attendee_ack_mismatch"))?;
        if pending.request_id() != request_id {
            return Err(error("attendee_ack_mismatch"));
        }
        match pending {
            Request::ProviderRequestOpen { .. } | Request::ProviderRequestDelivered { .. } => {
                return Err(error("unexpected_provider_request_ack"));
            }
            Request::Report { .. } => {
                let execution = self
                    .execution
                    .take()
                    .ok_or_else(|| error("attendee_execution_missing"))?;
                execution.acknowledge(self.runtime, request_id).await?;
                if !self.connection_ready {
                    self.pending = Some(self.ready_request().await?);
                }
            }
            Request::Started { .. } => {
                self.pending = self
                    .execution
                    .as_ref()
                    .and_then(AttendeeExecution::report_request);
            }
            Request::Ready { .. } => {
                self.connection_ready = true;
                self.pending = self.next_report();
            }
        }
        Ok(())
    }

    fn next_report(&self) -> Option<Request> {
        self.execution.as_ref().and_then(|execution| {
            execution
                .started_request()
                .or_else(|| execution.report_request())
        })
    }

    async fn ready_request(&mut self) -> Result<Request, AttendeeClientError> {
        Ok(Request::Ready {
            request_id: Uuid::new_v4(),
            report: Box::new(self.runtime.start().await?),
        })
    }
}

async fn complete_execution(
    execution: &mut Option<AttendeeExecution>,
) -> Result<(), AttendeeClientError> {
    match execution {
        Some(execution) if execution.is_running() => execution.complete().await,
        _ => std::future::pending().await,
    }
}
async fn report_interrupt(
    client: &RoomAttendeeClient,
    connection: Uuid,
    interrupt: &mut Option<AttendeeInterrupt>,
) -> Result<(), AttendeeClientError> {
    let Some(interrupt) = interrupt else {
        return std::future::pending().await;
    };
    interrupt.complete().await?;
    let report = interrupt
        .report()
        .ok_or_else(|| error("attendee_interrupt_unconfirmed"))?;
    client.report_interrupt(connection, report).await
}

fn error(code: &str) -> AttendeeClientError {
    AttendeeClientError::local(code)
}
