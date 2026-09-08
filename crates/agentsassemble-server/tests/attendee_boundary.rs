use agentsassemble_domain::{
    FriendDetails, FriendParticipantType, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, SaveFriend,
};
use agentsassemble_persistence::RoomManagerAuthority;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};
use uuid::Uuid;

#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_http_publishes_external_custody_without_host_provider_discovery() -> TestResult {
    let (store, invite) = fixture().await?;
    // The fixture deliberately supplies an empty host catalog. The provider belongs to the caller.
    let server = human_invite::start(store.clone()).await;
    let mut events = server.rooms().subscribe("general").await;
    let client = Client::new();
    let endpoint = format!("{}/api/room-attendee/join", server.base_url);
    let mut body = json!({"request_id":Uuid::new_v4(),"client_secret":URL_SAFE_NO_PAD.encode([7;32]),
        "provider":"opencode", "display_name":"External Codex"});
    for (provider, expected_code) in [
        ("opencode", "provider_mismatch"),
        ("freebuff", "unsupported_provider"),
        ("antigravity", "unsupported_provider"),
    ] {
        body["provider"] = json!(provider);
        let response = client
            .post(&endpoint)
            .bearer_auth(&invite.invite_bearer)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), 403);
        let error: Value = response.json().await?;
        assert_eq!(error["error"]["code"], expected_code);
    }
    assert!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .is_empty()
    );
    body["provider"] = json!("codex");
    let response = client
        .post(&endpoint)
        .bearer_auth(&invite.invite_bearer)
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let joined: Value = response.json().await?;
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = events.recv().await?;
            if event.event_type == "agent_session_created" {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(event.actor.participant_id, joined["participant_id"]);
    assert_eq!(
        event.extra["agent_session"]["process_ownership"],
        "external"
    );
    assert_eq!(event.extra["agent_session"]["enabled"], false);
    assert_eq!(
        event.extra["agent_session"]["provider_session_active"],
        false
    );
    verify_retry_and_isolation(
        &client,
        &endpoint,
        &server.base_url,
        &invite.invite_bearer,
        &body,
        &joined,
    )
    .await?;
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .len(),
        1
    );
    server.stop().await;
    Ok(())
}

async fn verify_retry_and_isolation(
    client: &Client,
    endpoint: &str,
    base_url: &str,
    invite: &str,
    body: &Value,
    joined: &Value,
) -> TestResult {
    let retry: Value = client
        .post(endpoint)
        .bearer_auth(invite)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(retry["deduplicated"], true);
    let same_credential = retry["session_bearer"] == joined["session_bearer"];
    assert!(same_credential);
    assert_eq!(retry["participant_id"], joined["participant_id"]);
    assert_eq!(retry["expires_at"], joined["expires_at"]);
    let bearer = joined["session_bearer"]
        .as_str()
        .ok_or("attendee session missing")?;
    assert_eq!(
        client
            .post(endpoint)
            .bearer_auth(bearer)
            .json(body)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base_url}/api/room-connector/read"))
            .bearer_auth(bearer)
            .send()
            .await?
            .status(),
        401
    );
    assert!(
        !client
            .post(format!("{base_url}/api/session-tickets/socket"))
            .bearer_auth(bearer)
            .send()
            .await?
            .status()
            .is_success()
    );
    let mut competing = body.clone();
    competing["client_secret"] = json!(URL_SAFE_NO_PAD.encode([8; 32]));
    assert_eq!(
        client
            .post(endpoint)
            .bearer_auth(invite)
            .json(&competing)
            .send()
            .await?
            .status(),
        403
    );
    Ok(())
}

async fn fixture() -> Result<
    (
        agentsassemble_persistence::SqliteStore,
        agentsassemble_persistence::AttendeeInvite,
    ),
    Box<dyn std::error::Error>,
> {
    let (store, _) = human_invite::fixture(InviteScope::ReadWrite).await;
    let manager = RoomManagerAuthority::Local(
        store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?,
    );
    let friend = store
        .save_friend(&SaveFriend {
            friend_id: Uuid::new_v4(),
            expected_revision: 0,
            details: FriendDetails {
                display_name: "External Codex".to_owned(),
                handle: String::new(),
                participant_type: FriendParticipantType::SubscriptionAi,
                provider_kind: "codex_live_session".to_owned(),
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
    let invite = store
        .create_friend_attendee_invite(
            &manager,
            Uuid::new_v4(),
            friend.friend_id,
            chrono::Utc::now(),
        )
        .await?;
    Ok((store, invite))
}
