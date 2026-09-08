use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{PersistenceError, RoomSessionAuthorization};
use agentsassemble_protocol::{CommandAck, CommandResolution, RoomAction, ServerFrame};
use serde_json::{Value, json};

use crate::{
    AppState, room_command_result::CommandFailure, room_history_socket::read_history_frame,
    room_vote_socket::read_vote_summary_frame,
};

pub(crate) async fn command_frame(
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    session: Option<&RoomSessionAuthorization>,
    request_id: &str,
    action: RoomAction,
    payload: &Value,
) -> Option<Result<ServerFrame, CommandFailure>> {
    Some(match action {
        RoomAction::RoomHistory | RoomAction::ChannelHistory => {
            read_history_frame(
                &state.store,
                &state.socket_admission,
                principal,
                session,
                action,
                request_id,
                payload,
            )
            .await
        }
        RoomAction::RoomVoteSummary => {
            read_vote_summary_frame(&state.store, principal, session, request_id, payload).await
        }
        RoomAction::SideChatSend => {
            side_chat_frame(state, principal, session, request_id, payload).await
        }
        _ => return None,
    })
}

async fn side_chat_frame(
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    session: Option<&RoomSessionAuthorization>,
    request_id: &str,
    payload: &Value,
) -> Result<ServerFrame, CommandFailure> {
    let committed = state
        .rooms
        .execute_side_chat(principal, session, request_id, payload)
        .await?;
    let update = serde_json::to_value(committed.update)
        .map_err(PersistenceError::from)
        .map_err(CommandFailure::unresolved)?;
    Ok(ServerFrame::Ack(CommandAck {
        request_id: request_id.to_owned(),
        accepted: true,
        resolution: CommandResolution::Committed,
        action: RoomAction::SideChatSend.as_str().to_owned(),
        result: json!({"update":update}),
        deduplicated: committed.deduplicated,
    }))
}
