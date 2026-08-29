use agentsassemble_domain::{InviteScope, is_message_attachment_id};
use agentsassemble_persistence::HumanSessionAuthorization;
use chrono::Utc;

use super::{
    ConsumedRoomHttpTicket, HumanSessionGrantPurpose, IssuedTicket, RoomHttpPurpose,
    TicketAuthority, TicketError, TicketStore, resolve_room_http_authority,
};

pub(crate) enum ConsumedMessageAttachmentReadTicket {
    Local(ConsumedRoomHttpTicket),
    HumanSession(HumanSessionAuthorization),
}

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

    /// Issues one message-attachment upload credential from current writable session authority.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for read-only/expired sessions or an exhausted grant bound.
    pub async fn issue_human_session_message_attachment_upload(
        &self,
        authorization: HumanSessionAuthorization,
    ) -> Result<IssuedTicket, TicketError> {
        if authorization.principal().invite_scope != InviteScope::ReadWrite {
            return Err(TicketError::Invalid);
        }
        self.issue_human_session(
            authorization,
            HumanSessionGrantPurpose::MessageAttachmentUpload,
        )
        .await
    }

    /// Issues one asset-bound read credential from current human-session provenance.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for malformed IDs, expired sessions, or exhausted grant capacity.
    pub async fn issue_human_session_bound_message_attachment_read(
        &self,
        authorization: HumanSessionAuthorization,
        attachment_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if !is_message_attachment_id(&attachment_id) {
            return Err(TicketError::Invalid);
        }
        self.issue_human_session(
            authorization,
            HumanSessionGrantPurpose::BoundMessageAttachmentRead { attachment_id },
        )
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
    ) -> Result<ConsumedMessageAttachmentReadTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        match grant.authority {
            TicketAuthority::RoomHttp(room) => match room.purpose.clone() {
                RoomHttpPurpose::BoundMessageAttachmentRead {
                    attachment_id: expected,
                } if expected == attachment_id => resolve_room_http_authority(
                    room,
                    &RoomHttpPurpose::BoundMessageAttachmentRead {
                        attachment_id: expected,
                    },
                )
                .map(ConsumedMessageAttachmentReadTicket::Local),
                _ => Err(TicketError::Invalid),
            },
            TicketAuthority::HumanSession(session) => Self::resolve_human_session_authority(
                session,
                &HumanSessionGrantPurpose::BoundMessageAttachmentRead {
                    attachment_id: attachment_id.to_owned(),
                },
                Utc::now(),
            )
            .map(ConsumedMessageAttachmentReadTicket::HumanSession),
            TicketAuthority::Room(_)
            | TicketAuthority::LocalRoomManager(_)
            | TicketAuthority::SettingsDirectoryRead { .. }
            | TicketAuthority::ServerOperator { .. }
            | TicketAuthority::CentralRegistration { .. } => Err(TicketError::Invalid),
        }
    }
}
