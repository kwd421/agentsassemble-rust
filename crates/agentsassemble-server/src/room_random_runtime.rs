use agentsassemble_domain::{RoomRandomRequest, RoomRandomResult};
use agentsassemble_persistence::{
    CommandOutcome, HumanSessionAuthorization, PersistenceError, ProviderRoomRandomCommit,
    SqliteStore, room_write_command_size,
};
use agentsassemble_provider::{ProviderRoomToolCommand, ProviderRoomToolResult};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    provider_room_tool_runtime::public_tool_error, provider_write_budget::ProviderWriteBudget,
    room_runtime::RoomCommand,
};

pub(crate) async fn execute_room_random(
    store: &SqliteStore,
    command: &RoomCommand,
) -> Result<CommandOutcome, PersistenceError> {
    if let Some(outcome) = store
        .replay_command(
            &command.principal,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await?
    {
        return Ok(outcome);
    }
    let request =
        RoomRandomRequest::parse(command.action.as_str(), &command.payload).map_err(|error| {
            PersistenceError::CommandRejected {
                code: "invalid_room_random_request",
                message: error.message,
            }
        })?;
    let result = generate_room_random(&request);
    store
        .execute_room_random_command(
            &command.principal,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
            &result,
        )
        .await
}

pub(crate) async fn execute_human_session_room_random(
    store: &SqliteStore,
    command: &RoomCommand,
    authorization: &HumanSessionAuthorization,
) -> Result<CommandOutcome, PersistenceError> {
    let request =
        RoomRandomRequest::parse(command.action.as_str(), &command.payload).map_err(|error| {
            PersistenceError::CommandRejected {
                code: "invalid_room_random_request",
                message: error.message,
            }
        })?;
    let result = generate_room_random(&request);
    store
        .execute_human_session_room_random_command(
            authorization,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
            &result,
        )
        .await
}

#[must_use]
pub(crate) fn generate_room_random(request: &RoomRandomRequest) -> RoomRandomResult {
    match request {
        RoomRandomRequest::Roll {
            notation,
            count,
            sides,
            modifier,
            ..
        } => {
            let rolls = (0..*count)
                .map(|_| rand::random_range(1..=*sides))
                .collect::<Vec<_>>();
            let total =
                rolls.iter().map(|roll| i64::from(*roll)).sum::<i64>() + i64::from(*modifier);
            RoomRandomResult::RollDice {
                notation: notation.clone(),
                rolls,
                modifier: *modifier,
                total,
            }
        }
        RoomRandomRequest::Choose { options, .. } => {
            let index = rand::random_range(0..options.len());
            RoomRandomResult::ChooseRandom {
                choice: options[index].clone(),
                index,
                option_count: options.len(),
                options: options.clone(),
            }
        }
    }
}

pub(crate) async fn handle_provider_room_random(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    room_id: &str,
    command: ProviderRoomToolCommand,
    request: RoomRandomRequest,
    write_budget: &mut ProviderWriteBudget,
) -> Option<crate::event_publication::PublicationAttempt> {
    let result_id = format!("result-{}", Uuid::new_v4().simple());
    let payload = request.canonical_payload();
    let payload_bytes = match room_write_command_size(&result_id, request.room_action(), &payload) {
        Ok(payload_bytes) => payload_bytes,
        Err(error) => {
            command.complete(Err(public_tool_error(error)));
            return None;
        }
    };
    if let Err(error) = write_budget.admit(command.session_id(), payload_bytes) {
        command.complete(Err(public_tool_error(error)));
        return None;
    }
    let result = generate_room_random(&request);
    let committed = store
        .commit_provider_room_random(ProviderRoomRandomCommit {
            room_id,
            session_id: command.session_id(),
            turn_id: command.turn_id(),
            input_up_to_seq: command.input_up_to_seq(),
            turn_generation: command.turn_generation(),
            execution_id: command.execution_id(),
            result_id: &result_id,
            request: &request,
            result: &result,
        })
        .await;
    match committed {
        Ok(_) => {
            let publication =
                crate::event_publication::publish_durable_room_events(store, event_tx, room_id)
                    .await;
            command.complete(Ok(ProviderRoomToolResult::Random(result)));
            Some(publication)
        }
        Err(error) => {
            command.complete(Err(public_tool_error(error)));
            None
        }
    }
}
