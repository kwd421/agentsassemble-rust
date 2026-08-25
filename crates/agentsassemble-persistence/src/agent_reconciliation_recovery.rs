use std::collections::BTreeMap;

use agentsassemble_domain::DurableAgentSession;
use chrono::Utc;
use serde_json::json;

use crate::{
    LiveRuntimeReconciliation, PersistenceError, RuntimeReconciliationCandidate,
    RuntimeReconciliationObservation, SqliteStore,
    agent_lifecycle::clear_intent,
    agent_lifecycle_events::{
        append_error_event, append_session_event, append_state_event, store_result,
    },
    agent_lifecycle_reservations::{
        LifecycleReservation, finish_lifecycle_command, reject_lifecycle_command,
    },
    agent_reconciliation::{
        detach_participant, invalid_observation, invalid_stored_authority, load_candidate,
        reconcile_gone, reconcile_observation, save_reconciled_session, stale_candidate,
        validate_adoption, validate_uncertain_lease,
    },
};

const RECOVERED_START_CODE: &str = "runtime_start_recovered_gone";
const RECOVERED_START_MESSAGE: &str = "The original provider start did not complete before server recovery. Retry with a new request.";
const ABANDONED_START_CODE: &str = "runtime_start_abandoned_before_effect";
const ABANDONED_START_MESSAGE: &str = "The provider start owner ended before authorizing an external effect. Retry with a new request.";
const ABANDONED_STOP_CODE: &str = "runtime_stop_abandoned_before_effect";
const ABANDONED_STOP_MESSAGE: &str = "The provider stop owner ended before authorizing an external effect. Retry with a new request.";

pub(crate) async fn reject_abandoned_lifecycle_before_effect(
    store: &SqliteStore,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<(), PersistenceError> {
    let mut transaction = store.pool.begin().await?;
    let current = current_candidate(&mut transaction, candidate).await?;
    reject_abandoned_in_transaction(&mut transaction, current).await?;
    transaction.commit().await?;
    Ok(())
}

async fn reject_abandoned_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    current: RuntimeReconciliationCandidate,
) -> Result<(), PersistenceError> {
    if current.session.lifecycle_intent_status != "prepared" {
        return Err(stale_candidate());
    }
    let reservation = current
        .reservation
        .as_ref()
        .ok_or_else(invalid_stored_authority)?;
    let (code, message) = match current.session.lifecycle_intent_action.as_str() {
        "start" => (ABANDONED_START_CODE, ABANDONED_START_MESSAGE),
        "stop" => (ABANDONED_STOP_CODE, ABANDONED_STOP_MESSAGE),
        _ => return Err(invalid_stored_authority()),
    };
    reject_lifecycle_command(transaction, &reservation_ref(reservation), code, message).await?;
    let mut session = current.session;
    if session.lifecycle_intent_action == "start"
        && session.runtime_handle_id.is_empty()
        && session.runtime_owner_id.is_empty()
    {
        "unavailable".clone_into(&mut session.public.status);
        session.public.enabled = false;
        "error".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.provider_session_reused = false;
    }
    message.clone_into(&mut session.public.last_error);
    code.clone_into(&mut session.public.last_error_code);
    session.public.recovery_required = false;
    clear_intent(&mut session);
    session.public.updated_at = Utc::now();
    save_reconciled_session(transaction, &session).await?;
    append_error_event(
        transaction,
        &reservation.principal,
        &session.public,
        code,
        message,
    )
    .await?;
    append_state_event(transaction, &reservation.principal, &session.public).await?;
    Ok(())
}

pub(crate) async fn apply_startup_reconciliation(
    store: &SqliteStore,
    candidate: &RuntimeReconciliationCandidate,
    observation: &RuntimeReconciliationObservation,
) -> Result<(), PersistenceError> {
    let mut transaction = store.pool.begin().await?;
    let current = current_candidate(&mut transaction, candidate).await?;
    if current.session.lifecycle_intent_status == "prepared" {
        reject_abandoned_in_transaction(&mut transaction, current).await?;
        transaction.commit().await?;
        return Ok(());
    }
    let mut session = current.session.clone();
    if recovered_stop_is_terminal(&session, observation) {
        finalize_recovered_stop(&mut transaction, &mut session, &current).await?;
        transaction.commit().await?;
        return Ok(());
    }
    if observation == &RuntimeReconciliationObservation::Gone
        && session.lifecycle_intent_action == "start"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "effect_inflight" | "unconfirmed"
        )
    {
        reject_recovered_start(&mut transaction, &mut session, &current).await?;
        transaction.commit().await?;
        return Ok(());
    }
    let detach = reconcile_observation(&mut session, observation)?;
    save_reconciled_session(&mut transaction, &session).await?;
    if detach {
        detach_participant(
            &mut transaction,
            &session.public.room_id,
            &session.public.participant_id,
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn apply_live_reconciliation(
    store: &SqliteStore,
    candidate: &RuntimeReconciliationCandidate,
    observation: &RuntimeReconciliationObservation,
) -> Result<LiveRuntimeReconciliation, PersistenceError> {
    let mut transaction = store.pool.begin().await?;
    let current = current_candidate(&mut transaction, candidate).await?;
    let Some(reservation) = &current.reservation else {
        return Err(invalid_stored_authority());
    };
    if reservation.supervisor_generation != store.runtime_generation() {
        return Err(PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            message: "The original provider effect belongs to a previous server runtime and awaits server-owned reconciliation.".to_owned(),
        });
    }
    let mut session = current.session;
    if !matches!(
        session.lifecycle_intent_status.as_str(),
        "effect_inflight" | "unconfirmed"
    ) || session.lifecycle_intent_id != reservation.operation_id
    {
        return Err(stale_candidate());
    }
    let retry = match observation {
        RuntimeReconciliationObservation::Gone => {
            let detach = reconcile_gone(&mut session)?;
            save_reconciled_session(&mut transaction, &session).await?;
            if detach {
                detach_participant(
                    &mut transaction,
                    &session.public.room_id,
                    &session.public.participant_id,
                )
                .await?;
            }
            true
        }
        RuntimeReconciliationObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        } if session.lifecycle_intent_action == "start" => {
            validate_adoption(
                &session,
                handle_id,
                previous_owner_id,
                new_owner_id,
                runtime_profile_key,
            )?;
            session.runtime_handle_id.clone_from(handle_id);
            session.runtime_owner_id.clone_from(new_owner_id);
            "available".clone_into(&mut session.public.status);
            session.public.enabled = true;
            "starting".clone_into(&mut session.public.runtime_status);
            session.public.provider_session_active = false;
            session.public.last_error.clear();
            session.public.last_error_code.clear();
            session.public.recovery_required = false;
            "prepared".clone_into(&mut session.lifecycle_intent_status);
            session.public.updated_at = Utc::now();
            save_reconciled_session(&mut transaction, &session).await?;
            true
        }
        RuntimeReconciliationObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        } => {
            validate_adoption(
                &session,
                handle_id,
                previous_owner_id,
                new_owner_id,
                runtime_profile_key,
            )?;
            false
        }
        RuntimeReconciliationObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        } => {
            validate_uncertain_lease(&session, handle_id, owner_id, reason_code)?;
            false
        }
        RuntimeReconciliationObservation::Ambiguous { reason_code } => {
            if reason_code.is_empty() {
                return Err(invalid_observation());
            }
            false
        }
    };
    transaction.commit().await?;
    Ok(if retry {
        LiveRuntimeReconciliation::RetryOriginalEffect
    } else {
        LiveRuntimeReconciliation::StillUnresolved
    })
}

async fn current_candidate(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<RuntimeReconciliationCandidate, PersistenceError> {
    let current = load_candidate(
        transaction,
        &candidate.session.public.room_id,
        &candidate.session.public.session_id,
    )
    .await?
    .ok_or_else(stale_candidate)?;
    if current.cas_token != candidate.cas_token || current.session != candidate.session {
        return Err(stale_candidate());
    }
    Ok(current)
}

fn recovered_stop_is_terminal(
    session: &DurableAgentSession,
    observation: &RuntimeReconciliationObservation,
) -> bool {
    session.lifecycle_intent_action == "stop"
        && (session.lifecycle_intent_status == "effect_applied"
            || (observation == &RuntimeReconciliationObservation::Gone
                && matches!(
                    session.lifecycle_intent_status.as_str(),
                    "prepared" | "effect_inflight" | "unconfirmed"
                )))
}

async fn reject_recovered_start(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session: &mut DurableAgentSession,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<(), PersistenceError> {
    let reservation = candidate
        .reservation
        .as_ref()
        .ok_or_else(invalid_stored_authority)?;
    reject_lifecycle_command(
        transaction,
        &reservation_ref(reservation),
        RECOVERED_START_CODE,
        RECOVERED_START_MESSAGE,
    )
    .await?;
    "unavailable".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "error".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    RECOVERED_START_MESSAGE.clone_into(&mut session.public.last_error);
    RECOVERED_START_CODE.clone_into(&mut session.public.last_error_code);
    session.public.recovery_required = false;
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    clear_intent(session);
    session.public.updated_at = Utc::now();
    save_reconciled_session(transaction, session).await?;
    append_error_event(
        transaction,
        &reservation.principal,
        &session.public,
        RECOVERED_START_CODE,
        RECOVERED_START_MESSAGE,
    )
    .await?;
    append_state_event(transaction, &reservation.principal, &session.public).await?;
    Ok(())
}

async fn finalize_recovered_stop(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session: &mut DurableAgentSession,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<(), PersistenceError> {
    let reservation = candidate
        .reservation
        .as_ref()
        .ok_or_else(invalid_stored_authority)?;
    finish_lifecycle_command(transaction, &reservation_ref(reservation)).await?;
    session.pending_inputs = crate::turn_queue::merge_room_inputs(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    )
    .map_err(|_| invalid_stored_authority())?;
    session.inflight_inputs.clear();
    "detached".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "stopped".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.active_source_event_id.clear();
    session.input_up_to_event_id.clear();
    session.input_up_to_seq = 0;
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    clear_intent(session);
    session.public.updated_at = Utc::now();
    save_reconciled_session(transaction, session).await?;
    detach_participant(
        transaction,
        &session.public.room_id,
        &session.public.participant_id,
    )
    .await?;
    let detached = append_session_event(
        transaction,
        &reservation.principal,
        &session.public,
        "session_detached",
        BTreeMap::from([("reason".to_owned(), json!("operator stop"))]),
    )
    .await?;
    let state = append_state_event(transaction, &reservation.principal, &session.public).await?;
    let events = vec![detached, state];
    let result = json!({
        "agent_session": session.public,
        "process": {
            "stopped": true,
            "alive": false,
            "ownership": "server",
            "confirmed": true,
        },
        "revoked_sessions": 0,
        "events": events,
        "event": events.last(),
    });
    store_result(
        transaction,
        &reservation.principal,
        &reservation.request_id,
        &reservation.action,
        reservation.payload_hash.clone(),
        result,
        events,
    )
    .await?;
    Ok(())
}

fn reservation_ref(
    reservation: &crate::RuntimeReconciliationReservation,
) -> LifecycleReservation<'_> {
    LifecycleReservation {
        principal: &reservation.principal,
        request_id: &reservation.request_id,
        action: &reservation.action,
        payload_hash: &reservation.payload_hash,
        session_id: &reservation.session_id,
        operation_id: &reservation.operation_id,
        phase: &reservation.phase,
        prepared_result_json: &reservation.prepared_result_json,
    }
}
