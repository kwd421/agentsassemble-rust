use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::{issue_message_pins_read_ticket, issue_message_pins_write_ticket};

use crate::{AppState, control_error};

pub(crate) async fn response(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let (write, ticket) = match request {
        LocalControlRequest::IssueMessagePinsReadTicket { meeting_id, .. } => (
            false,
            issue_message_pins_read_ticket(state, &meeting_id).await,
        ),
        LocalControlRequest::IssueMessagePinsWriteTicket { meeting_id, .. } => (
            true,
            issue_message_pins_write_ticket(state, &meeting_id).await,
        ),
        _ => unreachable!("message-pin control owner received another request"),
    };
    match (write, ticket) {
        (false, Ok(ticket)) => LocalControlResponse::MessagePinsReadOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        (true, Ok(ticket)) => LocalControlResponse::MessagePinsWriteOk {
            request_id,
            ticket: ticket.ticket,
            ttl_seconds: ticket.ttl_seconds,
        },
        (_, Err(error)) => control_error(request_id, error),
    }
}
