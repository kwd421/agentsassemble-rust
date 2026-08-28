use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use serde_json::Value;

use super::ControlledServer;

pub(super) async fn assert_invite_tickets(server: &mut ControlledServer, created: &Value) {
    let server_id = field(created, "server_id");
    let authority_lineage_id = field(created, "authority_lineage_id");
    let canonical_room = field(&created["room"], "room_id");
    let stable_room_uid = field(&created["room"], "room_uid");
    let create = server
        .send_control(&LocalControlRequest::IssueHumanInviteCreateTicket {
            request_id: "human-invite-create-ticket-1".to_owned(),
            server_id: server_id.to_owned(),
            authority_lineage_id: authority_lineage_id.to_owned(),
            meeting_id: canonical_room.to_owned(),
            room_uid: stable_room_uid.to_owned(),
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
            server_id: server_id.to_owned(),
            authority_lineage_id: authority_lineage_id.to_owned(),
            meeting_id: canonical_room.to_owned(),
            room_uid: stable_room_uid.to_owned(),
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

    let different_server = different_uuid(server_id);
    let different_lineage = different_uuid(authority_lineage_id);
    let different_room_uid = different_uuid(stable_room_uid);
    for (request_id, wrong_server, wrong_lineage, wrong_room_uid) in [
        (
            "human-invite-wrong-server",
            different_server.as_str(),
            authority_lineage_id,
            stable_room_uid,
        ),
        (
            "human-invite-wrong-lineage",
            server_id,
            different_lineage.as_str(),
            stable_room_uid,
        ),
        (
            "human-invite-wrong-room-uid",
            server_id,
            authority_lineage_id,
            different_room_uid.as_str(),
        ),
    ] {
        let rejected = server
            .send_control(&LocalControlRequest::IssueHumanInviteCreateTicket {
                request_id: request_id.to_owned(),
                server_id: wrong_server.to_owned(),
                authority_lineage_id: wrong_lineage.to_owned(),
                meeting_id: canonical_room.to_owned(),
                room_uid: wrong_room_uid.to_owned(),
            })
            .await;
        assert!(matches!(
            rejected,
            LocalControlResponse::Error { request_id: actual, code, .. }
                if actual == request_id && code == "room_authority_changed"
        ));
    }
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("created room has no {name}"))
}

fn different_uuid(value: &str) -> String {
    let mut different = value.to_owned();
    let replacement = if different.ends_with('0') { '1' } else { '0' };
    different.pop();
    different.push(replacement);
    different
}
