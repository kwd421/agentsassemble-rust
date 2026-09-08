use agentsassemble_domain::{
    AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeStatus, AuthenticatedPrincipal,
    DurableAgentSession, RoomEvent,
};
use chrono::Utc;
use sqlx::{Sqlite, Transaction};

use crate::{
    AgentTurnCommit, PersistenceError,
    agent_lifecycle::{load_session, save_session},
    agent_lifecycle_authority::{lifecycle_intent_is_empty, require_matching_operation},
    agent_lifecycle_reservations::LifecycleReservation,
    attendee_invites::rejected,
    room_runtime_cleanup::{cleanup_exists, request_runtime_cleanup},
};

// Server restart does not transfer ownership of an already requested external effect.
pub(crate) async fn owns_pending_reservation(
    tx: &mut Transaction<'_, Sqlite>,
    reservation: &LifecycleReservation<'_>,
) -> Result<bool, PersistenceError> {
    if reservation.action != "agent.stop" {
        return Ok(false);
    }
    let session = load_session(tx, &reservation.principal.room_id, reservation.session_id).await?;
    Ok(session.public.external_owned
        && session.public.process_ownership == "external"
        && session.lifecycle_intent_action == AgentLifecycleAction::Stop
        && session.lifecycle_intent_id == reservation.operation_id
        && session.lifecycle_intent_status == AgentLifecycleIntentStatus::EffectInflight
        && cleanup_exists(tx, &session.public.room_id, &session.public.session_id).await?)
}

pub(crate) async fn prepare_in(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    session: &mut DurableAgentSession,
    operation_id: &str,
) -> Result<Vec<RoomEvent>, PersistenceError> {
    if !lifecycle_intent_is_empty(session) {
        require_matching_operation(session, AgentLifecycleAction::Stop, operation_id)?;
        if session.lifecycle_intent_status != AgentLifecycleIntentStatus::EffectInflight
            || !cleanup_exists(tx, &session.public.room_id, &session.public.session_id).await?
        {
            return Err(invalid_stop());
        }
        return Ok(Vec::new());
    }
    session.lifecycle_intent_action = AgentLifecycleAction::Stop;
    operation_id.clone_into(&mut session.lifecycle_intent_id);
    session.lifecycle_intent_status = AgentLifecycleIntentStatus::EffectInflight;
    session.public.runtime_status = AgentRuntimeStatus::Stopping;
    request_runtime_cleanup(tx, session).await?;
    let event =
        crate::agent_lifecycle_events::append_state_event(tx, principal, &session.public).await?;
    Ok(vec![event])
}

// Called only after the sealed cleanup owner has matched the exact positive runtime report.
// The stored principal identifies the original committed command; it grants no new operation.
pub(crate) async fn confirm_in(
    tx: &mut Transaction<'_, Sqlite>,
    mut session: DurableAgentSession,
) -> Result<AgentTurnCommit, PersistenceError> {
    if session.lifecycle_intent_status != AgentLifecycleIntentStatus::EffectInflight {
        return Err(invalid_stop());
    }
    let rows = crate::agent_reconciliation::load_pending_reservations(
        tx,
        &session.public.room_id,
        &session.public.session_id,
    )
    .await?;
    let reservation = crate::agent_reconciliation::validate_candidate_authority(&session, &rows)?
        .ok_or_else(invalid_stop)?;
    let terminal = crate::provider_turn_stop::terminalize_confirmed_stop_turn(tx, &session).await?;
    session.lifecycle_intent_status = AgentLifecycleIntentStatus::EffectApplied;
    session.public.updated_at = Utc::now();
    // Confirmed absence also ends reuse custody, including an idle reconnected provider.
    session.public.provider_session_reused = false;
    save_session(tx, &session).await?;
    let outcome = crate::agent_stop_lifecycle::finalize_stop_in(
        tx,
        &reservation.principal,
        &reservation.request_id,
        &reservation.payload,
    )
    .await?;
    let mut events = terminal.into_iter().collect::<Vec<_>>();
    events.extend(outcome.events);
    Ok(AgentTurnCommit {
        events,
        next_assignments: Vec::new(),
    })
}

fn invalid_stop() -> PersistenceError {
    rejected(
        "external_stop_changed",
        "The pending external stop authority changed.",
    )
}
