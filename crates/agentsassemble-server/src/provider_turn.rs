use agentsassemble_domain::RoomInputDeliveryKind;
use agentsassemble_persistence::{
    AgentTurnAssignment, AgentTurnCommit, PersistenceError, ProviderTurnAuthority,
    ProviderTurnEffectPhase, ProviderTurnInterruptEffect, ProviderTurnStartAuthority, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAdapterError, ProviderRoomObservation, ProviderRoomToolIngress,
    ProviderTurnCompleted, ProviderTurnOutcome, ProviderTurnRequest,
};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use tokio::{sync::broadcast, task::JoinSet};

pub(crate) struct ProviderTurnTaskResult {
    pub(crate) assignment: AgentTurnAssignment,
    pub(crate) task_panicked: bool,
    start_authority: Result<ProviderTurnStartAuthority, PersistenceError>,
    result: Option<Result<ProviderTurnCompleted, ProviderAdapterError>>,
}

pub(crate) fn spawn_provider_turn(
    tasks: &mut JoinSet<ProviderTurnTaskResult>,
    store: SqliteStore,
    provider_adapter: ProviderAdapter,
    assignment: AgentTurnAssignment,
    room_tool_ingress: ProviderRoomToolIngress,
) {
    tasks.spawn(async move {
        let failed_assignment = assignment.clone();
        let task = async move {
            let room_observation = matches!(
                assignment.delivery_kind,
                RoomInputDeliveryKind::OrderedObservation
                    | RoomInputDeliveryKind::AmbientObservation
            )
            .then(|| ProviderRoomObservation {
                session_id: assignment.session.public.session_id.clone(),
                input_up_to_seq: assignment.session.input_up_to_seq,
                view: assignment.room_view.clone(),
                allowed_agent_ids: assignment.room_agent_ids.clone(),
                room_tool_ingress: assignment.tabletop_tools.then_some(room_tool_ingress),
            });
            let request = ProviderTurnRequest {
                turn_id: assignment.turn_id.clone(),
                turn_generation: assignment.turn_generation,
                execution_id: assignment.execution_id.clone(),
                input: assignment.provider_input.clone(),
                room_observation,
            };
            let Ok(prepared) = provider_adapter
                .prepare_turn(&assignment.session, &request)
                .await
            else {
                return ProviderTurnTaskResult {
                    assignment,
                    task_panicked: false,
                    start_authority: Err(PersistenceError::CommandUnresolved {
                        code: "provider_turn_prepare_unresolved",
                        message: "The exact provider turn could not acquire runtime ownership."
                            .to_owned(),
                    }),
                    result: None,
                };
            };
            let start_authority = store
                .authorize_provider_turn_start(
                    &assignment.session.public.room_id,
                    &assignment.session.public.session_id,
                    assignment.turn_generation,
                    &assignment.turn_id,
                )
                .await;
            let Ok(_) = &start_authority else {
                provider_adapter.discard_prepared_turn(&prepared).await;
                return ProviderTurnTaskResult {
                    assignment,
                    task_panicked: false,
                    start_authority,
                    result: None,
                };
            };
            let result = provider_adapter
                .send_prepared_turn(prepared, &assignment.session, &request)
                .await;
            ProviderTurnTaskResult {
                assignment,
                task_panicked: false,
                start_authority,
                result: Some(result),
            }
        };
        match AssertUnwindSafe(task).catch_unwind().await {
            Ok(result) => result,
            Err(_) => ProviderTurnTaskResult {
                assignment: failed_assignment,
                task_panicked: true,
                start_authority: Err(PersistenceError::CommandUnresolved {
                    code: "provider_turn_task_failed",
                    message: "The provider turn task ended without a typed result.".to_owned(),
                }),
                result: None,
            },
        }
    });
}

pub(crate) async fn commit_provider_result(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    completed: ProviderTurnTaskResult,
) -> Result<AgentTurnCommit, PersistenceError> {
    let room_id = &completed.assignment.session.public.room_id;
    let session_id = &completed.assignment.session.public.session_id;
    let start_authority = completed.start_authority?;
    let Some(result) = completed.result else {
        return Err(PersistenceError::CommandUnresolved {
            code: "provider_turn_start_unresolved",
            message: "Provider turn start authorization remains unresolved.".to_owned(),
        });
    };
    match result {
        Ok(result) => match result.outcome {
            ProviderTurnOutcome::Message {
                ref content,
                ref target_agent_id,
            } => {
                store
                    .mark_provider_turn_running(&start_authority, &result.provider_turn_id)
                    .await?;
                store
                    .complete_agent_turn(
                        room_id,
                        session_id,
                        turn_authority(&start_authority, &result),
                        content,
                        target_agent_id,
                    )
                    .await
            }
            ProviderTurnOutcome::Declined { ref reason_code } => {
                store
                    .mark_provider_turn_running(&start_authority, &result.provider_turn_id)
                    .await?;
                store
                    .decline_agent_turn(
                        room_id,
                        session_id,
                        turn_authority(&start_authority, &result),
                        reason_code,
                    )
                    .await
            }
        },
        Err(error) => {
            commit_provider_error(
                store,
                provider_adapter,
                room_id,
                session_id,
                &start_authority,
                error,
            )
            .await
        }
    }
}

async fn commit_provider_error(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    room_id: &str,
    session_id: &str,
    start: &ProviderTurnStartAuthority,
    error: ProviderAdapterError,
) -> Result<AgentTurnCommit, PersistenceError> {
    if error.code == "provider_turn_interrupted"
        && !error.effect_uncertain
        && !error.runtime_stopped
    {
        match store
            .provider_turn_interrupt_effect(room_id, session_id, start.turn_generation)
            .await
        {
            Ok(effect) if interrupt_effect_owns_result(start, &effect) => {
                return Ok(AgentTurnCommit {
                    events: Vec::new(),
                    next_assignments: Vec::new(),
                });
            }
            Ok(_)
            | Err(PersistenceError::CommandRejected {
                code: "stale_provider_turn_effect",
                ..
            }) => {}
            Err(persistence_error) => return Err(persistence_error),
        }
    }
    if error.effect_uncertain && !error.runtime_stopped {
        store.mark_provider_turn_recovery_required(start).await?;
        return Err(PersistenceError::CommandUnresolved {
            code: "provider_turn_recovery_required",
            message: "The exact provider turn remains quarantined pending recovery.".to_owned(),
        });
    }
    let confirmed_stop = error.runtime_stopped.then_some((
        error.runtime_handle_id.as_str(),
        error.runtime_owner_id.as_str(),
        error.runtime_lease_token.as_str(),
    ));
    let commit = store
        .fail_agent_turn(
            room_id,
            session_id,
            ProviderTurnAuthority {
                room_id,
                session_id,
                turn_id: &start.turn_id,
                turn_generation: start.turn_generation,
                execution_id: &start.execution_id,
                start_dispatch_nonce: &start.start_dispatch_nonce,
                runtime_handle_id: &start.runtime_handle_id,
                runtime_owner_id: &start.runtime_owner_id,
                runtime_lease_token: &start.runtime_lease_token,
                provider_turn_id: "",
                provider_session_id: None,
            },
            error.code,
            error.message,
            confirmed_stop,
        )
        .await?;
    if error.runtime_stopped {
        provider_adapter
            .release_confirmed_stop(
                room_id,
                session_id,
                &error.runtime_handle_id,
                &error.runtime_owner_id,
                &error.runtime_lease_token,
            )
            .await;
    }
    Ok(commit)
}

fn interrupt_effect_owns_result(
    start: &ProviderTurnStartAuthority,
    effect: &ProviderTurnInterruptEffect,
) -> bool {
    effect.room_id == start.room_id
        && effect.session_id == start.session_id
        && effect.turn_generation == start.turn_generation
        && effect.execution_id == start.execution_id
        && effect.turn_id == start.turn_id
        && effect.start_dispatch_nonce == start.start_dispatch_nonce
        && effect.runtime_handle_id == start.runtime_handle_id
        && effect.runtime_owner_id == start.runtime_owner_id
        && effect.runtime_lease_token == start.runtime_lease_token
        && matches!(
            effect.phase,
            ProviderTurnEffectPhase::IssuedWaitingQuiescence
                | ProviderTurnEffectPhase::InterruptAmbiguous
                | ProviderTurnEffectPhase::RecoveryRequired
                | ProviderTurnEffectPhase::Finalized
        )
}

fn turn_authority<'a>(
    start: &'a ProviderTurnStartAuthority,
    result: &'a ProviderTurnCompleted,
) -> ProviderTurnAuthority<'a> {
    ProviderTurnAuthority {
        room_id: &start.room_id,
        session_id: &start.session_id,
        turn_id: &start.turn_id,
        turn_generation: start.turn_generation,
        execution_id: &start.execution_id,
        start_dispatch_nonce: &start.start_dispatch_nonce,
        runtime_handle_id: &start.runtime_handle_id,
        runtime_owner_id: &start.runtime_owner_id,
        runtime_lease_token: &start.runtime_lease_token,
        provider_turn_id: &result.provider_turn_id,
        provider_session_id: result.provider_session_id.as_deref(),
    }
}

pub(crate) async fn publish_turn_commit(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    tasks: &mut JoinSet<ProviderTurnTaskResult>,
    provider_adapter: ProviderAdapter,
    room_tool_ingress: ProviderRoomToolIngress,
    commit: AgentTurnCommit,
) {
    let room_id = commit
        .events
        .first()
        .map(|event| event.room_id.clone())
        .or_else(|| {
            commit
                .next_assignments
                .first()
                .map(|assignment| assignment.session.public.room_id.clone())
        });
    if let Some(room_id) = room_id
        && let Err(error) =
            crate::event_publication::drain_room_publications(store, event_tx, &room_id).await
    {
        tracing::error!(
            error = ?error,
            room_id,
            "provider-turn events remain durably pending for publication retry"
        );
    }
    for assignment in commit.next_assignments {
        spawn_provider_turn(
            tasks,
            store.clone(),
            provider_adapter.clone(),
            assignment,
            room_tool_ingress.clone(),
        );
    }
}
