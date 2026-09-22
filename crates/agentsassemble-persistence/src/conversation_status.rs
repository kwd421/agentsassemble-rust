use crate::{PersistenceError, ProviderMessageSearchAuthority, RoomMutationAuthority, SqliteStore};
use agentsassemble_domain::{AgentActivity, AuthenticatedPrincipal, ConversationStatus};

impl SqliteStore {
    /// Reads public activity and a bounded open-poll page for a current room session.
    ///
    /// # Errors
    /// Rejects stale authority, absent history permission and invalid cursors.
    pub async fn conversation_status(
        &self,
        authority: RoomMutationAuthority<'_>,
        before_seq: i64,
    ) -> Result<ConversationStatus, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let principal = authority.resolve(&mut tx).await?;
        if !principal.capabilities.room_history {
            return Err(PersistenceError::CommandRejected {
                code: "permission_denied".into(),
                message: "Room history permission is required.".into(),
            });
        }
        let status = read_in(&mut tx, &principal, before_seq).await?;
        tx.commit().await?;
        Ok(status)
    }

    /// Reads public activity for the exact active provider turn without ending it.
    ///
    /// # Errors
    /// Rejects stale turn/participant provenance and invalid cursors.
    pub async fn provider_conversation_status(
        &self,
        authority: ProviderMessageSearchAuthority<'_>,
        before_seq: i64,
    ) -> Result<ConversationStatus, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let principal =
            crate::message_search::provider_search_principal(&mut tx, authority).await?;
        let status = read_in(&mut tx, &principal, before_seq).await?;
        tx.commit().await?;
        Ok(status)
    }
}

pub(crate) async fn read_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal: &AuthenticatedPrincipal,
    before_seq: i64,
) -> Result<ConversationStatus, PersistenceError> {
    let (open_votes, next_before_seq) =
        crate::room_votes::open_vote_page(tx, principal, before_seq).await?;
    let agents = crate::sqlite::load_agent_sessions(tx, &principal.room_id)
        .await?
        .into_iter()
        .map(|session| AgentActivity {
            participant_id: session.participant_id,
            display_name: session.display_name,
            status: session.status,
            runtime_status: session.runtime_status,
            turn_phase: session.turn_phase,
            recovery_required: session.recovery_required,
        })
        .collect();
    Ok(ConversationStatus {
        observed_at: chrono::Utc::now().to_rfc3339(),
        agents,
        open_votes,
        next_before_seq,
    })
}
