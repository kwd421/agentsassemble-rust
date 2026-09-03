use agentsassemble_domain::DurableAgentSession;
use sqlx::{Sqlite, Transaction};

use crate::PersistenceError;

pub(crate) async fn load_optional_agent_session_row(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<Option<DurableAgentSession>, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = ? AND session_id = ?",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?;
    encoded
        .map(|encoded| serde_json::from_str(&encoded))
        .transpose()
        .map_err(Into::into)
}

pub(crate) async fn update_agent_session_row(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<u64, PersistenceError> {
    Ok(sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = ? AND session_id = ?",
    )
    .bind(serde_json::to_string(session)?)
    .bind(&session.public.room_id)
    .bind(&session.public.session_id)
    .execute(&mut **transaction)
    .await?
    .rows_affected())
}
