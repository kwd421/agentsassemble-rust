use super::{
    AgentTurnCommit, ProviderTurnAuthority, apply_provider_session_transition,
    finalization::{ProviderTurnDisposition, ProviderTurnFinalization},
    scheduler::assign_available_pending,
    support::{
        agent_final_event, clear_active_turn_fields, error_event, load_active_room,
        load_participant, provider_room_principal, public_error_code, rejected,
        require_active_turn, session_state_event, turn_finished_event, validate_identifier,
        validate_input_cursor,
    },
    validate_publication_target,
};
use crate::{
    PersistenceError,
    agent_lifecycle::{load_session, save_session},
    room_event_sequence::next_sequence,
    turn_queue::merge_room_inputs,
};
use agentsassemble_domain::{
    AgentRuntimeStatus, AgentSessionStatus, AgentTurnPhase, InviteScope, MAX_MESSAGE_CHARACTERS,
    VoteCommand, clean_message, has_visible_text, redact_persisted_diagnostic_text,
};
use chrono::Utc;

pub(super) async fn complete_message(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    session_id: &str,
    authority: ProviderTurnAuthority<'_>,
    content: &str,
    target_agent_id: &str,
) -> Result<AgentTurnCommit, PersistenceError> {
    let ProviderTurnAuthority {
        turn_id,
        provider_turn_id,
        provider_session_id,
        ..
    } = authority;
    validate_identifier(provider_turn_id, "provider_turn_invalid")?;
    let content = clean_message(content, MAX_MESSAGE_CHARACTERS);
    if !has_visible_text(&content) {
        return Err(rejected(
            "provider_turn_output_missing",
            "The provider turn completed without a room-visible final message.",
        ));
    }
    let (room, settings) = load_active_room(transaction, room_id).await?;
    let mut session = load_session(transaction, room_id, session_id).await?;
    require_active_turn(&session, turn_id)?;
    crate::provider_turn_execution::terminalize_ordinary_execution(
        transaction,
        &session,
        authority,
        crate::ProviderTurnExecutionPhase::Completed,
    )
    .await?;
    validate_input_cursor(transaction, &session).await?;
    validate_publication_target(transaction, &session, target_agent_id).await?;
    apply_provider_session_transition(&mut session, provider_session_id)?;
    let source_event_id = session.active_source_event_id.clone();
    let final_event = agent_final_event(
        transaction,
        &session,
        turn_id,
        provider_turn_id,
        &source_event_id,
        content,
        target_agent_id,
    )
    .await?;
    let commit = ProviderTurnFinalization {
        room: &room,
        settings: &settings,
        turn_id,
        provider_turn_id,
        disposition: ProviderTurnDisposition::Completed {
            route_first_event: true,
        },
    }
    .apply(transaction, &mut session, vec![final_event])
    .await?;
    Ok(commit)
}

pub(super) async fn complete_vote(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    session_id: &str,
    authority: ProviderTurnAuthority<'_>,
    command: VoteCommand,
) -> Result<AgentTurnCommit, PersistenceError> {
    let ProviderTurnAuthority {
        turn_id,
        provider_turn_id,
        provider_session_id,
        ..
    } = authority;
    validate_identifier(provider_turn_id, "provider_turn_invalid")?;
    if matches!(&command, VoteCommand::Create(create) if !create.attachment_ids.is_empty()) {
        return Err(rejected(
            "invalid_vote",
            "Agent Session votes cannot bind browser upload custody.",
        ));
    }
    let route_to_floor = matches!(command, VoteCommand::Create(_));
    let (room, settings) = load_active_room(transaction, room_id).await?;
    let mut session = load_session(transaction, room_id, session_id).await?;
    require_active_turn(&session, turn_id)?;
    validate_input_cursor(transaction, &session).await?;
    apply_provider_session_transition(&mut session, provider_session_id)?;
    let participant = load_participant(
        transaction,
        &session.public.room_id,
        &session.public.participant_id,
    )
    .await?;
    let principal = provider_room_principal(&session, &participant, InviteScope::ReadWrite)?;
    let sequence = next_sequence(transaction, room_id).await?;
    let vote_result = crate::room_votes::apply_vote_command(
        transaction,
        &principal,
        &participant,
        command,
        sequence,
        Utc::now(),
    )
    .await;
    let commit = match vote_result {
        Ok(event) => {
            crate::provider_turn_execution::terminalize_ordinary_execution(
                transaction,
                &session,
                authority,
                crate::ProviderTurnExecutionPhase::Completed,
            )
            .await?;
            ProviderTurnFinalization {
                room: &room,
                settings: &settings,
                turn_id,
                provider_turn_id,
                disposition: ProviderTurnDisposition::Completed {
                    route_first_event: route_to_floor,
                },
            }
            .apply(transaction, &mut session, vec![event])
            .await?
        }
        Err(error) if crate::room_votes::is_terminal_vote_rejection(&error) => {
            let PersistenceError::CommandRejected { code, message } = error else {
                unreachable!("terminal vote rejections are command rejections")
            };
            crate::provider_turn_execution::terminalize_ordinary_execution(
                transaction,
                &session,
                authority,
                crate::ProviderTurnExecutionPhase::Failed,
            )
            .await?;
            ProviderTurnFinalization {
                room: &room,
                settings: &settings,
                turn_id,
                provider_turn_id,
                disposition: ProviderTurnDisposition::Rejected {
                    error_code: &code,
                    message: &message,
                },
            }
            .apply(transaction, &mut session, Vec::new())
            .await?
        }
        Err(error) => return Err(error),
    };
    Ok(commit)
}

pub(super) async fn decline(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    session_id: &str,
    authority: ProviderTurnAuthority<'_>,
    reason_code: &str,
) -> Result<AgentTurnCommit, PersistenceError> {
    let ProviderTurnAuthority {
        turn_id,
        provider_turn_id,
        provider_session_id,
        ..
    } = authority;
    validate_identifier(provider_turn_id, "provider_turn_invalid")?;
    if !matches!(
        reason_code,
        "nothing_useful_to_add" | "not_addressed" | "duplicate"
    ) {
        return Err(rejected(
            "invalid_decline_reason",
            "The provider decline reason is unsupported.",
        ));
    }
    let (room, settings) = load_active_room(transaction, room_id).await?;
    let mut session = load_session(transaction, room_id, session_id).await?;
    require_active_turn(&session, turn_id)?;
    crate::provider_turn_execution::terminalize_ordinary_execution(
        transaction,
        &session,
        authority,
        crate::ProviderTurnExecutionPhase::Declined,
    )
    .await?;
    validate_input_cursor(transaction, &session).await?;
    apply_provider_session_transition(&mut session, provider_session_id)?;
    let commit = ProviderTurnFinalization {
        room: &room,
        settings: &settings,
        turn_id,
        provider_turn_id,
        disposition: ProviderTurnDisposition::Declined { reason_code },
    }
    .apply(transaction, &mut session, Vec::new())
    .await?;
    Ok(commit)
}

pub(super) async fn fail(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    session_id: &str,
    authority: ProviderTurnAuthority<'_>,
    error_code: &str,
    message: &str,
    confirmed_runtime_stop: Option<(&str, &str, &str)>,
) -> Result<AgentTurnCommit, PersistenceError> {
    let turn_id = authority.turn_id;
    let (room, settings) = load_active_room(transaction, room_id).await?;
    let mut session = load_session(transaction, room_id, session_id).await?;
    require_active_turn(&session, turn_id)?;
    crate::provider_turn_execution::terminalize_ordinary_execution(
        transaction,
        &session,
        authority,
        crate::ProviderTurnExecutionPhase::Failed,
    )
    .await?;
    if let Some((handle_id, owner_id, lease_token)) = confirmed_runtime_stop {
        if handle_id.is_empty()
            || owner_id.is_empty()
            || lease_token.is_empty()
            || session.runtime_handle_id != handle_id
            || session.runtime_owner_id != owner_id
            || session.runtime_lease_token != lease_token
        {
            return Err(rejected(
                "stale_provider_turn",
                "Confirmed provider shutdown does not match durable turn authority.",
            ));
        }
        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        session.runtime_lease_token.clear();
        session.public.provider_session_active = false;
        session.public.provider_session_reused = false;
    }
    let code = public_error_code(error_code);
    let message = clean_message(&redact_persisted_diagnostic_text(message, 512), 512);
    let message = if has_visible_text(&message) {
        message
    } else {
        "Provider turn failed.".to_owned()
    };
    let error = error_event(transaction, &session, turn_id, code, &message).await?;
    let finished = turn_finished_event(transaction, &session, turn_id, "error", None, None).await?;
    session.pending_inputs = merge_room_inputs(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    )
    .map_err(|_| {
        rejected(
            "stored_turn_authority_invalid",
            "Stored Agent Session turn queue authority is inconsistent or oversized.",
        )
    })?;
    session.inflight_inputs.clear();
    session.public.status = AgentSessionStatus::Error;
    session.public.runtime_status = AgentRuntimeStatus::Error;
    session.public.turn_phase = AgentTurnPhase::None;
    session.public.active_turn_id.clear();
    session.public.last_error = message;
    session.public.last_error_code = code.to_owned();
    session.public.recovery_required = true;
    clear_active_turn_fields(&mut session);
    session.public.updated_at = Utc::now();
    save_session(transaction, &session).await?;
    let state = session_state_event(transaction, &session).await?;
    let prepared = assign_available_pending(transaction, &room, &settings).await?;
    let mut events = vec![error, finished, state];
    let mut next_assignments = Vec::with_capacity(prepared.len());
    for item in prepared {
        events.extend(item.events);
        next_assignments.push(item.assignment);
    }
    Ok(AgentTurnCommit {
        events,
        next_assignments,
    })
}
