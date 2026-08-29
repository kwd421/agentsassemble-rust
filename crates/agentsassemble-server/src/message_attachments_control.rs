use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::{
    issue_message_attachment_read_ticket, issue_message_attachment_upload_ticket,
};

use crate::{AppState, control_error};

pub(crate) async fn response(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    match request {
        LocalControlRequest::IssueMessageAttachmentUploadTicket { meeting_id, .. } => {
            match issue_message_attachment_upload_ticket(state, &meeting_id).await {
                Ok(ticket) => LocalControlResponse::MessageAttachmentUploadOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        LocalControlRequest::IssueMessageAttachmentReadTicket {
            meeting_id,
            attachment_id,
            ..
        } => match issue_message_attachment_read_ticket(state, &meeting_id, &attachment_id).await {
            Ok(ticket) => LocalControlResponse::MessageAttachmentReadOk {
                request_id,
                ticket: ticket.ticket,
                ttl_seconds: ticket.ttl_seconds,
            },
            Err(error) => control_error(request_id, error),
        },
        _ => unreachable!("message-attachment control owner received another request"),
    }
}
