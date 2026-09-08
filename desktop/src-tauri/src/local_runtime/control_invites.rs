use super::{
    HttpTicketKind, LocalControlRequest, LocalControlResponse, TicketFailure,
    control_ticket_failure,
};

pub(super) fn request(kind: HttpTicketKind<'_>, request_id: &str) -> LocalControlRequest {
    match kind {
        HttpTicketKind::ConnectorInviteCreate(authority) => {
            LocalControlRequest::IssueConnectorInviteCreateTicket {
                request_id: request_id.to_owned(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
            }
        }
        HttpTicketKind::HumanInviteCreate(authority) => {
            LocalControlRequest::IssueHumanInviteCreateTicket {
                request_id: request_id.to_owned(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
            }
        }
        HttpTicketKind::HumanInviteRevoke(authority) => {
            LocalControlRequest::IssueHumanInviteRevokeTicket {
                request_id: request_id.to_owned(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
            }
        }
        _ => unreachable!("invite request dispatch requires an invite kind"),
    }
}

pub(super) fn response(
    kind: HttpTicketKind<'_>,
    request_id: &str,
    response: LocalControlResponse,
) -> Result<(String, u64), TicketFailure> {
    match (kind, response) {
        (
            HttpTicketKind::ConnectorInviteCreate(_),
            LocalControlResponse::ConnectorInviteCreateOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        )
        | (
            HttpTicketKind::HumanInviteCreate(_),
            LocalControlResponse::HumanInviteCreateOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        )
        | (
            HttpTicketKind::HumanInviteRevoke(_),
            LocalControlResponse::HumanInviteRevokeOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            _,
            LocalControlResponse::Error {
                request_id: response_id,
                code,
                message,
            },
        ) if response_id == request_id => Err(control_ticket_failure(&code, message)),
        _ => Err(TicketFailure::Broken(
            "local runtime invite ticket response did not match the request".to_owned(),
        )),
    }
}
