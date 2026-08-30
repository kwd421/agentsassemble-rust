use agentsassemble_domain::{AuthenticatedPrincipal, VoteReference};
use agentsassemble_persistence::{HumanSessionAuthorization, PersistenceError, SqliteStore};
use agentsassemble_protocol::{CommandAck, CommandResolution, RoomAction, ServerFrame};
use serde_json::Value;

use crate::{
    authenticated_channel::encode_server_frame,
    room_command_result::{CommandFailure, validate_command_envelope},
};

pub(crate) async fn read_vote_summary_frame(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    human_session: Option<&HumanSessionAuthorization>,
    request_id: &str,
    payload: &Value,
) -> Result<ServerFrame, CommandFailure> {
    validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
    let request =
        VoteReference::from_summary_payload(payload).map_err(CommandFailure::domain_rejected)?;
    let summary = match human_session {
        Some(authorization) => {
            store
                .human_session_room_vote_summary(authorization, &request.vote_id)
                .await
        }
        None => {
            store
                .local_room_vote_summary(
                    &principal.room_id,
                    &principal.principal_id,
                    &principal.participant_id,
                    &request.vote_id,
                )
                .await
        }
    }
    .map_err(CommandFailure::transactional)?;
    let result = serde_json::to_value(summary)
        .map_err(PersistenceError::from)
        .map_err(CommandFailure::unresolved)?;
    let frame = ServerFrame::Ack(CommandAck {
        request_id: request_id.to_owned(),
        accepted: true,
        resolution: CommandResolution::Committed,
        action: RoomAction::RoomVoteSummary.as_str().to_owned(),
        result,
        deduplicated: false,
    });
    if encode_server_frame(&frame).is_err() {
        return Err(CommandFailure::rejected(
            PersistenceError::CommandRejected {
                code: "response_too_large",
                message: "The canonical vote summary exceeds the WebSocket frame limit.".to_owned(),
            },
        ));
    }
    Ok(frame)
}
