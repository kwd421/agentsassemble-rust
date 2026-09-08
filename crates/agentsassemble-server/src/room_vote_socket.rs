use agentsassemble_domain::{AuthenticatedPrincipal, VoteReference};
use agentsassemble_persistence::{PersistenceError, RoomSessionAuthorization, SqliteStore};
use agentsassemble_protocol::{CommandAck, CommandResolution, RoomAction, ServerFrame};
use serde_json::Value;

use crate::{
    room_channel::encode_server_frame,
    room_command_result::{CommandFailure, validate_command_envelope},
};

pub(crate) async fn read_vote_summary_frame(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    room_session: Option<&RoomSessionAuthorization>,
    request_id: &str,
    payload: &Value,
) -> Result<ServerFrame, CommandFailure> {
    validate_command_envelope(request_id).map_err(CommandFailure::rejected)?;
    let request =
        VoteReference::from_summary_payload(payload).map_err(CommandFailure::domain_rejected)?;
    let summary = match room_session {
        Some(authorization) => {
            store
                .authorized_room_vote_summary(authorization.mutation_authority(), &request.vote_id)
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
                code: "response_too_large".into(),
                message: "The canonical vote summary exceeds the WebSocket frame limit.".to_owned(),
            },
        ));
    }
    Ok(frame)
}
