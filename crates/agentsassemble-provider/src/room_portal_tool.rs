use agentsassemble_domain::{
    LobbyMessageContext, LobbyMessageSearchPage, RoomRandomRequest, RoomRandomResult,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use super::{RoomToolAuthority, RoomToolReservation, tool_error};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ProviderRoomToolError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProviderRoomToolIngress {
    sender: mpsc::Sender<ProviderRoomToolCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRoomToolRequest {
    Random(RoomRandomRequest),
    SearchMessages { query: String, cursor: String },
    ReadMessageContext { event_id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderRoomToolResult {
    Random(RoomRandomResult),
    SearchMessages(LobbyMessageSearchPage),
    MessageContext(LobbyMessageContext),
}

impl PartialEq for ProviderRoomToolIngress {
    fn eq(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}

impl Eq for ProviderRoomToolIngress {}

impl ProviderRoomToolIngress {
    #[must_use]
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<ProviderRoomToolCommand>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender }, receiver)
    }

    pub(crate) async fn submit(
        &self,
        authority: RoomToolAuthority,
        request: ProviderRoomToolRequest,
        reservation: RoomToolReservation,
    ) -> Result<ProviderRoomToolResult, ProviderRoomToolError> {
        let (reply, response) = oneshot::channel();
        let command = ProviderRoomToolCommand {
            authority,
            request,
            reservation,
            reply: Some(reply),
            resolved: false,
        };
        if let Err(error) = self.sender.try_send(command) {
            let (command, failure) = match error {
                mpsc::error::TrySendError::Full(command) => (
                    command,
                    tool_error("room_busy", "The room tool queue is full."),
                ),
                mpsc::error::TrySendError::Closed(command) => (
                    command,
                    tool_error("room_unavailable", "The room tool owner stopped."),
                ),
            };
            command.complete(Err(failure));
        }
        response.await.unwrap_or_else(|_| {
            Err(tool_error(
                "room_unavailable",
                "The room tool response was lost.",
            ))
        })
    }
}

#[derive(Debug)]
pub struct ProviderRoomToolCommand {
    authority: RoomToolAuthority,
    request: ProviderRoomToolRequest,
    reservation: RoomToolReservation,
    reply: Option<oneshot::Sender<Result<ProviderRoomToolResult, ProviderRoomToolError>>>,
    resolved: bool,
}

impl ProviderRoomToolCommand {
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.authority.session_id
    }

    #[must_use]
    pub fn turn_id(&self) -> &str {
        &self.authority.turn_id
    }

    #[must_use]
    pub const fn input_up_to_seq(&self) -> i64 {
        self.authority.input_up_to_seq
    }

    #[must_use]
    pub const fn turn_generation(&self) -> u64 {
        self.authority.durable_turn_generation
    }

    #[must_use]
    pub fn execution_id(&self) -> &str {
        &self.authority.execution_id
    }

    #[must_use]
    pub fn request(&self) -> &ProviderRoomToolRequest {
        &self.request
    }

    /// Transfers a queued portal reservation to the room actor's execution phase.
    ///
    /// # Errors
    ///
    /// Rejects stale, closing, missing, or already-consumed turn authority.
    pub fn begin_execution(&mut self) -> Result<(), ProviderRoomToolError> {
        self.reservation.begin_execution()
    }

    pub fn complete(mut self, result: Result<ProviderRoomToolResult, ProviderRoomToolError>) {
        let result = result.and_then(|result| {
            response_matches(&self.request, &result)
                .then_some(result)
                .ok_or_else(|| {
                    tool_error(
                        "room_tool_invalid",
                        "The room tool owner returned a mismatched result.",
                    )
                })
        });
        self.reservation.resolve(result.is_ok());
        self.resolved = true;
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(result);
        }
    }
}

impl Drop for ProviderRoomToolCommand {
    fn drop(&mut self) {
        if self.resolved {
            return;
        }
        self.reservation.resolve(false);
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Err(tool_error(
                "room_unavailable",
                "The room tool command did not complete.",
            )));
        }
    }
}

fn response_matches(request: &ProviderRoomToolRequest, result: &ProviderRoomToolResult) -> bool {
    matches!(
        (request, result),
        (
            ProviderRoomToolRequest::Random(_),
            ProviderRoomToolResult::Random(_)
        ) | (
            ProviderRoomToolRequest::SearchMessages { .. },
            ProviderRoomToolResult::SearchMessages(_)
        ) | (
            ProviderRoomToolRequest::ReadMessageContext { .. },
            ProviderRoomToolResult::MessageContext(_)
        )
    )
}
