use agentsassemble_domain::{RoomRandomRequest, RoomRandomResult};
use agentsassemble_persistence::{PersistenceError, ProviderRoomRandomCommit, SqliteStore};
use agentsassemble_provider::{ProviderRoomToolCommand, ProviderRoomToolError};
use tokio::sync::broadcast;
use uuid::Uuid;

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

pub(crate) async fn handle_provider_room_tool(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<agentsassemble_domain::RoomEvent>,
    room_id: &str,
    mut command: ProviderRoomToolCommand,
) {
    if let Err(error) = command.begin_commit() {
        command.complete(Err(error));
        return;
    }
    let result = generate_room_random(command.request());
    let result_id = format!("result-{}", Uuid::new_v4().simple());
    let committed = store
        .commit_provider_room_random(ProviderRoomRandomCommit {
            room_id,
            session_id: command.session_id(),
            turn_id: command.turn_id(),
            input_up_to_seq: command.input_up_to_seq(),
            result_id: &result_id,
            request: command.request(),
            result: &result,
        })
        .await;
    match committed {
        Ok(_) => {
            if let Err(error) =
                crate::event_publication::drain_room_publications(store, event_tx, room_id).await
            {
                tracing::error!(
                    error = ?error,
                    room_id,
                    "committed room-tool event remains pending for publication retry"
                );
            }
            command.complete(Ok(result));
        }
        Err(error) => command.complete(Err(public_tool_error(error))),
    }
}

fn public_tool_error(error: PersistenceError) -> ProviderRoomToolError {
    match error {
        PersistenceError::CommandRejected { code, message } => {
            ProviderRoomToolError { code, message }
        }
        _ => ProviderRoomToolError {
            code: "persistence_error",
            message: "The room tool result could not be committed.".to_owned(),
        },
    }
}
