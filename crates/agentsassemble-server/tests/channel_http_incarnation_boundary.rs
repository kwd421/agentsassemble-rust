use agentsassemble_domain::{InviteScope, SideChatSnapshot};
use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_server::{
    AppState, issue_local_ticket, issue_message_pins_read_ticket, issue_message_pins_write_ticket,
    issue_message_search_read_ticket, issue_side_chat_read_ticket,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}
use support::human_invite::{fixture, start};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn native_channel_http_tickets_cannot_cross_room_recreation() -> TestResult {
    let (store, _) = fixture(InviteScope::ReadOnly).await;
    let server = start(store.clone()).await;
    let app = server.state();
    let socket_ticket = issue_local_ticket(app, "general").await?;
    let principal = app.tickets.consume(&socket_ticket.ticket).await?.principal;
    let stale = tickets(app).await?;
    let (room_uid, event_id) = replace_room(&store, &principal).await?;

    let client = Client::new();
    let paths = [
        "/api/side-chat?room_id=general".to_owned(),
        "/api/room-search?room_id=general&channel_id=all&q=needle".to_owned(),
        format!("/api/room-search/context?room_id=general&channel_id=lobby&event_id={event_id}"),
        "/api/room-pins?room_id=general&channel_id=lobby".to_owned(),
    ];
    for (path, ticket) in paths.iter().zip(&stale[..4]) {
        let response = client
            .get(format!("{}{path}", server.base_url))
            .bearer_auth(ticket)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        let body = response.text().await?;
        assert!(
            !body.contains("replacement"),
            "stale credential exposed replacement data"
        );
    }
    let pin_body =
        json!({"room_id":"general","channel_id":"lobby","event_id":event_id,"pinned":true});
    let stale_write = client
        .post(format!("{}/api/room-pins", server.base_url))
        .bearer_auth(&stale[4])
        .json(&pin_body)
        .send()
        .await?;
    assert_eq!(stale_write.status(), StatusCode::UNAUTHORIZED);

    let fresh = tickets(app).await?;
    let side = client
        .get(format!("{}{}", server.base_url, paths[0]))
        .bearer_auth(&fresh[0])
        .send()
        .await?;
    assert_eq!(side.status(), StatusCode::OK);
    let side: SideChatSnapshot = side.json().await?;
    assert_eq!(side.room_uid, room_uid);
    assert_eq!(side.messages.len(), 1);
    for (path, ticket) in paths[1..3].iter().zip(&fresh[1..3]) {
        let response = client
            .get(format!("{}{path}", server.base_url))
            .bearer_auth(ticket)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.text().await?.contains("replacement needle"));
    }
    let pins = client
        .get(format!("{}{}", server.base_url, paths[3]))
        .bearer_auth(&fresh[3])
        .send()
        .await?;
    assert_eq!(pins.status(), StatusCode::OK);
    assert_eq!(pins.json::<Value>().await?["pins"], json!([]));
    let written = client
        .post(format!("{}/api/room-pins", server.base_url))
        .bearer_auth(&fresh[4])
        .json(&pin_body)
        .send()
        .await?;
    assert_eq!(written.status(), StatusCode::OK);
    assert_eq!(
        written.json::<Value>().await?["pins"][0]["event_id"],
        event_id
    );
    server.stop().await;
    Ok(())
}

async fn replace_room(
    store: &agentsassemble_persistence::SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
) -> TestResult<(uuid::Uuid, String)> {
    let old_room = store.snapshot("general", 0, 20).await?.room;

    store
        .execute_room_delete(
            TrustedPrincipal(principal),
            "delete-ticket-room",
            &json!({"room_uid":old_room.room_uid,"confirmation_name":old_room.label}),
        )
        .await?;
    for event in store.pending_room_publications("general").await? {
        store
            .acknowledge_room_publication("general", event.seq)
            .await?;
    }
    assert!(store.finish_room_deletion("general").await?);
    let replacement = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", "General")
        .await?;
    assert_ne!(old_room.room_uid, replacement.room.room_uid);
    let event = store
        .execute_message(
            principal,
            "new-room-message",
            "message.send",
            &json!({"content":"replacement needle"}),
        )
        .await?
        .event;
    let now = chrono::Utc::now();
    let initial = store
        .side_chat_snapshot(TrustedPrincipal(principal), now)
        .await?;
    store
        .execute_side_chat(
            TrustedPrincipal(principal),
            "new-side-chat",
            &json!({"generation":initial.generation,"after_seq":0,"content":"replacement private"}),
            now,
        )
        .await?;

    Ok((replacement.room.room_uid, event.id))
}

async fn tickets(state: &AppState) -> TestResult<[String; 5]> {
    Ok([
        issue_side_chat_read_ticket(state, "general").await?.ticket,
        issue_message_search_read_ticket(state, "general")
            .await?
            .ticket,
        issue_message_search_read_ticket(state, "general")
            .await?
            .ticket,
        issue_message_pins_read_ticket(state, "general")
            .await?
            .ticket,
        issue_message_pins_write_ticket(state, "general")
            .await?
            .ticket,
    ])
}
