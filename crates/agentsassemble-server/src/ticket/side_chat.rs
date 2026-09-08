use super::{
    IssuedTicket, LocalRoomManagerAuthority, LocalRoomManagerPurpose, TicketError, TicketStore,
};

impl TicketStore {
    /// Issues one exact side-chat bootstrap credential for a resolved local human.
    ///
    /// # Errors
    /// Rejects exhausted ticket capacity; the operation revalidates the exact authority.
    pub async fn issue_side_chat_read(
        &self,
        authority: LocalRoomManagerAuthority,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_local_room_manager(authority, LocalRoomManagerPurpose::SideChatRead)
            .await
    }

    pub(crate) async fn consume_side_chat_read(
        &self,
        ticket: &str,
    ) -> Result<LocalRoomManagerAuthority, TicketError> {
        self.consume_local_room_manager(ticket, &LocalRoomManagerPurpose::SideChatRead)
            .await
            .map(|grant| grant.authority)
    }
}
