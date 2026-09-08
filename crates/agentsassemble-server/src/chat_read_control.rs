use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::{issue_message_search_read_ticket, issue_side_chat_read_ticket};

use crate::{AppState, control_error};

pub(crate) async fn response(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let (issued, side_chat) = match request {
        LocalControlRequest::IssueMessageSearchReadTicket { meeting_id, .. } => (
            issue_message_search_read_ticket(state, &meeting_id).await,
            false,
        ),
        LocalControlRequest::IssueSideChatReadTicket { meeting_id, .. } => {
            (issue_side_chat_read_ticket(state, &meeting_id).await, true)
        }
        _ => unreachable!("chat-read control owner received another request"),
    };
    match issued {
        Ok(ticket) if side_chat => LocalControlResponse::SideChatReadOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        Ok(ticket) => LocalControlResponse::MessageSearchReadOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        Err(error) => control_error(request_id, error),
    }
}
