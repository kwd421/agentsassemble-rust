use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use agentsassemble_server::ManagerRoomAuthorityRequest;
use reqwest::StatusCode;
use serde_json::{Value, json};

use super::ControlledServer;

const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==";

pub(super) async fn assert_appearance_tickets(server: &mut ControlledServer, created: &Value) {
    let authority = manager_authority(created);
    let upload_ticket = issue_upload(server, &authority).await;
    assert_mismatched_authority_rejected(server, &authority).await;

    let upload = reqwest::Client::new()
        .post(format!("{}/api/attachments", server.address))
        .bearer_auth(upload_ticket)
        .json(&json!({
            "purpose": "room_appearance",
            "filename": "control-banner.png",
            "content_type": "image/png",
            "data_base64": PNG_BASE64
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload controlled appearance: {error}"));
    assert_eq!(upload.status(), StatusCode::OK);
    let upload: Value = upload
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode controlled appearance: {error}"));
    let asset_id = field(&upload["attachment"], "id");
    let asset_url = field(&upload["attachment"], "url");

    let malformed = issue_pending(server, &authority, "ra_invalid", "appearance-invalid").await;
    assert!(matches!(
        malformed,
        LocalControlResponse::Error { request_id, code, .. }
            if request_id == "appearance-invalid" && code == "bad_request"
    ));

    let pending = issue_pending(server, &authority, asset_id, "appearance-pending").await;
    let LocalControlResponse::AppearancePendingReadOk { ticket, .. } = pending else {
        panic!("pending appearance read ticket was rejected");
    };
    let preview = reqwest::Client::new()
        .get(format!("{}{}", server.address, asset_url))
        .bearer_auth(ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read controlled pending appearance: {error}"));
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()["content-type"], "image/png");
    assert_eq!(preview.headers()["cache-control"], "private, no-store");

    let bound = server
        .send_control(&LocalControlRequest::IssueAppearanceBoundReadTicket {
            request_id: "appearance-bound".to_owned(),
            server_id: authority.server_id.clone(),
            authority_lineage_id: authority.authority_lineage_id.clone(),
            meeting_id: authority.room_id.clone(),
            room_uid: authority.room_uid.clone(),
            asset_id: asset_id.to_owned(),
        })
        .await;
    assert!(matches!(
        bound,
        LocalControlResponse::AppearanceBoundReadOk { request_id, ticket, ttl_seconds }
            if request_id == "appearance-bound" && ticket.len() == 64 && ttl_seconds > 0
    ));
}

async fn issue_upload(
    server: &mut ControlledServer,
    authority: &ManagerRoomAuthorityRequest,
) -> String {
    let response = server
        .send_control(&LocalControlRequest::IssueAppearanceUploadTicket {
            request_id: "appearance-upload".to_owned(),
            server_id: authority.server_id.clone(),
            authority_lineage_id: authority.authority_lineage_id.clone(),
            meeting_id: authority.room_id.clone(),
            room_uid: authority.room_uid.clone(),
        })
        .await;
    let LocalControlResponse::AppearanceUploadOk {
        request_id,
        ticket,
        ttl_seconds,
    } = response
    else {
        panic!("appearance upload ticket was rejected");
    };
    assert_eq!(request_id, "appearance-upload");
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    ticket
}

async fn issue_pending(
    server: &mut ControlledServer,
    authority: &ManagerRoomAuthorityRequest,
    asset_id: &str,
    request_id: &str,
) -> LocalControlResponse {
    server
        .send_control(&LocalControlRequest::IssueAppearancePendingReadTicket {
            request_id: request_id.to_owned(),
            server_id: authority.server_id.clone(),
            authority_lineage_id: authority.authority_lineage_id.clone(),
            meeting_id: authority.room_id.clone(),
            room_uid: authority.room_uid.clone(),
            asset_id: asset_id.to_owned(),
        })
        .await
}

async fn assert_mismatched_authority_rejected(
    server: &mut ControlledServer,
    authority: &ManagerRoomAuthorityRequest,
) {
    for (request_id, server_id, lineage_id, room_uid) in [
        (
            "appearance-wrong-server",
            different_uuid(&authority.server_id),
            authority.authority_lineage_id.clone(),
            authority.room_uid.clone(),
        ),
        (
            "appearance-wrong-lineage",
            authority.server_id.clone(),
            different_uuid(&authority.authority_lineage_id),
            authority.room_uid.clone(),
        ),
        (
            "appearance-wrong-room-uid",
            authority.server_id.clone(),
            authority.authority_lineage_id.clone(),
            different_uuid(&authority.room_uid),
        ),
    ] {
        let rejected = server
            .send_control(&LocalControlRequest::IssueAppearanceUploadTicket {
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
