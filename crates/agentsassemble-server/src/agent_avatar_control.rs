use crate::{control_error, manager_request};
use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::{AppState, issue_agent_avatar_upload_ticket};

pub(crate) async fn response(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let LocalControlRequest::IssueAgentAvatarUploadTicket {
        server_id,
        authority_lineage_id,
        meeting_id,
        room_uid,
        session_id,
        ..
    } = request
    else {
        unreachable!("Agent avatar control accepts only its upload ticket request");
    };
    let authority = manager_request(server_id, authority_lineage_id, meeting_id, room_uid);
    match issue_agent_avatar_upload_ticket(state, &authority, &session_id).await {
        Ok(ticket) => LocalControlResponse::AgentAvatarUploadOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        Err(error) => control_error(request_id, error),
    }
}
