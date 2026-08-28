use std::path::Path;

use agentsassemble_persistence::SqliteStore;
use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::ManagerRoomAuthorityRequest;
use reqwest::StatusCode;
use serde_json::{Value, json};

use super::ControlledServer;

pub(super) async fn assert_invite_tickets(
    server: &mut ControlledServer,
    created: &Value,
) -> String {
    let authority = manager_authority(created);
    let create_ticket = issue_create(server, &authority, "human-invite-create-ticket-1").await;
    let invite_id = create_invite(server, &create_ticket).await;
    let revoke_ticket = issue_revoke(server, &authority, "human-invite-revoke-ticket-1").await;
    assert_mismatched_authorities(server, &authority).await;
    revoke_invite(server, &revoke_ticket, &invite_id).await;
    invite_id
}

pub(super) async fn assert_persisted_revocation(database: &Path, invite_id: &str) {
    let store = SqliteStore::open_path(database)
        .await
        .unwrap_or_else(|error| panic!("reopen controlled invite authority: {error}"));
    let invites = store
        .list_human_invites()
        .await
        .unwrap_or_else(|error| panic!("read controlled invites after restart: {error}"));
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].invite_id, invite_id);
    assert!(invites[0].revoked);
}

async fn issue_create(
    server: &mut ControlledServer,
    authority: &ManagerRoomAuthorityRequest,
    request_id: &str,
) -> String {
    let response = server
        .send_control(&LocalControlRequest::IssueHumanInviteCreateTicket {
            request_id: request_id.to_owned(),
            server_id: authority.server_id.clone(),
            authority_lineage_id: authority.authority_lineage_id.clone(),
            meeting_id: authority.room_id.clone(),
            room_uid: authority.room_uid.clone(),
        })
        .await;
    let LocalControlResponse::HumanInviteCreateOk {
        request_id: actual,
        ticket,
        ttl_seconds,
    } = response
    else {
        panic!("invite-create ticket request was rejected");
    };
    assert_eq!(actual, request_id);
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    ticket
}

async fn issue_revoke(
    server: &mut ControlledServer,
    authority: &ManagerRoomAuthorityRequest,
    request_id: &str,
) -> String {
    let response = server
        .send_control(&LocalControlRequest::IssueHumanInviteRevokeTicket {
            request_id: request_id.to_owned(),
            server_id: authority.server_id.clone(),
            authority_lineage_id: authority.authority_lineage_id.clone(),
            meeting_id: authority.room_id.clone(),
            room_uid: authority.room_uid.clone(),
        })
        .await;
    let LocalControlResponse::HumanInviteRevokeOk {
        request_id: actual,
        ticket,
        ttl_seconds,
    } = response
    else {
        panic!("invite-revoke ticket request was rejected");
    };
    assert_eq!(actual, request_id);
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    ticket
}

async fn assert_mismatched_authorities(
    server: &mut ControlledServer,
    authority: &ManagerRoomAuthorityRequest,
) {
    for (request_id, server_id, lineage_id, room_uid) in [
        (
            "human-invite-wrong-server",
            different_uuid(&authority.server_id),
            authority.authority_lineage_id.clone(),
            authority.room_uid.clone(),
        ),
        (
            "human-invite-wrong-lineage",
            authority.server_id.clone(),
            different_uuid(&authority.authority_lineage_id),
            authority.room_uid.clone(),
        ),
        (
            "human-invite-wrong-room-uid",
            authority.server_id.clone(),
            authority.authority_lineage_id.clone(),
            different_uuid(&authority.room_uid),
        ),
    ] {
        let rejected = server
            .send_control(&LocalControlRequest::IssueHumanInviteCreateTicket {
                request_id: request_id.to_owned(),
                server_id,
                authority_lineage_id: lineage_id,
                meeting_id: authority.room_id.clone(),
                room_uid,
            })
            .await;
        assert!(matches!(
            rejected,
            LocalControlResponse::Error { request_id: actual, code, .. }
                if actual == request_id && code == "room_authority_changed"
        ));
    }
}

async fn create_invite(server: &ControlledServer, ticket: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!("{}/api/room-invite/create", server.address))
        .bearer_auth(ticket)
        .json(&json!({
            "meeting_id": "general",
            "invite_scope": "room",
            "ttl_seconds": 60,
            "max_uses": 1
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("create invite through controlled HTTP server: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode controlled invite: {error}"));
    field(&body, "invite_id").to_owned()
}

async fn revoke_invite(server: &ControlledServer, ticket: &str, invite_id: &str) {
    let response = reqwest::Client::new()
        .post(format!("{}/api/room-invite/revoke", server.address))
        .bearer_auth(ticket)
        .json(&json!({"meeting_id": "general", "invite_id": invite_id}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("revoke invite through controlled HTTP server: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode controlled revoke: {error}"));
    assert_eq!(body["status"], "revoked");
    assert_eq!(body["invite_id"], invite_id);
}

fn manager_authority(created: &Value) -> ManagerRoomAuthorityRequest {
    ManagerRoomAuthorityRequest {
        server_id: field(created, "server_id").to_owned(),
        authority_lineage_id: field(created, "authority_lineage_id").to_owned(),
        room_id: field(&created["room"], "room_id").to_owned(),
        room_uid: field(&created["room"], "room_uid").to_owned(),
    }
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("value has no {name}"))
}

fn different_uuid(value: &str) -> String {
    let mut different = value.to_owned();
    let replacement = if different.ends_with('0') { '1' } else { '0' };
    different.pop();
    different.push(replacement);
    different
}
