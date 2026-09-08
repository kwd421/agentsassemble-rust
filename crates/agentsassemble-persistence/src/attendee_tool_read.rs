use agentsassemble_domain::{InviteScope, RoomMessageContext, RoomMessageSearchPage};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    MessageAttachment, PersistenceError, ProviderAttachmentReadAuthority, SqliteStore,
    attendee_connection::authorize_current_in,
    message_attachments::bound_provider_attachment_in,
    room_turns::support::{load_participant, provider_room_principal},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeToolReadRequest {
    pub turn_generation: u64,
    pub execution_id: String,
    pub tool: AttendeeToolRead,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttendeeToolRead {
    SearchMessages { query: String, cursor: String },
    MessageContext { event_id: String },
    Attachment { attachment_id: String },
}

pub enum AttendeeToolReadResult {
    SearchMessages(RoomMessageSearchPage),
    MessageContext(RoomMessageContext),
    Attachment(MessageAttachment),
}

impl SqliteStore {
    /// Reads only a current external connection's exact turn tools in one transaction.
    /// Room, participant and input boundary are derived from the admission and turn owners.
    ///
    /// # Errors
    /// Rejects stale connections/turns, pending interrupts, revoked permission and invalid targets.
    pub async fn read_attendee_tool(
        &self,
        fingerprint: &[u8; 32],
        connection_id: Uuid,
        request: &AttendeeToolReadRequest,
        now: DateTime<Utc>,
    ) -> Result<AttendeeToolReadResult, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let connection = authorize_current_in(&mut tx, fingerprint, connection_id, now).await?;
        let owner = connection.session().principal();
        let session = crate::attendee_tool_authority::load_turn_in(
            &mut tx,
            &connection,
            request.turn_generation,
            &request.execution_id,
        )
        .await?;
        let participant = load_participant(&mut tx, &owner.room_id, &owner.participant_id).await?;
        let principal = provider_room_principal(&session, &participant, InviteScope::ReadOnly)?;
        let result = match &request.tool {
            AttendeeToolRead::SearchMessages { query, cursor } => {
                AttendeeToolReadResult::SearchMessages(
                    crate::message_search::search_authorized_in(
                        &mut tx,
                        &principal.room_id,
                        "lobby",
                        query,
                        cursor,
                    )
                    .await?,
                )
            }
            AttendeeToolRead::MessageContext { event_id } => {
                AttendeeToolReadResult::MessageContext(
                    crate::message_search::context_authorized_in(
                        &mut tx, &principal, "lobby", event_id,
                    )
                    .await?,
                )
            }
            AttendeeToolRead::Attachment { attachment_id } => AttendeeToolReadResult::Attachment(
                bound_provider_attachment_in(
                    &mut tx,
                    ProviderAttachmentReadAuthority {
                        room_id: &principal.room_id,
                        session_id: &session.public.session_id,
                        turn_id: &session.public.active_turn_id,
                        input_up_to_seq: session.input_up_to_seq,
                        turn_generation: request.turn_generation,
                        execution_id: &request.execution_id,
                    },
                    attachment_id,
                )
                .await?,
            ),
        };
        tx.commit().await?;
        Ok(result)
    }
}
