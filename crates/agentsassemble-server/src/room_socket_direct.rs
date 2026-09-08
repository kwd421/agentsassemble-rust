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
    room_uid: uuid::Uuid,
    session: Option<&RoomSessionAuthorization>,
    request_id: &str,
    action: RoomAction,
    payload: &Value,
) -> Option<Result<ServerFrame, CommandFailure>> {
    Some(match action {
        RoomAction::ProviderRequestResolve => {
            provider_response_frame(state, principal, room_uid, session, request_id, payload).await
        }
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

async fn provider_response_frame(
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    room_uid: uuid::Uuid,
    session: Option<&RoomSessionAuthorization>,
    request_id: &str,
    payload: &Value,
) -> Result<ServerFrame, CommandFailure> {
    let id = uuid::Uuid::parse_str(request_id)
        .ok()
        .filter(|id| !id.is_nil())
        .ok_or_else(|| {
            CommandFailure::rejected(PersistenceError::CommandRejected {
                code: "invalid_provider_response",
                message: "Use the provider request UUID as the response request identity."
                    .to_owned(),
            })
        })?;
    let resolution: agentsassemble_domain::ProviderRequestResolution =
        serde_json::from_value(payload.clone()).map_err(|_| {
            CommandFailure::rejected(PersistenceError::CommandRejected {
                code: "invalid_provider_response",
                message: "The provider response is invalid.".to_owned(),
            })
        })?;
    let committed = match session {
        Some(session) => {
            state
                .rooms
                .resolve_live_provider_request(session.clone(), id, resolution)
                .await
        }
        None => {
            state
                .rooms
                .resolve_local_provider_request(principal.clone(), room_uid, id, resolution)
                .await
        }
    }
    .map_err(CommandFailure::transactional)?;
    Ok(ServerFrame::Ack(CommandAck {
        request_id: request_id.to_owned(),
        accepted: true,
        resolution: CommandResolution::Committed,
        action: RoomAction::ProviderRequestResolve.as_str().to_owned(),
        result: json!({"provider_request_id": id, "event_id": committed.event.id}),
        deduplicated: committed.deduplicated,
    }))
}
