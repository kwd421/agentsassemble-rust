use super::{issue_operator_ticket, issue_room_ticket, start, zero_room_fixture};
use agentsassemble_server::TicketStore;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

async fn response(request: RequestBuilder, status: StatusCode) -> Value {
    let result = request
        .send()
        .await
        .unwrap_or_else(|error| panic!("friend request: {error}"));
    assert_eq!(result.status(), status);
    assert_eq!(
        result
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    result
        .json()
        .await
        .unwrap_or_else(|error| panic!("friend response: {error}"))
}

#[tokio::test]
async fn friends_http_requires_one_use_server_authority_and_returns_committed_changes() {
    let store = zero_room_fixture().await;
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let server = start(store, tickets.clone()).await;
    let client = Client::new();
    let url = format!("{}/api/room-friends", server.base_url);
    response(client.get(&url), StatusCode::UNAUTHORIZED).await;
    response(
        client
            .get(&url)
            .bearer_auth(issue_room_ticket(&tickets, "room-main").await),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let ticket = issue_operator_ticket(&tickets).await;
    assert_eq!(
        response(client.get(&url).bearer_auth(&ticket), StatusCode::OK).await,
        json!({"friends": []})
    );
    response(
        client.get(&url).bearer_auth(&ticket),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let mut draft = json!({
        "friend_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "expected_revision": 0,
        "details": {"display_name": "친구", "handle": "", "participant_type": "human",
            "provider_kind": "codex", "connection_kind": "", "agent_id": "",
            "source_agent_id": "", "last_meeting_id": "", "status": "offline",
            "source": "manual", "last_seen_at": null}
    });
    let saved = response(
        client
            .post(&url)
            .bearer_auth(issue_operator_ticket(&tickets).await)
            .json(&draft),
        StatusCode::OK,
    )
    .await;
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["details"], draft["details"]);
    draft["details"]["display_name"] = json!("changed without revision");
    response(
        client
            .post(&url)
            .bearer_auth(issue_operator_ticket(&tickets).await)
            .json(&draft),
        StatusCode::CONFLICT,
    )
    .await;
    let deletion = response(
        client
            .delete(format!(
                "{url}?friend_id=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
            ))
            .bearer_auth(issue_operator_ticket(&tickets).await),
        StatusCode::OK,
    )
    .await;
    assert_eq!(deletion, json!({"deleted": true}));
    assert_eq!(
        response(
            client
                .get(&url)
                .bearer_auth(issue_operator_ticket(&tickets).await),
            StatusCode::OK
        )
        .await,
        json!({"friends": []})
    );
    server.stop().await;
}
