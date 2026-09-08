//! The attendee HTTP tool projection is shared by its server and native client.
use agentsassemble_domain::{RoomMessageContext, RoomMessageSearchPage};
use agentsassemble_persistence::MessageAttachmentMetadata;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttendeeToolReadResponse {
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
