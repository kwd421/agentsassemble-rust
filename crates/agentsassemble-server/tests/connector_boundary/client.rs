use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector_client::RoomConnectorClient;

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
