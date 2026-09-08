//! Native portal reservations remain owned until their exact HTTP result is delivered.
use crate::{AttendeeClientError, AttendeeToolReadResponse, RoomAttendeeClient};
use agentsassemble_persistence::{
    AttendeeRandomRequest, AttendeeToolRead, AttendeeToolReadRequest, MessageAttachment,
};
use agentsassemble_provider::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderRoomToolCommand,
    ProviderRoomToolError, ProviderRoomToolRequest, ProviderRoomToolResult,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use uuid::Uuid;

enum Request {
    Read(AttendeeToolReadRequest),
    Random(AttendeeRandomRequest),
}

pub struct AttendeeToolCall {
    command: ProviderRoomToolCommand,
    request: Request,
}

impl AttendeeToolCall {
    /// Consumes the native reservation once and fixes any random-operation retry identity.
    ///
    /// # Errors
    /// Rejects a native turn reservation that has already closed or lost exact authority.
    pub async fn new(mut command: ProviderRoomToolCommand) -> Result<Self, ProviderRoomToolError> {
        if let Err(error) = command.begin_execution().await {
            command.complete(Err(error.clone()));
            return Err(error);
        }
        let request = match command.request() {
            ProviderRoomToolRequest::Random(random) => Request::Random(AttendeeRandomRequest {
                request_id: Uuid::new_v4(),
                turn_generation: command.turn_generation(),
                execution_id: command.execution_id().to_owned(),
                action: random.room_action().to_owned(),
                payload: random.canonical_payload(),
            }),
            ProviderRoomToolRequest::SearchMessages { query, cursor } => {
                Request::Read(AttendeeToolReadRequest {
                    turn_generation: command.turn_generation(),
                    execution_id: command.execution_id().to_owned(),
                    tool: AttendeeToolRead::SearchMessages {
                        query: query.clone(),
                        cursor: cursor.clone(),
                    },
                })
            }
            ProviderRoomToolRequest::ReadMessageContext { event_id } => {
                Request::Read(AttendeeToolReadRequest {
                    turn_generation: command.turn_generation(),
                    execution_id: command.execution_id().to_owned(),
                    tool: AttendeeToolRead::MessageContext {
                        event_id: event_id.clone(),
                    },
                })
            }
        };
        Ok(Self { command, request })
    }

    /// Sends one attempt with the current connection and the original operation identity.
    /// The caller retains this call on uncertainty; only `complete` releases the native reply.
    ///
    /// # Errors
    /// Preserves server rejection, response uncertainty and mismatched response kinds.
    pub async fn execute(
        &self,
        client: &RoomAttendeeClient,
        connection: Uuid,
    ) -> Result<ProviderRoomToolResult, AttendeeClientError> {
        match &self.request {
            Request::Random(request) => client
                .random(connection, request)
                .await
                .map(ProviderRoomToolResult::Random),
            Request::Read(request) => {
                match (&request.tool, client.read_tool(connection, request).await?) {
                    (
                        AttendeeToolRead::SearchMessages { .. },
                        AttendeeToolReadResponse::SearchMessages { result },
                    ) => Ok(ProviderRoomToolResult::SearchMessages(result)),
                    (
                        AttendeeToolRead::MessageContext { .. },
                        AttendeeToolReadResponse::MessageContext { result },
                    ) => Ok(ProviderRoomToolResult::MessageContext(result)),
                    _ => Err(invalid_response()),
                }
            }
        }
    }

    pub fn complete(self, result: Result<ProviderRoomToolResult, ProviderRoomToolError>) {
        self.command.complete(result);
    }
}

impl RoomAttendeeClient {
    /// Reads the native turn's bound attachment without forwarding local filesystem authority.
    ///
    /// # Errors
    /// Rejects stale room authority, malformed response kind and invalid encoded content.
    pub async fn read_provider_attachment(
        &self,
        connection: Uuid,
        command: &ProviderAttachmentReadCommand,
    ) -> Result<ProviderAttachment, AttendeeClientError> {
        let response = self
            .read_tool(
                connection,
                &AttendeeToolReadRequest {
                    turn_generation: command.turn_generation(),
                    execution_id: command.execution_id().to_owned(),
                    tool: AttendeeToolRead::Attachment {
                        attachment_id: command.attachment_id().to_owned(),
                    },
                },
            )
            .await?;
        let AttendeeToolReadResponse::Attachment {
            metadata,
            content_base64,
        } = response
        else {
            return Err(invalid_response());
        };
        let content = STANDARD
            .decode(content_base64)
            .map_err(|_| invalid_response())?;
        Ok(
            crate::provider_attachment_runtime::into_provider_attachment(MessageAttachment {
                metadata,
                content,
            }),
        )
    }
}

fn invalid_response() -> AttendeeClientError {
    AttendeeClientError::local("invalid_attendee_tool_response")
}
