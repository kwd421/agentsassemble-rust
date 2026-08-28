use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::{
    AppState, ManagerRoomAuthorityRequest, issue_appearance_bound_read_ticket,
    issue_appearance_pending_read_ticket, issue_appearance_upload_ticket,
};

use crate::{control_error, manager_request};

enum AppearanceTicketRequest {
    Upload(ManagerRoomAuthorityRequest),
    PendingRead(ManagerRoomAuthorityRequest, String),
    BoundRead(ManagerRoomAuthorityRequest, String),
}

pub(crate) async fn response(
    state: &AppState,
    request_id: String,
    request: LocalControlRequest,
) -> LocalControlResponse {
    let request = match request {
        LocalControlRequest::IssueAppearanceUploadTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            ..
        } => AppearanceTicketRequest::Upload(manager_request(
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
        )),
        LocalControlRequest::IssueAppearancePendingReadTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            asset_id,
            ..
        } => AppearanceTicketRequest::PendingRead(
            manager_request(server_id, authority_lineage_id, meeting_id, room_uid),
            asset_id,
        ),
        LocalControlRequest::IssueAppearanceBoundReadTicket {
            server_id,
            authority_lineage_id,
            meeting_id,
            room_uid,
            asset_id,
            ..
        } => AppearanceTicketRequest::BoundRead(
            manager_request(server_id, authority_lineage_id, meeting_id, room_uid),
            asset_id,
        ),
        _ => unreachable!("appearance control accepts only appearance ticket requests"),
    };
    match request {
        AppearanceTicketRequest::Upload(authority) => {
            match issue_appearance_upload_ticket(state, &authority).await {
                Ok(ticket) => LocalControlResponse::AppearanceUploadOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        AppearanceTicketRequest::PendingRead(authority, asset_id) => {
            match issue_appearance_pending_read_ticket(state, &authority, &asset_id).await {
                Ok(ticket) => LocalControlResponse::AppearancePendingReadOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
        AppearanceTicketRequest::BoundRead(authority, asset_id) => {
            match issue_appearance_bound_read_ticket(state, &authority, &asset_id).await {
                Ok(ticket) => LocalControlResponse::AppearanceBoundReadOk {
                    request_id,
                    ticket: ticket.ticket,
                    ttl_seconds: ticket.ttl_seconds,
                },
                Err(error) => control_error(request_id, error),
            }
        }
    }
}
