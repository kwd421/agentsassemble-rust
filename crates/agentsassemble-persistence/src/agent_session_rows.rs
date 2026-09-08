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

/// Shared initial durable state; runtime custody is supplied only by its actual owner.
pub(crate) fn without_runtime(public: agentsassemble_domain::AgentSession) -> DurableAgentSession {
    DurableAgentSession {
        public,
        executable: String::new(),
        executable_identity: String::new(),
        workspace: String::new(),
        workspace_identity: String::new(),
        provider_endpoint: String::new(),
        runtime_profile_key: String::new(),
        runtime_profile_version: agentsassemble_domain::CURRENT_RUNTIME_PROFILE_VERSION,
        provider_session_id: String::new(),
        runtime_handle_id: String::new(),
        runtime_owner_id: String::new(),
        runtime_lease_token: String::new(),
        turn_generation: 0,
        schedule_requested: false,
        pending_inputs: Vec::new(),
        inflight_inputs: Vec::new(),
        active_source_event_id: String::new(),
        input_up_to_event_id: String::new(),
        input_up_to_seq: 0,
        lifecycle_intent_action: agentsassemble_domain::AgentLifecycleAction::None,
        lifecycle_intent_id: String::new(),
        lifecycle_intent_status: agentsassemble_domain::AgentLifecycleIntentStatus::None,
    }
}
