use agentsassemble_persistence::{PersistenceError, ProviderMessageSearchAuthority};
use agentsassemble_provider::{
    ProviderRoomToolCommand, ProviderRoomToolError, ProviderRoomToolRequest, ProviderRoomToolResult,
};

use crate::provider_write_budget::ProviderWriteBudget;

pub(crate) async fn handle_provider_room_tool(
    owners: crate::room_runtime::RoomCommandOwners<'_>,
    room_id: &str,
    mut command: ProviderRoomToolCommand,
    write_budget: &mut ProviderWriteBudget,
    admission: &crate::principal_mutation_admission::PrincipalMutationAdmission,
) -> Option<crate::event_publication::PublicationAttempt> {
    let store = owners.store;
    let event_tx = owners.event_tx;
    if let Err(error) = command.begin_execution().await {
        command.complete(Err(error));
        return None;
    }
    let delegated = agentsassemble_persistence::DelegateRoomAuthority::Provider {
        room_id: room_id.into(),
        session_id: command.session_id().into(),
        turn_id: command.turn_id().into(),
        input_up_to_seq: command.input_up_to_seq(),
        turn_generation: command.turn_generation(),
        execution_id: command.execution_id().into(),
    };
    match command.request().clone() {
        ProviderRoomToolRequest::DirectAction(request) => {
            let execution = crate::room_runtime::delegated_direct::apply(
                store,
                owners.provider_adapter,
                admission,
                &delegated,
                &request,
            )
            .await;
            let publication =
                crate::room_runtime::delegated_direct::publish(owners, &execution).await;
            command.complete(
                execution
                    .reply
                    .map(|outcome| ProviderRoomToolResult::DirectAction(outcome.result))
                    .map_err(|failure| public_tool_error(failure.error)),
            );
            publication
        }
        ProviderRoomToolRequest::DelegatedActions => {
            command.complete(
                store
                    .delegated_direct_actions(&delegated)
                    .await
                    .map(ProviderRoomToolResult::DelegatedActions)
                    .map_err(public_tool_error),
            );
            None
        }
        ProviderRoomToolRequest::SetMessagePin(request) => {
            let result =
                crate::room_runtime::delegated_pin::apply(store, admission, &delegated, &request)
                    .await;
            let publication = if result.as_ref().is_ok_and(|outcome| !outcome.deduplicated) {
                Some(
                    crate::event_publication::publish_durable_room_events(store, event_tx, room_id)
                        .await,
                )
            } else {
                None
            };
            command.complete(
                result
                    .map(|outcome| ProviderRoomToolResult::MessagePin(outcome.result))
                    .map_err(public_tool_error),
            );
            publication
        }
        ProviderRoomToolRequest::ReadRoomStatus { before_seq } => {
            let result = store
                .provider_conversation_status(authority(room_id, &command), before_seq)
                .await
                .map(ProviderRoomToolResult::ConversationStatus)
                .map_err(public_tool_error);
            command.complete(result);
            None
        }
        ProviderRoomToolRequest::Random(request) => {
            crate::room_random_runtime::handle_provider_room_random(
                store,
                event_tx,
                room_id,
                command,
                request,
                write_budget,
            )
            .await
        }
        ProviderRoomToolRequest::SearchMessages { query, cursor } => {
            let result = store
                .search_provider_lobby_messages(authority(room_id, &command), &query, &cursor)
                .await
                .map(ProviderRoomToolResult::SearchMessages)
                .map_err(public_tool_error);
            command.complete(result);
            None
        }
        ProviderRoomToolRequest::ReadMessageContext { event_id } => {
            let result = store
                .provider_lobby_message_context(authority(room_id, &command), &event_id)
                .await
                .map(ProviderRoomToolResult::MessageContext)
                .map_err(public_tool_error);
            command.complete(result);
            None
        }
    }
}

fn authority<'a>(
    room_id: &'a str,
    command: &'a ProviderRoomToolCommand,
) -> ProviderMessageSearchAuthority<'a> {
    ProviderMessageSearchAuthority {
        room_id,
        session_id: command.session_id(),
        turn_id: command.turn_id(),
        input_up_to_seq: command.input_up_to_seq(),
        turn_generation: command.turn_generation(),
        execution_id: command.execution_id(),
    }
}

pub(crate) fn public_tool_error(error: PersistenceError) -> ProviderRoomToolError {
    match error {
        PersistenceError::CommandRejected { code, message } => {
            ProviderRoomToolError { code, message }
        }
        PersistenceError::CommandConflict => ProviderRoomToolError {
            code: "command_conflict".into(),
            message: "The request ID was used with different parameters.".into(),
        },
        _ => ProviderRoomToolError {
            code: "persistence_error".into(),
            message: "The room tool request could not be completed.".to_owned(),
        },
    }
}
