use agentsassemble_domain::DurableAgentSession;
use sqlx::{Sqlite, Transaction};

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
