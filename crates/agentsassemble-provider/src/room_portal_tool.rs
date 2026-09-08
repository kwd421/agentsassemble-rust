use agentsassemble_domain::{
    RoomMessageContext, RoomMessageSearchPage, RoomRandomRequest, RoomRandomResult,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use super::{RoomToolAuthority, RoomToolReservation, tool_error};

#[derive(Debug, Clone, PartialEq, Eq, Error, serde::Serialize, serde::Deserialize)]
#[error("{message}")]
pub struct ProviderRoomToolError {
    pub code: std::borrow::Cow<'static, str>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProviderRoomToolIngress {
    sender: mpsc::Sender<ProviderRoomToolCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProviderRoomToolRequest {
    Random(RoomRandomRequest),
    SearchMessages { query: String, cursor: String },
    ReadMessageContext { event_id: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProviderRoomToolResult {
    Random(RoomRandomResult),
    SearchMessages(RoomMessageSearchPage),
    MessageContext(RoomMessageContext),
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
        reservation: impl Into<Admission>,
    ) -> Result<ProviderRoomToolResult, ProviderRoomToolError> {
        let (reply, response) = oneshot::channel();
        let command = ProviderRoomToolCommand {
            authority,
            request,
            reservation: reservation.into(),
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
    reservation: Admission,
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
    pub fn begin_execution(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), ProviderRoomToolError>> + Send {
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

/// The child keeps the native reservation; a relayed command waits for its answer.
#[derive(Debug)]
pub(crate) enum Admission {
    Local(RoomToolReservation),
    Remote(Option<oneshot::Sender<oneshot::Sender<Result<(), ProviderRoomToolError>>>>),
}

impl From<RoomToolReservation> for Admission {
    fn from(reservation: RoomToolReservation) -> Self {
        Self::Local(reservation)
    }
}

impl From<oneshot::Sender<oneshot::Sender<Result<(), ProviderRoomToolError>>>> for Admission {
    fn from(sender: oneshot::Sender<oneshot::Sender<Result<(), ProviderRoomToolError>>>) -> Self {
        Self::Remote(Some(sender))
    }
}

impl Admission {
    fn begin_execution(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), ProviderRoomToolError>> + Send {
        use futures_util::future::Either;
        let sender = match self {
            Self::Local(reservation) => {
                return Either::Left(std::future::ready(reservation.begin_execution()));
            }
            Self::Remote(sender) => sender,
        };
        let Some(sender) = sender.take() else {
            return Either::Left(std::future::ready(Err(tool_error(
                "room_tool_conflict",
                "The room tool reservation was already consumed.",
            ))));
        };
        let (reply, response) = oneshot::channel();
        if sender.send(reply).is_err() {
            return Either::Left(std::future::ready(Err(relay_lost())));
        }
        Either::Right(async move { response.await.map_err(|_| relay_lost())? })
    }

    fn resolve(&mut self, successful: bool) {
        if let Self::Local(reservation) = self {
            reservation.resolve(successful);
        }
    }
}

fn relay_lost() -> ProviderRoomToolError {
    tool_error(
        "room_unavailable",
        "The native tool reservation owner is unavailable.",
    )
}
