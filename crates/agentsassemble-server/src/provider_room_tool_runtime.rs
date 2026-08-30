use agentsassemble_persistence::{PersistenceError, ProviderMessageSearchAuthority, SqliteStore};
use agentsassemble_provider::{
    ProviderRoomToolCommand, ProviderRoomToolError, ProviderRoomToolRequest, ProviderRoomToolResult,
};
use tokio::sync::broadcast;

use crate::provider_write_budget::ProviderWriteBudget;

pub(crate) async fn handle_provider_room_tool(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    room_id: &str,
    mut command: ProviderRoomToolCommand,
    write_budget: &mut ProviderWriteBudget,
) -> Option<crate::event_publication::PublicationAttempt> {
    if let Err(error) = command.begin_execution() {
        command.complete(Err(error));
        return None;
    }
    match command.request().clone() {
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
        _ => ProviderRoomToolError {
            code: "persistence_error",
            message: "The room tool request could not be completed.".to_owned(),
        },
    }
}
