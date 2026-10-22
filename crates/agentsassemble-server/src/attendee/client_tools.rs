//! Native portal reservations remain owned until their exact HTTP result is delivered.
use crate::{AttendeeClientError, AttendeeToolReadResponse, RoomAttendeeClient};
use agentsassemble_persistence::{
    AttendeeRandomRequest, AttendeeToolRead, AttendeeToolReadRequest, MESSAGE_UPLOAD_PIECE_BYTES,
    MessageAttachment,
};
use agentsassemble_provider::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderRoomToolCommand,
    ProviderRoomToolError, ProviderRoomToolRequest, ProviderRoomToolResult,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

/// The client machine's file custody for one tool call: its own working folder.
pub struct AttendeeFileContext {
    pub workspace: String,
    pub identity: String,
    pub permission_mode: String,
}

impl AttendeeFileContext {
    fn writable(&self) -> bool {
        matches!(
            self.permission_mode.as_str(),
            "workspace_write" | "full_access"
        )
    }
}

enum Request {
    Upload {
        path: String,
        content_type: String,
        operation_id: String,
    },
    Direct(crate::attendee::tool_wire::DelegatedDirectRequest),
    Pin(crate::attendee::tool_wire::DelegatedPinRequest),
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
            ProviderRoomToolRequest::UploadAttachment { path, content_type } => Request::Upload {
                path: path.clone(),
                content_type: content_type.clone(),
                operation_id: Uuid::new_v4().to_string(),
            },
            ProviderRoomToolRequest::DelegatedActions
            | ProviderRoomToolRequest::DirectAction(_) => {
                Request::Direct(crate::attendee::tool_wire::DelegatedDirectRequest {
                    turn_generation: command.turn_generation(),
                    execution_id: command.execution_id().into(),
                    request: match command.request() {
                        ProviderRoomToolRequest::DirectAction(request) => {
                            Some(request.as_ref().clone())
                        }
                        _ => None,
                    },
                })
            }
            ProviderRoomToolRequest::SetMessagePin(_) => {
                Request::Pin(crate::attendee::tool_wire::DelegatedPinRequest {
                    turn_generation: command.turn_generation(),
                    execution_id: command.execution_id().into(),
                    request: match command.request() {
                        ProviderRoomToolRequest::SetMessagePin(request) => Some(request.clone()),
                        _ => None,
                    },
                })
            }
            ProviderRoomToolRequest::ReadRoomStatus { before_seq } => {
                Request::Read(AttendeeToolReadRequest {
                    turn_generation: command.turn_generation(),
                    execution_id: command.execution_id().to_owned(),
                    tool: AttendeeToolRead::ConversationStatus {
                        before_seq: *before_seq,
                    },
                })
            }
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
        files: &AttendeeFileContext,
    ) -> Result<ProviderRoomToolResult, AttendeeClientError> {
        match &self.request {
            Request::Upload {
                path,
                content_type,
                operation_id,
            } => {
                upload_attachment(
                    client,
                    connection,
                    &self.command,
                    files,
                    path,
                    content_type,
                    operation_id,
                )
                .await
            }
            Request::Direct(request) => {
                let result = client
                    .post("tool/direct", Some(connection), request)
                    .await?;
                if request.request.is_some() {
                    Ok(ProviderRoomToolResult::DirectAction(result))
                } else {
                    serde_json::from_value(result)
                        .map(ProviderRoomToolResult::DelegatedActions)
                        .map_err(|_| invalid_response())
                }
            }
            Request::Pin(request) => {
                let result = client.post("tool/pin", Some(connection), request).await?;
                Ok(ProviderRoomToolResult::MessagePin(result))
            }
            Request::Random(request) => client
                .random(connection, request)
                .await
                .map(ProviderRoomToolResult::Random),
            Request::Read(request) => {
                match (&request.tool, client.read_tool(connection, request).await?) {
                    (
                        AttendeeToolRead::ConversationStatus { .. },
                        AttendeeToolReadResponse::ConversationStatus { result },
                    ) => Ok(ProviderRoomToolResult::ConversationStatus(result)),
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
    /// Reads the native turn's bound attachment, saving larger files into the
    /// client's own working folder instead of holding them in memory.
    ///
    /// # Errors
    /// Rejects stale room authority, malformed response kind and invalid encoded content.
    pub async fn read_provider_attachment(
        &self,
        connection: Uuid,
        command: &ProviderAttachmentReadCommand,
        files: &AttendeeFileContext,
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
            .await;
        let response = match response {
            Ok(response) => response,
            Err(failure) if failure.code == "attachment_too_large" => {
                // Read-only agents keep the explicit overflow error, as on managed runs.
                // Older hosts never advertise transfers and keep the same error.
                if files.writable() && self.attachment_transfers() {
                    return download_to_workspace(
                        self,
                        connection,
                        command,
                        files,
                        command.attachment_id(),
                    )
                    .await;
                }
                return Err(failure);
            }
            Err(failure) => return Err(failure),
        };
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

/// Uploads one regular file from the client's own working folder through the
/// host's transfer owner: size and quota are accepted before any byte is read,
/// and bytes stream in bounded pieces without being held whole in memory.
async fn upload_attachment(
    client: &RoomAttendeeClient,
    connection: Uuid,
    command: &ProviderRoomToolCommand,
    files: &AttendeeFileContext,
    path: &str,
    content_type: &str,
    operation_id: &str,
) -> Result<ProviderRoomToolResult, AttendeeClientError> {
    if !client.attachment_transfers() {
        return Err(AttendeeClientError {
            code: "workspace_upload_unavailable".into(),
            message: "Attendee path uploads are not yet implemented.".into(),
            resolution: None,
        });
    }
    if !files.writable() {
        return Err(AttendeeClientError {
            code: "permission_denied".into(),
            message: "This conversation-only agent has no workspace file access.".into(),
            resolution: None,
        });
    }
    let workspace = files.workspace.clone();
    let identity = files.identity.clone();
    let requested = path.to_owned();
    // Capability resolution confines symlinks exactly as on managed runs; no byte is read here.
    let (file, filename, size) = tokio::task::spawn_blocking(move || {
        agentsassemble_provider::open_workspace_attachment(&workspace, &identity, &requested)
    })
    .await
    .map_err(|_| workspace_failed())?
    .map_err(|_| workspace_failed())?;
    let turn_generation = command.turn_generation();
    let execution_id = command.execution_id().to_owned();
    let begin = client
        .upload_begin(
            connection,
            &json!({
                "turn_generation": turn_generation,
                "execution_id": execution_id,
                "filename": filename,
                "content_type": content_type,
                "declared_size": size,
                "operation_id": operation_id,
            }),
        )
        .await?;
    let handle = transfer_string(&begin, "/transfer/handle")?;
    let mut offset = transfer_usize(&begin, "/transfer/accepted_offset")?;
    if offset > size {
        let _ = client
            .upload_cancel(connection, &handle, turn_generation, &execution_id)
            .await;
        return Err(invalid_response());
    }
    let mut file = tokio::fs::File::from_std(file);
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset as u64))
            .await
            .map_err(|_| workspace_failed())?;
    }
    let remaining = size - offset;
    if remaining > 0 {
        let body = tokio_util::io::ReaderStream::with_capacity(file, MESSAGE_UPLOAD_PIECE_BYTES);
        let status = client
            .upload_data(
                connection,
                &handle,
                offset,
                turn_generation,
                &execution_id,
                reqwest::Body::wrap_stream(body),
            )
            .await;
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                let _ = client
                    .upload_cancel(connection, &handle, turn_generation, &execution_id)
                    .await;
                return Err(error);
            }
        };
        offset = transfer_usize(&status, "/transfer/accepted_offset")?;
        if offset != size {
            let _ = client
                .upload_cancel(connection, &handle, turn_generation, &execution_id)
                .await;
            return Err(invalid_response());
        }
    }
    let finished = client
        .upload_finish(connection, &handle, turn_generation, &execution_id)
        .await;
    let finished = match finished {
        Ok(finished) => finished,
        Err(error) => {
            let _ = client
                .upload_cancel(connection, &handle, turn_generation, &execution_id)
                .await;
            return Err(error);
        }
    };
    let attachment = finished
        .pointer("/transfer/attachment")
        .cloned()
        .ok_or_else(invalid_response)?;
    Ok(ProviderRoomToolResult::UploadAttachment(attachment))
}

/// Streams one large published attachment into the client's working-folder
/// subfolder; a truncated stream never publishes a partial file.
async fn download_to_workspace(
    client: &RoomAttendeeClient,
    connection: Uuid,
    command: &ProviderAttachmentReadCommand,
    files: &AttendeeFileContext,
    attachment_id: &str,
) -> Result<ProviderAttachment, AttendeeClientError> {
    let (metadata, stream) = client
        .download_attachment(
            connection,
            attachment_id,
            command.turn_generation(),
            command.execution_id(),
        )
        .await?;
    let workspace = files.workspace.clone();
    let identity = files.identity.clone();
    let filename = metadata.filename.clone();
    let mut sink = tokio::task::spawn_blocking(move || {
        agentsassemble_provider::WorkspaceAttachmentSink::new(&workspace, &identity, &filename)
    })
    .await
    .map_err(|_| workspace_failed())?
    .map_err(|_| workspace_failed())?;
    tokio::pin!(stream);
    let mut received = 0usize;
    while let Some(next) = stream.next().await {
        let bytes = next?;
        received = received.saturating_add(bytes.len());
        if received > metadata.size {
            return Err(invalid_response());
        }
        sink.file
            .write_all(&bytes)
            .await
            .map_err(|_| workspace_failed())?;
    }
    if received != metadata.size {
        return Err(invalid_response());
    }
    let path = sink.publish().await.map_err(|_| workspace_failed())?;
    let mut attachment =
        crate::provider_attachment_runtime::into_provider_attachment(MessageAttachment {
            metadata,
            content: Vec::new(),
        });
    attachment.path = Some(path);
    Ok(attachment)
}

fn transfer_string(value: &Value, pointer: &str) -> Result<String, AttendeeClientError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(invalid_response)
}

fn transfer_usize(value: &Value, pointer: &str) -> Result<usize, AttendeeClientError> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(invalid_response)
}

fn workspace_failed() -> AttendeeClientError {
    AttendeeClientError {
        code: "workspace_file_failed".into(),
        message:
            "The working-folder file operation failed or the path is outside the working folder."
                .into(),
        resolution: None,
    }
}

fn invalid_response() -> AttendeeClientError {
    AttendeeClientError::local("invalid_attendee_tool_response")
}

impl From<AttendeeClientError> for agentsassemble_provider::ProviderAttachmentReadError {
    fn from(failure: AttendeeClientError) -> Self {
        if matches!(
            failure.code.as_ref(),
            "attachment_too_large" | "attachment_download_budget_exhausted"
        ) {
            Self {
                code: failure.code.into(),
                message: failure.message,
            }
        } else {
            Self {
                code: "attendee_attachment_rejected".into(),
                message: failure.code,
            }
        }
    }
}
