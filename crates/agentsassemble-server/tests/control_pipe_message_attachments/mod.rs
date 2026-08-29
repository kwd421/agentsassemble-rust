use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::connect_async;

use super::{ControlledServer, support::subscription_proof::AuthenticatedTestSocket};

pub(super) async fn assert_message_attachment_tickets(server: &mut ControlledServer) {
    let client = Client::new();
    let upload_ticket = issue_upload(server).await;
    let uploaded = upload(&client, server, &upload_ticket).await;
    let attachment_id = field(&uploaded, "id");
    let download_url = field(&uploaded, "download_url");

    let replay = upload(&client, server, &upload_ticket).await;
    assert_eq!(replay["code"], "unauthorized");

    let unbound_ticket = issue_read(server, attachment_id, "attachment-unbound-read").await;
    let unbound = read(&client, server, download_url, &unbound_ticket).await;
    assert_eq!(unbound.status(), StatusCode::NOT_FOUND);

    let mut socket = connect_room(server).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "control-message-attachment-bind",
            "action": "message.send",
            "payload": {"content": "", "attachment_ids": [attachment_id]}
        }))
        .await;
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    assert!([&first, &second].iter().any(|frame| {
        frame["op"] == "ack" && frame["request_id"] == "control-message-attachment-bind"
    }));
    assert!([&first, &second].iter().any(|frame| {
        frame["op"] == "event"
            && frame["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["attachments"][0]["id"] == attachment_id)
            })
    }));

    let consumed_unbound = read(&client, server, download_url, &unbound_ticket).await;
    assert_eq!(consumed_unbound.status(), StatusCode::UNAUTHORIZED);
    let read_ticket = issue_read(server, attachment_id, "attachment-bound-read").await;
    let readable = read(&client, server, download_url, &read_ticket).await;
    assert_eq!(readable.status(), StatusCode::OK);
    assert_eq!(readable.headers()["cache-control"], "private, no-store");
    assert_eq!(readable.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        readable.bytes().await.unwrap_or_default(),
        b"control bytes"[..]
    );
    socket.close().await;

    let malformed = server
        .send_control(&LocalControlRequest::IssueMessageAttachmentReadTicket {
            request_id: "attachment-invalid-read".to_owned(),
            meeting_id: "general".to_owned(),
            attachment_id: "ma_invalid".to_owned(),
        })
        .await;
    assert!(matches!(
        malformed,
        LocalControlResponse::Error { request_id, code, .. }
            if request_id == "attachment-invalid-read" && code == "bad_request"
    ));
}

async fn issue_upload(server: &mut ControlledServer) -> String {
    let response = server
        .send_control(&LocalControlRequest::IssueMessageAttachmentUploadTicket {
            request_id: "attachment-upload".to_owned(),
            meeting_id: "general".to_owned(),
        })
        .await;
    let LocalControlResponse::MessageAttachmentUploadOk {
        request_id,
        ticket,
        ttl_seconds,
    } = response
    else {
        panic!("message-attachment upload ticket was rejected");
    };
    assert_eq!(request_id, "attachment-upload");
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    ticket
}

async fn issue_read(
    server: &mut ControlledServer,
    attachment_id: &str,
    request_id: &str,
) -> String {
    let response = server
        .send_control(&LocalControlRequest::IssueMessageAttachmentReadTicket {
            request_id: request_id.to_owned(),
            meeting_id: "general".to_owned(),
            attachment_id: attachment_id.to_owned(),
        })
        .await;
    let LocalControlResponse::MessageAttachmentReadOk {
        request_id: actual,
        ticket,
        ttl_seconds,
    } = response
    else {
        panic!("message-attachment read ticket was rejected");
    };
    assert_eq!(actual, request_id);
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    ticket
}

async fn upload(client: &Client, server: &ControlledServer, ticket: &str) -> Value {
    let response = client
        .post(format!("{}/api/attachments", server.address))
        .bearer_auth(ticket)
        .json(&json!({
            "purpose": "room_attachment",
            "filename": "control.txt",
            "content_type": "text/plain",
            "data_base64": "Y29udHJvbCBieXRlcw=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload through controlled runtime: {error}"));
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode controlled upload: {error}"));
    if status == StatusCode::OK {
        body["attachment"].clone()
    } else {
        body
    }
}

async fn read(
    client: &Client,
    server: &ControlledServer,
    path: &str,
    ticket: &str,
) -> reqwest::Response {
    client
        .get(format!("{}{}", server.address, path))
        .bearer_auth(ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read through controlled runtime: {error}"))
}

async fn connect_room(
    server: &mut ControlledServer,
) -> AuthenticatedTestSocket<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let response = server.issue_ticket().await;
    let LocalControlResponse::Ok {
        ticket,
        server_proof_key,
        ..
    } = response
    else {
        panic!("room socket ticket was rejected");
    };
    let socket = connect_async(format!(
        "{}/ws?ticket={ticket}",
        server.address.replacen("http://", "ws://", 1)
    ))
    .await
    .unwrap_or_else(|error| panic!("connect controlled room socket: {error}"))
    .0;
    let mut socket = AuthenticatedTestSocket::new(socket, ticket, server_proof_key);
    let receipt = socket.subscribe(0).await;
    assert_eq!(receipt["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    socket
}

fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("value has no {name}"))
}
