use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, SIDE_CHAT_MAX_MESSAGES, SideChatSnapshot,
};
use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_server::{issue_message_search_read_ticket, issue_side_chat_read_ticket};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, StatusCode};
use serde_json::json;

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}
use support::human_invite::{canonical_session_token, fixture, join, start};

#[tokio::test]
async fn side_chat_bootstrap_is_bounded_private_and_exactly_authorized()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let generation = populate_side_chat(&store).await?;
    let server = start(store).await;
    let client = Client::new();
    let url = format!("{}/api/side-chat?room_id=general", server.base_url);
    let ticket = issue_side_chat_read_ticket(server.state(), "general").await?;
    let response = client.get(&url).bearer_auth(&ticket.ticket).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = response.bytes().await?;
    assert!(bytes.len() > agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES);
    let snapshot: SideChatSnapshot = serde_json::from_slice(&bytes)?;
    assert_eq!(snapshot.messages.len(), SIDE_CHAT_MAX_MESSAGES);
    assert_eq!(snapshot.generation, generation);
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&ticket.ticket)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let wrong = issue_message_search_read_ticket(server.state(), "general").await?;
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&wrong.ticket)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!(
                "{}/api/room-search?room_id=general&q=text",
                server.base_url
            ))
            .bearer_auth(wrong.ticket)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let admission = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x58; 32])),
        "a23e4567-e89b-12d3-a456-426614174000",
        "Side Reader",
        "",
    )
    .await;
    let token = canonical_session_token(&admission);
    let human = client.get(&url).bearer_auth(token).send().await?;
    assert_eq!(human.status(), StatusCode::OK);
    assert_eq!(human.json::<SideChatSnapshot>().await?, snapshot);
    assert_eq!(
        client
            .get(format!("{}/api/side-chat?room_id=other", server.base_url))
            .bearer_auth(token)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(format!("{}/api/room-invite/leave", server.base_url))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client.get(&url).bearer_auth(token).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    server.stop().await;
    Ok(())
}

async fn populate_side_chat(
    store: &agentsassemble_persistence::SqliteStore,
) -> Result<uuid::Uuid, Box<dyn std::error::Error>> {
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "SeiNel".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let now = chrono::Utc::now();
    let initial = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    for index in 0..SIDE_CHAT_MAX_MESSAGES {
        store.execute_side_chat(TrustedPrincipal(&principal), &index.to_string(),
            &json!({"generation":initial.generation,"after_seq":index,"content":"😀".repeat(2000)}), now).await?;
    }
    Ok(initial.generation)
}
