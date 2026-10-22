use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector::client::RoomConnectorClient;

#[tokio::test]
async fn current_conversation_client_keeps_wait_and_command_custody_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let mut clients = Vec::new();
    for scope in [InviteScope::ReadWrite, InviteScope::ReadOnly] {
        let invite = store
            .create_connector_invite(&manager, Uuid::new_v4(), scope, chrono::Utc::now())
            .await?;
        let link = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
        assert!(
            RoomConnectorClient::new(
                &link,
                "Blocked",
                Some(&[url::Url::parse("https://other.example.test/")?])
            )
            .is_err()
        );
        let client = RoomConnectorClient::new(
            &link,
            "Current AI",
            Some(&[url::Url::parse(&server.base_url)?]),
        )?;
        let joined = client.join().await?;
        assert_eq!(client.join().await?.participant_id, joined.participant_id);
        assert!(!serde_json::to_string(&joined)?.contains(&invite.invite_bearer));
        clients.push(client);
    }
    let writer = &clients[0];
    let observer = &clients[1];
    let pending = observer.wait_next();
    let contribution = writer.command(
        RoomAction::MessageSend,
        json!({"content":"Current conversation delivery"}),
    );
    let (received, sent) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(pending, contribution)
    })
    .await??;
    assert_eq!(
        received["messages"][0]["content"],
        "Current conversation delivery"
    );
    assert_eq!(received["last_seq"], sent["result"]["event_seq"]);
    let history = observer.read().await?;
    assert_eq!(
        history["messages"][0]["content"],
        "Current conversation delivery"
    );
    assert!(
        observer
            .command(RoomAction::MessageSend, json!({"content":"not allowed"}))
            .await
            .is_err()
    );
    observer
        .command(RoomAction::ParticipantLeave, json!({}))
        .await?;
    observer.close();
    assert!(observer.read().await.is_err());
    writer
        .command(RoomAction::ParticipantLeave, json!({}))
        .await?;
    writer.close();
    assert!(writer.join().await.is_err());
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn normal_connector_silence_outlives_the_http_connection_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let mut clients = Vec::new();
    for name in ["Waiting reader", "Delayed writer"] {
        let invite = store
            .create_connector_invite(
                &manager,
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                chrono::Utc::now(),
            )
            .await?;
        let client = RoomConnectorClient::new(
            &format!("{}/join?token={}", server.base_url, invite.invite_bearer),
            name,
            None,
        )?;
        client.join().await?;
        clients.push(client);
    }
    let pending = clients[0].wait_next();
    tokio::pin!(pending);
    // This duration crosses the production 30-second connection deadline itself;
    // it is not a sleep used to guess whether a concurrency interleaving occurred.
    tokio::select! {
        result = &mut pending => panic!("normal silence ended the wait: {result:?}"),
        () = tokio::time::sleep(std::time::Duration::from_secs(31)) => {},
    }
    let (received, sent) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(
            pending,
            clients[1].command(
                RoomAction::MessageSend,
                json!({"content":"After the old HTTP deadline"}),
            )
        )
    })
    .await??;
    assert_eq!(
        received["messages"][0]["content"],
        "After the old HTTP deadline"
    );
    assert_eq!(received["last_seq"], sent["result"]["event_seq"]);
    for client in &clients {
        client
            .command(RoomAction::ParticipantLeave, json!({}))
            .await?;
    }
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn a_renamed_external_ai_is_told_its_new_name_once() -> Result<(), Box<dyn std::error::Error>>
{
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, LOCAL_OPERATOR_PARTICIPANT_ID,
        LOCAL_OPERATOR_USER_ID,
    };
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let link = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let client =
        RoomConnectorClient::new(&link, "Ace", Some(&[url::Url::parse(&server.base_url)?]))?;
    let joined = client.join().await?;
    assert!(client.read().await?.get("name_changed").is_none());
    let owner = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    store
        .execute_agent_profile_update(
            agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&owner),
            "rename-ace",
            &json!({"agent_id": joined.participant_id, "display_name": "Ace 2"}),
        )
        .await?;
    // A wait that brings a message carries the notice the first time.
    let pending = client.wait_next();
    let hello = json!({"content": "hello"});
    let said = store.execute_message_with_turn(&owner, "say-after-rename", "message.send", &hello);
    let (told, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(async { pending.await.map_err(|error| error.code) }, async {
            said.await.map_err(|error| error.to_string())
        })
    })
    .await??;
    assert_eq!(told["your_display_name"], "Ace 2");
    assert!(
        told["name_changed"]
            .as_str()
            .is_some_and(|notice| notice.contains("\"Ace 2\""))
    );
    assert!(client.read().await?.get("name_changed").is_none());
    // A rejoin reports the current name.
    assert_eq!(client.join().await?.display_name, "Ace 2");
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn an_external_ai_gets_its_owners_instructions_and_hears_when_they_change()
-> Result<(), Box<dyn std::error::Error>> {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, LOCAL_OPERATOR_PARTICIPANT_ID,
        LOCAL_OPERATOR_USER_ID,
    };
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, chrono::Utc::now())
        .await?;
    let link = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let client = RoomConnectorClient::new(&link, "Ace", Some(&[url::Url::parse(&server.base_url)?]))?;
    let joined = client.join().await?;
    let first = client.read().await?;
    assert_eq!(first["your_instructions"], "");
    assert!(first.get("instructions_changed").is_none());
    store
        .set_agent_instructions("general", &joined.participant_id, "Review only Rust files.", chrono::Utc::now())
        .await?;
    let owner = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    // A wait points the AI at room_read; the read carries the text.
    let pending = client.wait_next();
    let hello = json!({"content": "hello"});
    let said = store.execute_message_with_turn(&owner, "say-after-instructions", "message.send", &hello);
    let (waited, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(async { pending.await.map_err(|error| error.code) }, async { said.await.map_err(|error| error.to_string()) })
    })
    .await??;
    assert!(waited["instructions_changed"].as_str().is_some_and(|notice| notice.contains("room_read")));
    let read = client.read().await?;
    assert_eq!(read["your_instructions"], "Review only Rust files.");
    assert!(read.get("instructions_changed").is_none());
    // Clearing them is a change the AI hears about too.
    store
        .set_agent_instructions("general", &joined.participant_id, "", chrono::Utc::now())
        .await?;
    let cleared = client.read().await?;
    assert_eq!(cleared["your_instructions"], "");
    assert!(cleared["instructions_changed"].as_str().is_some_and(|notice| notice.contains("removed")));
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn a_mention_only_external_ai_wakes_only_for_messages_that_mention_it()
-> Result<(), Box<dyn std::error::Error>> {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, LOCAL_OPERATOR_PARTICIPANT_ID,
        LOCAL_OPERATOR_USER_ID,
    };
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, chrono::Utc::now())
        .await?;
    let link = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let client = RoomConnectorClient::new(&link, "Quiet", Some(&[url::Url::parse(&server.base_url)?]))?;
    let joined = client.join().await?;
    assert!(store.set_agent_mention_only("general", &joined.participant_id, true).await?);
    let owner = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let pending = client.wait_next();
    let plain = json!({"content": "just chatting"});
    // A name typed by hand calls it too, as it would an app AI.
    let called = json!({"content": "@Quiet your turn"});
    let say = async {
        store.execute_message_with_turn(&owner, "plain", "message.send", &plain).await?;
        store.execute_message_with_turn(&owner, "called", "message.send", &called).await
    };
    let (woken, _) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(async { pending.await.map_err(|error| error.code) }, async { say.await.map_err(|error| error.to_string()) })
    })
    .await??;
    let messages = woken["messages"].as_array().ok_or("messages missing")?;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0]["content"].as_str().is_some_and(|content| content.ends_with("your turn")));
    server.stop().await;
    Ok(())
}
