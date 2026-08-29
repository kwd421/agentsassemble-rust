use std::collections::HashSet;

use agentsassemble_domain::{
    MAX_ATTACHMENT_BYTES, MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES,
    MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS, is_message_attachment_id,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAttachment {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub is_image: bool,
    pub content: Vec<u8>,
}

impl ProviderAttachment {
    pub(crate) fn is_valid(&self) -> bool {
        is_message_attachment_id(&self.id)
            && !self.filename.is_empty()
            && self.filename.chars().count() <= MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS
            && !self.filename.chars().any(char::is_control)
            && !self.content_type.is_empty()
            && self.content_type.len() <= MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES
            && !self.content_type.chars().any(char::is_control)
            && (1..=MAX_ATTACHMENT_BYTES).contains(&self.size)
            && self.content.len() == self.size
    }
}

pub(crate) fn valid_observation_attachments(
    room_view: &str,
    attachment_ids: &[String],
    has_ingress: bool,
) -> bool {
    let unique_ids = attachment_ids.iter().collect::<HashSet<_>>();
    unique_ids.len() == attachment_ids.len()
        && attachment_ids.iter().all(|attachment_id| {
            is_message_attachment_id(attachment_id) && room_view.contains(attachment_id)
        })
        && attachment_ids.is_empty() != has_ingress
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderAttachmentReadAuthority {
    pub(crate) session_id: String,
    pub(crate) turn_id: String,
    pub(crate) input_up_to_seq: i64,
    pub(crate) turn_generation: u64,
    pub(crate) execution_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ProviderAttachmentReadError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProviderAttachmentReadIngress {
    sender: mpsc::Sender<ProviderAttachmentReadCommand>,
}

impl PartialEq for ProviderAttachmentReadIngress {
    fn eq(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}

impl Eq for ProviderAttachmentReadIngress {}

impl ProviderAttachmentReadIngress {
    #[must_use]
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<ProviderAttachmentReadCommand>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender }, receiver)
    }

    pub(crate) async fn read(
        &self,
        authority: ProviderAttachmentReadAuthority,
        attachment_id: String,
    ) -> Result<ProviderAttachment, ProviderAttachmentReadError> {
        let (reply, response) = oneshot::channel();
        let command = ProviderAttachmentReadCommand {
            authority,
            attachment_id,
            reply: Some(reply),
        };
        self.sender.try_send(command).map_err(|_| unavailable())?;
        response.await.unwrap_or_else(|_| Err(unavailable()))
    }
}

#[derive(Debug)]
pub struct ProviderAttachmentReadCommand {
    authority: ProviderAttachmentReadAuthority,
    attachment_id: String,
    reply: Option<oneshot::Sender<Result<ProviderAttachment, ProviderAttachmentReadError>>>,
}

impl ProviderAttachmentReadCommand {
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
        self.authority.turn_generation
    }

    #[must_use]
    pub fn execution_id(&self) -> &str {
        &self.authority.execution_id
    }

    #[must_use]
    pub fn attachment_id(&self) -> &str {
        &self.attachment_id
    }

    pub fn complete(mut self, result: Result<ProviderAttachment, ProviderAttachmentReadError>) {
        let result = result.and_then(|attachment| {
            (attachment.id == self.attachment_id && attachment.is_valid())
                .then_some(attachment)
                .ok_or_else(invalid_response)
        });
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(result);
        }
    }
}

impl Drop for ProviderAttachmentReadCommand {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Err(unavailable()));
        }
    }
}

fn unavailable() -> ProviderAttachmentReadError {
    ProviderAttachmentReadError {
        code: "room_unavailable",
        message: "The room attachment owner is unavailable.".to_owned(),
    }
}

fn invalid_response() -> ProviderAttachmentReadError {
    ProviderAttachmentReadError {
        code: "attachment_invalid",
        message: "The room attachment response is invalid.".to_owned(),
    }
}
