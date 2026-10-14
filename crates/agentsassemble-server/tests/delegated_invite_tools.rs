use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

// An approved delegated invite is charged to the owner's existing write budget, so
// several director AIs cannot multiply that owner's allowance.
#[tokio::test]
async fn connector_create_human_invite_uses_a_fresh_owner_budget() -> TestResult {
    let (store, _) = human_invite::fixture(InviteScope::ReadWrite).await;
    let server = human_invite::start_with_public_mcp(
        store.clone(),
        "https://invite.example.test",
        "invite-fixture-proxy-00000000000001",
    )
    .await;
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "{}/mcp",
            server.base_url
        )))
        .await?;
    let manager = RoomManagerAuthority::Local(
        store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?,
    );
    let now = chrono::Utc::now();
    let _pairing = store
        .create_operator_pairing(&manager, &[61; 32], "https://invite.example.test", now)
        .await?;
    let paired = store
        .redeem_operator_pairing(&[61; 32], &[62; 32], "https://invite.example.test", now)
        .await?;
    let invite = store
        .create_connector_invite(
            &RoomManagerAuthority::Operator(Box::new(paired.authorization)),
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let mut join = json!({"invite_url":format!("{}/join?token={}",server.base_url,invite.invite_bearer),"display_name":"Director"});
    join["connection_id"] = call(&client, "room_join", join.clone()).await["connection_id"].clone();
    let joined = call(&client, "room_join", join.clone()).await;
    role(
        &store,
        joined["participant_id"].as_str().unwrap(),
        "director",
    )
    .await?;
    verify(
        &client,
        &store,
        joined["participant_id"].as_str().unwrap(),
        Some(&joined["connection_id"]),
        (&server.base_url, server.state()),
    )
    .await?;
    client.cancel().await?;
    server.stop().await;
    Ok(())
}

// One end-to-end pass for the owner-confirmed HUMAN room invite on the connector entry:
// the pending request changes nothing, only the exact owner approves, the approval mints
// the invite through the owner's own core and returns the link only in the owner's live
// acknowledgement, and that link admits a person through the existing join path. The
// credential never reaches a room event, an AI-visible result or a replayed decision.
pub(super) async fn verify(
    client: &Client,
    store: &SqliteStore,
    _delegate: &str,
    connection: Option<&Value>,
    owner: (&str, &AppState),
) -> TestResult {
    let (base_url, _) = owner;
    let invites_before = store.list_human_invites().await?.len();
    let before = store.snapshot("general", 0, 400).await?;
    let mut args = json!({
        "request_id": Uuid::new_v4().to_string(),
        "display_name": "Delegated Guest",
        "invite_scope": "read_only",
        "ttl_seconds": 600,
        "max_uses": 2,
    });
    if let Some(connection) = connection {
        args["connection_id"] = connection.clone();
    }
    let pending = call(client, "request_create_invite", args.clone()).await;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let confirmation_id = pending["confirmation_id"]
        .as_str()
        .ok_or("missing confirmation")?
        .to_owned();
    // Nothing changed: room settings are untouched and no invite row or credential exists.
    let after = store.snapshot("general", 0, 400).await?;
    assert_eq!(after.settings, before.settings);
    assert_eq!(after.agent_sessions, before.agent_sessions);
    assert_eq!(store.list_human_invites().await?.len(), invites_before);
    assert_eq!(
        pending,
        call(client, "request_create_invite", args.clone()).await
    );
    // Changed parameters under the same identity conflict instead.
    let mut changed = args.clone();
    changed["max_uses"] = json!(9);
    let conflict = room_portal_fixture::call_tool(client, "request_create_invite", changed).await;
    assert_eq!(conflict.is_error, Some(true));
    assert!(
        conflict.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("request_conflict"),
        "{conflict:?}"
    );
    // The owner approves through the room socket; the approval mints the invite and
    // returns the credential only in the owner's live acknowledgement.
    let ack = super::decide_on_socket(owner, &confirmation_id, "approve", None).await;
    assert_eq!(ack["op"], "ack", "{ack}");
    assert_eq!(ack["result"]["state"], "executing", "{ack}");
    let invite = &ack["result"]["invite"];
    let invite_token = invite["invite_token"]
        .as_str()
        .ok_or("invite token missing")?
        .to_owned();
    let join_code = invite["join_code"]
        .as_str()
        .ok_or("join code missing")?
        .to_owned();
    let join_url = invite["join_url"]
        .as_str()
        .ok_or("join url missing")?
        .to_owned();
    assert!(invite_token.starts_with("aai1."), "{invite_token}");
    assert!(join_code.starts_with("aaj1_"), "{join_code}");
    assert!(join_url.contains(&join_code), "{join_url}");
    assert_eq!(invite["invite_scope"], "read_only");
    assert_eq!(invite["max_uses"], 2);
    // The link admits a person through the existing join path.
    let http = reqwest::Client::new();
    let joined = human_invite::join(
        &http,
        base_url,
        &invite_token,
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xCC; 32])),
        &Uuid::new_v4().to_string(),
        "Delegated Guest",
        "",
    )
    .await;
    assert_eq!(joined["participant_type"], "human", "{joined}");
    // The AI observes only the recorded success, with no invite id, code or URL.
    let done = call(client, "request_create_invite", args).await;
    assert_eq!(done["status"], "succeeded", "{done}");
    assert_eq!(done["confirmation_id"], confirmation_id);
    assert!(done.get("target_id").is_none());
    for secret in [&invite_token, &join_code, &join_url] {
        assert!(
            !serde_json::to_string(&done)?.contains(secret.as_str()),
            "AI result carries a credential: {done}"
        );
    }
    // The stored invite row matches the bound policy.
    let stored = store.list_human_invites().await?;
    let row = stored
        .iter()
        .find(|row| row.display_name == "Delegated Guest")
        .ok_or("stored invite missing")?
        .clone();
    assert_eq!(row.invite_scope, InviteScope::ReadOnly);
    assert_eq!(row.max_uses, 2);
    assert_eq!(row.created_by_user_id, LOCAL_OPERATOR_USER_ID);
    assert_eq!(row.room_id, "general");
    // No room event carries the link, code, token or invite id.
    let snapshot = store.snapshot("general", 0, 600).await?;
    for event in &snapshot.events {
        let encoded = serde_json::to_string(event)?;
        for secret in [
            invite_token.as_str(),
            join_code.as_str(),
            join_url.as_str(),
            row.invite_id.as_str(),
        ] {
            assert!(
                !encoded.contains(secret),
                "room event carries a credential: {encoded}"
            );
        }
    }
    // A replayed decision returns the receipt without the link.
    let replay = super::decide_on_socket(owner, &confirmation_id, "approve", None).await;
    assert_eq!(replay["op"], "ack", "{replay}");
    assert_eq!(replay["result"]["state"], "succeeded", "{replay}");
    assert!(replay["result"].get("invite").is_none(), "{replay}");
    let replay_encoded = serde_json::to_string(&replay)?;
    for secret in [&invite_token, &join_code, &join_url] {
        assert!(
            !replay_encoded.contains(secret.as_str()),
            "replayed decision carries a credential: {replay}"
        );
    }
    Ok(())
}
