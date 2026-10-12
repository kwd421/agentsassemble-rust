//! The attendee HTTP tool projection is shared by its server and native client.
use agentsassemble_domain::{RoomMessageContext, RoomMessageSearchPage};
use agentsassemble_persistence::MessageAttachmentMetadata;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DelegatedPinRequest {
    pub turn_generation: u64,
    pub execution_id: String,
    pub request: Option<agentsassemble_domain::SetMessagePin>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttendeeToolReadResponse {
    ConversationStatus {
        result: agentsassemble_domain::ConversationStatus,
    },
    SearchMessages {
        result: RoomMessageSearchPage,
    },
    MessageContext {
        result: RoomMessageContext,
    },
    Attachment {
        metadata: MessageAttachmentMetadata,
        content_base64: String,
    },
}
