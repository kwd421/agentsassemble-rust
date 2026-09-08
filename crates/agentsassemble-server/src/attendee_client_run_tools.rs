//! Bounded native tool reservations stay live across HTTP uncertainty and socket replacement.
use super::{
    AttendeeClientError, ProviderAttachmentReadIngress, ProviderRoomToolIngress, RoomAttendeeClient,
};
use crate::AttendeeToolCall;
use agentsassemble_provider::{
    ProviderAttachmentReadCommand, ProviderAttachmentReadError, ProviderRoomToolCommand,
    ProviderRoomToolError,
};
use tokio::sync::mpsc;
use uuid::Uuid;

pub(super) struct Tools {
    pub(super) ingress: ProviderRoomToolIngress,
    pub(super) attachments: ProviderAttachmentReadIngress,
    tool_rx: mpsc::Receiver<ProviderRoomToolCommand>,
    attachment_rx: mpsc::Receiver<ProviderAttachmentReadCommand>,
    tool: Option<AttendeeToolCall>,
    attachment: Option<ProviderAttachmentReadCommand>,
}

impl Tools {
    pub(super) fn new() -> Self {
        let (ingress, tool_rx) = ProviderRoomToolIngress::channel(4);
        let (attachments, attachment_rx) = ProviderAttachmentReadIngress::channel(4);
        Self {
            ingress,
            attachments,
            tool_rx,
            attachment_rx,
            tool: None,
            attachment: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.tool = None;
        self.attachment = None;
        while self.tool_rx.try_recv().is_ok() {}
        while self.attachment_rx.try_recv().is_ok() {}
    }

    pub(super) async fn step(
        &mut self,
        client: &RoomAttendeeClient,
        connection: Uuid,
    ) -> Result<(), AttendeeClientError> {
        tokio::select! {
            Some(command) = self.tool_rx.recv(), if self.tool.is_none() => {
                // Rejected stale reservations are already replied to by their existing owner.
                self.tool = AttendeeToolCall::new(command).await.ok();
            }
            Some(command) = self.attachment_rx.recv(), if self.attachment.is_none() => self.attachment = Some(command),
            result = async {
                match &self.tool {
                    Some(tool) => tool.execute(client, connection).await,
                    None => std::future::pending().await,
                }
            } => {
                match result {
                    Err(failure) if failure.is_retryable() => return Err(failure),
                    result => {
                        if let Some(tool) = self.tool.take() {
                            tool.complete(result.map_err(|failure| ProviderRoomToolError {
                                code: "attendee_tool_rejected".into(), message: failure.code,
                            }));
                        }
                    }
                }
            }
            result = async {
                match &self.attachment {
                    Some(command) => client.read_provider_attachment(connection, command).await,
                    None => std::future::pending().await,
                }
            } => {
                match result {
                    Err(failure) if failure.is_retryable() => return Err(failure),
                    result => {
                        if let Some(command) = self.attachment.take() {
                            command.complete(result.map_err(|failure| ProviderAttachmentReadError {
                                code: "attendee_attachment_rejected".into(), message: failure.code,
                            }));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
