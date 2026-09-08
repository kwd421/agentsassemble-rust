use super::{ConsumedRoomHttpTicket, IssuedTicket, RoomHttpPurpose, TicketError, TicketStore};

impl TicketStore {
    /// Issues one exact side-chat bootstrap credential for a resolved local human.
    ///
    /// # Errors
    /// Rejects invalid identity fields and exhausted ticket capacity.
    pub async fn issue_side_chat_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::SideChatRead,
        )
        .await
    }

    pub(crate) async fn consume_side_chat_read(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::SideChatRead)
            .await
    }
}
