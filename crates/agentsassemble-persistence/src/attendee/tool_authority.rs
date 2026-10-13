use agentsassemble_domain::DurableAgentSession;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    AttendeeConnectionAuthorization, PersistenceError, agent_lifecycle::load_session,
    turn_authority::require_room_tool_turn_authority,
};

// The caller validates this connection in the same transaction, including admission custody.
pub(crate) async fn load_turn_in(
    tx: &mut Transaction<'_, Sqlite>,
    connection: &AttendeeConnectionAuthorization,
    turn_generation: u64,
    execution_id: &str,
) -> Result<DurableAgentSession, PersistenceError> {
    let owner = connection.session().principal();
    let session = load_session(tx, &owner.room_id, &owner.participant_id).await?;
    require_room_tool_turn_authority(
        tx,
        &session,
        &session.public.active_turn_id,
        session.input_up_to_seq,
        turn_generation,
        execution_id,
    )
    .await?;
    Ok(session)
}

impl crate::SqliteStore {
    /// Authorizes one attendee file-transfer call under its current connection and turn.
    ///
    /// # Errors
    /// Rejects replaced connections, revoked admission and stale turn custody.
    pub async fn authorize_attendee_file_turn(
        &self,
        fingerprint: &[u8; 32],
        connection_id: Uuid,
        turn_generation: u64,
        execution_id: &str,
    ) -> Result<(AttendeeConnectionAuthorization, DurableAgentSession), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let connection = super::connection::authorize_current_in(
            &mut tx,
            fingerprint,
            connection_id,
            chrono::Utc::now(),
        )
        .await?;
        let session = load_turn_in(&mut tx, &connection, turn_generation, execution_id).await?;
        tx.commit().await?;
        Ok((connection, session))
    }
}
