use agentsassemble_persistence::{
    AgentTurnAssignment, AgentTurnCommit, PersistenceError, ProviderTurnAuthority, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAdapterError, ProviderRoomObservation, ProviderTurnCompleted,
    ProviderTurnOutcome, ProviderTurnRequest,
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
) {
    tasks.spawn(async move {
        let request = ProviderTurnRequest {
            turn_id: assignment.turn_id.clone(),
            input: assignment.provider_input.clone(),
            room_observation: Some(ProviderRoomObservation {
                input_up_to_seq: assignment.session.input_up_to_seq,
                view: assignment.room_view.clone(),
                allowed_agent_ids: assignment.room_agent_ids.clone(),
            }),
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

pub(crate) fn publish_turn_commit(
    event_tx: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    tasks: &mut JoinSet<ProviderTurnTaskResult>,
    provider_adapter: ProviderAdapter,
    commit: AgentTurnCommit,
) {
    for event in commit.events {
        let _ = event_tx.send(event);
    }
    if let Some(assignment) = commit.next_assignment {
        spawn_provider_turn(tasks, provider_adapter, assignment);
    }
}
