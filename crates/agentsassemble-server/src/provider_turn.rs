use agentsassemble_domain::RoomInputDeliveryKind;
use agentsassemble_persistence::{
    AgentTurnAssignment, AgentTurnCommit, PersistenceError, ProviderTurnAuthority, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAdapterError, ProviderRoomObservation, ProviderRoomToolIngress,
    ProviderTurnCompleted, ProviderTurnOutcome, ProviderTurnRequest,
};
use tokio::{sync::broadcast, task::JoinSet};

pub(crate) struct ProviderTurnTaskResult {
    pub(crate) assignment: AgentTurnAssignment,
    result: Result<ProviderTurnCompleted, ProviderAdapterError>,
}

pub(crate) fn spawn_provider_turn(
    tasks: &mut JoinSet<ProviderTurnTaskResult>,
    provider_adapter: ProviderAdapter,
    assignment: AgentTurnAssignment,
    room_tool_ingress: ProviderRoomToolIngress,
) {
    tasks.spawn(async move {
        let room_observation = matches!(
            assignment.delivery_kind,
            RoomInputDeliveryKind::OrderedObservation | RoomInputDeliveryKind::AmbientObservation
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
            input: assignment.provider_input.clone(),
            room_observation,
        };
        let result = provider_adapter
            .send_turn(&assignment.session, &request)
            .await;
        ProviderTurnTaskResult { assignment, result }
    });
}

pub(crate) async fn commit_provider_result(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    completed: ProviderTurnTaskResult,
) -> Result<AgentTurnCommit, PersistenceError> {
    let room_id = &completed.assignment.session.public.room_id;
    let session_id = &completed.assignment.session.public.session_id;
    let turn_id = &completed.assignment.turn_id;
    match completed.result {
        Ok(result) => match result.outcome {
            ProviderTurnOutcome::Message {
                content,
                target_agent_id,
            } => {
                store
                    .complete_agent_turn(
                        room_id,
                        session_id,
                        ProviderTurnAuthority {
                            turn_id,
                            provider_turn_id: &result.provider_turn_id,
                            provider_session_id: result.provider_session_id.as_deref(),
                        },
                        &content,
                        &target_agent_id,
                    )
                    .await
            }
            ProviderTurnOutcome::Declined { reason_code } => {
                store
                    .decline_agent_turn(
                        room_id,
                        session_id,
                        ProviderTurnAuthority {
                            turn_id,
                            provider_turn_id: &result.provider_turn_id,
                            provider_session_id: result.provider_session_id.as_deref(),
                        },
                        &reason_code,
                    )
                    .await
            }
        },
        Err(error) => {
            let confirmed_stop = error.runtime_stopped.then_some((
                error.runtime_handle_id.as_str(),
                error.runtime_owner_id.as_str(),
            ));
            let commit = store
                .fail_agent_turn(
                    room_id,
                    session_id,
                    turn_id,
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
                    )
                    .await;
            }
            Ok(commit)
        }
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
            provider_adapter.clone(),
            assignment,
            room_tool_ingress.clone(),
        );
    }
}
