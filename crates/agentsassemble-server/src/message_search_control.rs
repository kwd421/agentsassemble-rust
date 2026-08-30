use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::issue_message_search_read_ticket;

use crate::{AppState, control_error};

pub(crate) async fn response(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let LocalControlRequest::IssueMessageSearchReadTicket { meeting_id, .. } = request else {
        unreachable!("message-search control owner received another request");
    };
    match issue_message_search_read_ticket(state, &meeting_id).await {
        Ok(ticket) => LocalControlResponse::MessageSearchReadOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        Err(error) => control_error(request_id, error),
    }
}
