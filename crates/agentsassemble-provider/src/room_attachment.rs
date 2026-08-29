use std::collections::HashSet;

use agentsassemble_domain::{
    MAX_ATTACHMENT_BYTES, MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES,
    MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS, MAX_MESSAGE_ATTACHMENTS_PER_EVENT,
    is_message_attachment_id,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rmcp::model::{CallToolResult, ContentBlock, ResourceContents};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

const MAX_ATTACHMENT_BASE64_BYTES: usize = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;

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
            && self.filename.trim() == self.filename
            && !matches!(self.filename.as_str(), "." | "..")
            && self.filename.chars().count() <= MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS
            && !self.filename.chars().any(char::is_control)
            && !self.filename.contains(['/', '\\'])
            && !self.content_type.is_empty()
            && self.content_type.len() <= MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES
            && !self.content_type.chars().any(char::is_control)
            && (1..=MAX_ATTACHMENT_BYTES).contains(&self.size)
            && self.content.len() == self.size
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProviderAttachmentDescriptor {
    id: String,
    filename: String,
    content_type: String,
    size: usize,
    is_image: bool,
}

pub(crate) fn attachment_tool_result(
    attachment: &ProviderAttachment,
) -> Result<CallToolResult, String> {
    if !attachment.is_valid() {
        return Err("The room attachment response is invalid.".to_owned());
    }
    let descriptor = ProviderAttachmentDescriptor::from(attachment);
    let metadata = serde_json::to_string(&descriptor)
        .map_err(|_| "The room attachment metadata could not be encoded.".to_owned())?;
    let encoded = STANDARD.encode(&attachment.content);
    let media = if attachment.is_image {
        ContentBlock::image(encoded, &attachment.content_type)
    } else {
        ContentBlock::resource(
            ResourceContents::blob(encoded, attachment_uri(&attachment.id))
                .with_mime_type(&attachment.content_type),
        )
    };
    Ok(CallToolResult::success(vec![
        ContentBlock::text(metadata),
        media,
    ]))
}

pub(crate) fn attachment_from_tool_result(
    result: &CallToolResult,
) -> Result<ProviderAttachment, &'static str> {
    if result.is_error == Some(true) {
        return Err("room helper action was rejected");
    }
    let [metadata, media] = result.content.as_slice() else {
        return Err("room helper returned an invalid attachment");
    };
    let metadata = metadata
        .as_text()
        .ok_or("room helper returned invalid attachment metadata")?;
    let descriptor: ProviderAttachmentDescriptor = serde_json::from_str(&metadata.text)
        .map_err(|_| "room helper returned invalid attachment metadata")?;
    let encoded = match media {
        ContentBlock::Image(image)
            if descriptor.is_image && image.mime_type == descriptor.content_type =>
        {
            image.data.as_str()
        }
        ContentBlock::Resource(resource) if !descriptor.is_image => match &resource.resource {
            ResourceContents::BlobResourceContents {
                uri,
                mime_type: Some(content_type),
                blob,
                ..
            } if uri == &attachment_uri(&descriptor.id)
                && content_type == &descriptor.content_type =>
            {
                blob.as_str()
            }
            _ => return Err("room helper returned invalid attachment content"),
        },
        _ => return Err("room helper returned invalid attachment content"),
    };
    if encoded.is_empty() || encoded.len() > MAX_ATTACHMENT_BASE64_BYTES {
        return Err("room helper returned invalid attachment content");
    }
    let content = STANDARD
        .decode(encoded)
        .map_err(|_| "room helper returned invalid attachment content")?;
    let attachment = ProviderAttachment {
        id: descriptor.id,
        filename: descriptor.filename,
        content_type: descriptor.content_type,
        size: descriptor.size,
        is_image: descriptor.is_image,
        content,
    };
    attachment
        .is_valid()
        .then_some(attachment)
        .ok_or("room helper returned invalid attachment content")
}

impl From<&ProviderAttachment> for ProviderAttachmentDescriptor {
    fn from(attachment: &ProviderAttachment) -> Self {
        Self {
            id: attachment.id.clone(),
            filename: attachment.filename.clone(),
            content_type: attachment.content_type.clone(),
            size: attachment.size,
            is_image: attachment.is_image,
        }
    }
}

fn attachment_uri(attachment_id: &str) -> String {
    format!("agentsassemble://room-attachment/{attachment_id}")
}

pub(crate) fn valid_observation_attachments(
    room_view: &str,
    attachment_ids: &[String],
    has_ingress: bool,
) -> bool {
    let unique_ids = attachment_ids.iter().collect::<HashSet<_>>();
    attachment_ids.len() <= MAX_MESSAGE_ATTACHMENTS_PER_EVENT
        && unique_ids.len() == attachment_ids.len()
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
