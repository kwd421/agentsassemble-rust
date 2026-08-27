use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};

use super::ControlledServer;

pub(super) async fn assert_invite_tickets(server: &mut ControlledServer) {
    let create = server
        .send_control(&LocalControlRequest::IssueHumanInviteCreateTicket {
            request_id: "human-invite-create-ticket-1".to_owned(),
            meeting_id: "general".to_owned(),
        })
        .await;
    assert!(matches!(
        create,
        LocalControlResponse::HumanInviteCreateOk {
            request_id,
            ticket,
            ttl_seconds,
        } if request_id == "human-invite-create-ticket-1"
            && ticket.len() == 64
            && ttl_seconds > 0
    ));

    let revoke = server
        .send_control(&LocalControlRequest::IssueHumanInviteRevokeTicket {
            request_id: "human-invite-revoke-ticket-1".to_owned(),
            meeting_id: "general".to_owned(),
        })
        .await;
    assert!(matches!(
        revoke,
        LocalControlResponse::HumanInviteRevokeOk {
            request_id,
            ticket,
            ttl_seconds,
        } if request_id == "human-invite-revoke-ticket-1"
            && ticket.len() == 64
            && ttl_seconds > 0
    ));
}
