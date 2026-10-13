//! Managed file custody reuses the exact attachment-turn authority.
use crate::{PersistenceError, ProviderAttachmentReadAuthority, SqliteStore};
use agentsassemble_domain::{AuthenticatedPrincipal, DurableAgentSession, InviteScope};
use sqlx::{Sqlite, Transaction};

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderFileAuthority {
    pub room_id: String,
    pub session_id: String,
    pub participant_id: String,
    pub turn_id: String,
    pub input_up_to_seq: i64,
    pub turn_generation: u64,
    pub execution_id: String,
}
impl ProviderFileAuthority {
    pub fn read_authority(&self) -> ProviderAttachmentReadAuthority<'_> {
        ProviderAttachmentReadAuthority {
            room_id: &self.room_id,
            session_id: &self.session_id,
            turn_id: &self.turn_id,
            input_up_to_seq: self.input_up_to_seq,
            turn_generation: self.turn_generation,
            execution_id: &self.execution_id,
        }
    }
    pub(crate) async fn resolve(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let session =
            crate::message_attachments::provider_file_session_in(tx, self.read_authority()).await?;
        if session.public.external_owned
            || session.public.process_ownership != "server"
            || session.public.participant_id != self.participant_id
            || !matches!(
                session.public.permission_mode.as_str(),
                "workspace_write" | "full_access"
            )
        {
            return Err(crate::room_turns::support::rejected(
                "permission_denied",
                "This agent has no workspace file access.",
            ));
        }
        let participant =
            crate::room_turns::support::load_participant(tx, &self.room_id, &self.participant_id)
                .await?;
        crate::room_turns::support::provider_room_principal(
            &session,
            &participant,
            InviteScope::ReadWrite,
        )
    }
}

/// Attendee upload custody: the same room/quota/turn checks as managed runs,
/// resolved through the attendee's admission instead of a server-owned runtime.
#[derive(Clone, PartialEq, Eq)]
pub struct AttendeeFileAuthority {
    pub room_id: String,
    pub session_id: String,
    pub participant_id: String,
    pub session_fingerprint: [u8; 32],
    pub connection_id: uuid::Uuid,
    pub turn_generation: u64,
    pub execution_id: String,
}

impl AttendeeFileAuthority {
    pub(crate) async fn resolve(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let connection = crate::attendee::connection::authorize_current_in(
            tx,
            &self.session_fingerprint,
            self.connection_id,
            chrono::Utc::now(),
        )
        .await?;
        let session = crate::attendee::tool_authority::load_turn_in(
            tx,
            &connection,
            self.turn_generation,
            &self.execution_id,
        )
        .await?;
        if session.public.room_id != self.room_id
            || session.public.session_id != self.session_id
            || session.public.participant_id != self.participant_id
        {
            return Err(crate::room_turns::support::rejected(
                "stale_provider_turn",
                "The attendee turn no longer matches its upload custody.",
            ));
        }
        if !matches!(
            session.public.permission_mode.as_str(),
            "workspace_write" | "full_access"
        ) {
            return Err(crate::room_turns::support::rejected(
                "permission_denied",
                "This agent has no workspace file access.",
            ));
        }
        let participant =
            crate::room_turns::support::load_participant(tx, &self.room_id, &self.participant_id)
                .await?;
        crate::room_turns::support::provider_room_principal(
            &session,
            &participant,
            InviteScope::ReadWrite,
        )
    }
}
impl SqliteStore {
    /// Load the configured workspace only under the current attachment-turn custody.
    /// # Errors
    /// Rejects stale turns and lost participant authority.
    pub async fn provider_file_session(
        &self,
        authority: ProviderAttachmentReadAuthority<'_>,
    ) -> Result<DurableAgentSession, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let session =
            crate::message_attachments::provider_file_session_in(&mut tx, authority).await?;
        tx.commit().await?;
        Ok(session)
    }
}

/// Connector transfer custody: the same current room limit, quota and streaming
/// reader as the other AI surfaces, resolved through the connector's session
/// instead of a server-owned turn. The inline base64 path keeps its own fixed
/// 10 MiB bound and never uses this authority.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorFileAuthority {
    pub session_fingerprint: [u8; 32],
    pub room_id: String,
    pub participant_id: String,
}

impl ConnectorFileAuthority {
    pub(crate) async fn resolve(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let current = crate::connector_session::authorize_in(
            tx,
            &self.session_fingerprint,
            chrono::Utc::now(),
        )
        .await?;
        let principal = current.principal();
        if principal.room_id != self.room_id || principal.participant_id != self.participant_id {
            return Err(crate::room_turns::support::rejected(
                "stale_connector_session",
                "The connector session no longer matches its transfer custody.",
            ));
        }
        if !principal.capabilities.message_send {
            return Err(crate::room_turns::support::rejected(
                "permission_denied",
                "This connector session cannot publish room attachments.",
            ));
        }
        crate::message_attachments::require_current_message_writer(tx, principal).await?;
        Ok(principal.clone())
    }

    pub(crate) async fn read_principal(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let current = crate::connector_session::authorize_in(
            tx,
            &self.session_fingerprint,
            chrono::Utc::now(),
        )
        .await?;
        let principal = current.principal();
        if principal.room_id != self.room_id || principal.participant_id != self.participant_id {
            return Err(crate::room_turns::support::rejected(
                "stale_connector_session",
                "The connector session no longer matches its transfer custody.",
            ));
        }
        if !principal.capabilities.room_history {
            return Err(crate::room_turns::support::rejected(
                "permission_denied",
                "This connector session cannot read message history.",
            ));
        }
        Ok(principal.clone())
    }
}
