use super::*;
use agentsassemble_domain::{FriendDetails, FriendParticipantType, InviteScope, SaveFriend};
use agentsassemble_server::RoomAttendeeClient;
use uuid::Uuid;
#[path = "../support/human_invite.rs"]
mod human_invite;
#[path = "../support/room_socket_peer.rs"]
mod room_socket_peer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn saved_ai_packet_uses_exact_manager_purpose_and_normalized_attendee_admission() -> TestResult
{
    let server = start(true).await;
    let client = reqwest::Client::new();
    let friend = save_friend(&server.store, "codex").await?;
    let manager = server
        .store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let request = json!({"request_id":Uuid::new_v4(),"friend_id":friend});
    let endpoint = format!("{}/api/room-attendee/friend-invite", server.base_url);
    let wrong = server
        .tickets
        .issue_connector_invite_create(manager.clone())
        .await?;
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(wrong.ticket)
            .json(&request)
            .send()
            .await?
            .status(),
        401
    );
    let mut packet = None;
    for _ in 0..2 {
        let ticket = server
            .tickets
            .issue_attendee_invite_create(manager.clone())
            .await?;
        let response = client
            .post(&endpoint)
            .bearer_auth(ticket.ticket)
            .json(&request)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let value: Value = response.json().await?;
        if let Some(first) = &packet {
            assert_eq!(first, &value);
        }
        packet = Some(value);
        server.store.delete_friend(friend).await?;
    }
    let packet = packet.ok_or("packet missing")?;
    assert_eq!(
        packet["attend_command"],
        "assemble room attend --provider codex"
    );
    assert_eq!(packet["room_uid"], manager.room_uid.to_string());
    assert!(
        server
            .store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .is_empty()
    );
    let mut attendee = packet_client(&server, &packet)?;
    assert_eq!(attendee.join().await?.provider_kind, "codex_live_session");
    agentsassemble_server::shutdown_attendee(&attendee, None, None).await?;
    let unsupported = save_friend(&server.store, "antigravity").await?;
    let ticket = server.tickets.issue_attendee_invite_create(manager).await?;
    let response = client
        .post(endpoint)
        .bearer_auth(ticket.ticket)
        .json(&json!({"request_id":Uuid::new_v4(),"friend_id":unsupported}))
        .send()
        .await?;
    assert_eq!(response.status(), 403);
    assert_eq!(
        response.json::<Value>().await?["error"]["code"],
        "unsupported_provider"
    );
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn companion_packet_requires_posting_human_and_keeps_its_parent_custody() -> TestResult {
    let server = start(true).await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/api/room-attendee/companion-invite", server.base_url);
    let request =
        json!({"request_id":Uuid::new_v4(),"provider":"codex","display_name":"Human companion"});
    let manager_ticket = issue_create(&server, "general").await;
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(manager_ticket)
            .json(&request)
            .send()
            .await?
            .status(),
        401
    );
    for scope in [InviteScope::ReadOnly, InviteScope::ReadWrite] {
        let credentials = human_invite::persist_invite(
            &server.store,
            scope,
            1,
            &Uuid::new_v4().to_string(),
            "Companion owner",
        )
        .await;
        let joined = human_invite::join(
            &client,
            &server.base_url,
            credentials.invite_token(),
            &format!(
                "{}{}",
                agentsassemble_protocol::BROWSER_CREDENTIAL_PREFIX,
                base64::Engine::encode(
                    &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                    [11_u8; agentsassemble_protocol::BROWSER_CREDENTIAL_BYTES]
                )
            ),
            &Uuid::new_v4().to_string(),
            "Companion owner",
            "",
        )
        .await;
        let bearer = joined["session_token"]
            .as_str()
            .ok_or("human credential missing")?;
        let response = client
            .post(&endpoint)
            .bearer_auth(bearer)
            .json(&request)
            .send()
            .await?;
        if scope == InviteScope::ReadOnly {
            assert_eq!(response.status(), 403);
            continue;
        }
        assert_eq!(response.status(), 200);
        let packet: Value = response.json().await?;
        let retry: Value = client
            .post(&endpoint)
            .bearer_auth(bearer)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(packet, retry);
        let mut attendee = packet_client(&server, &packet)?;
        let admission = attendee.join().await?;
        assert_eq!(admission.provider_kind, "codex_live_session");
        let left = client
            .post(format!("{}/api/room-invite/leave", server.base_url))
            .bearer_auth(bearer)
            .json(&json!({}))
            .send()
            .await?;
        assert_eq!(left.status(), 200);
        assert!(
            client
                .post(&endpoint)
                .bearer_auth(bearer)
                .json(&request)
                .send()
                .await?
                .status()
                .is_client_error()
        );
        let denied = attendee
            .connect()
            .await
            .err()
            .ok_or("departed parent retained ordinary attendee access")?;
        assert_eq!(
            denied.resolution,
            Some(agentsassemble_protocol::CommandResolution::Rejected)
        );
        agentsassemble_server::shutdown_attendee(&attendee, None, None).await?;
    }
    server.stop().await;
    Ok(())
}

fn packet_client(
    server: &RunningServer,
    packet: &Value,
) -> Result<RoomAttendeeClient, Box<dyn std::error::Error>> {
    let url = url::Url::parse(packet["join_url"].as_str().ok_or("join URL missing")?)?;
    assert_eq!(url.origin().ascii_serialization(), PUBLIC_ORIGIN);
    let token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .ok_or("invite missing")?
        .1
        .into_owned();
    Ok(RoomAttendeeClient::new(
        &format!("{}/join?token={token}", server.base_url),
        "codex",
        "Packet client",
    )?)
}

async fn save_friend(
    store: &SqliteStore,
    provider: &str,
) -> Result<Uuid, agentsassemble_persistence::PersistenceError> {
    let friend = store
        .save_friend(&SaveFriend {
            friend_id: Uuid::new_v4(),
            expected_revision: 0,
            details: FriendDetails {
                display_name: "Saved AI".to_owned(),
                handle: String::new(),
                participant_type: FriendParticipantType::SubscriptionAi,
                provider_kind: provider.to_owned(),
                connection_kind: "external".to_owned(),
                agent_id: String::new(),
                source_agent_id: String::new(),
                last_meeting_id: String::new(),
                status: "offline".to_owned(),
                source: "manual".to_owned(),
                last_seen_at: None,
            },
        })
        .await?;
    Ok(friend.friend_id)
}
