use agentsassemble_domain::is_message_attachment_id;

use super::{ConsumedRoomHttpTicket, IssuedTicket, RoomHttpPurpose, TicketError, TicketStore};

impl TicketStore {
    /// Issues one message-attachment upload credential for a resolved local room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_message_attachment_upload(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::MessageAttachmentUpload,
        )
        .await
    }

    /// Issues one exact bound-message-attachment read credential for a local room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for malformed authority or asset identity, or exhausted capacity.
    pub async fn issue_bound_message_attachment_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
        attachment_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if !is_message_attachment_id(&attachment_id) {
            return Err(TicketError::Invalid);
        }
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::BoundMessageAttachmentRead { attachment_id },
        )
        .await
    }

    /// Consumes one exact message-attachment upload credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a mismatched, expired, unknown, or reused ticket.
    pub(crate) async fn consume_message_attachment_upload(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::MessageAttachmentUpload)
            .await
    }

    /// Consumes one exact asset-bound read credential without probing another authority.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a mismatched, expired, unknown, or reused ticket.
    pub(crate) async fn consume_message_attachment_read(
        &self,
        ticket: &str,
        attachment_id: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(
            ticket,
            &RoomHttpPurpose::BoundMessageAttachmentRead {
                attachment_id: attachment_id.to_owned(),
            },
        )
        .await
    }
}
