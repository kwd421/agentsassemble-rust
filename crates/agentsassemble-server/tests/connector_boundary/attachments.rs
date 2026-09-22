use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector_client::RoomConnectorClient;
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[tokio::test]
async fn connector_attachments_keep_pending_custody_and_revalidate_leave()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let mut clients = Vec::new();
    for scope in [
        InviteScope::ReadWrite,
        InviteScope::ReadWrite,
        InviteScope::ReadOnly,
    ] {
        let invite = store
            .create_connector_invite(&manager, Uuid::new_v4(), scope, chrono::Utc::now())
            .await?;
        let client = RoomConnectorClient::new(
            &format!("{}/join?token={}", server.base_url, invite.invite_bearer),
            "Attachment test",
            None,
        )?;
        client.join().await?;
        clients.push(client);
    }
    let writer = &clients[0];
    let other = &clients[1];
    let observer = &clients[2];
    let content = vec![b'x'; 70_000]; // Exceeds the former 64 KiB connector envelope.
    let upload = writer
        .upload_attachment("context.txt", "text/plain", &STANDARD.encode(&content))
        .await?;
    let id = upload["attachment"]["id"].as_str().ok_or("missing id")?;
    assert!(writer.read_attachment(id).await.is_err());
    assert!(
        observer
            .upload_attachment("denied.txt", "text/plain", "eA==")
            .await
            .is_err()
    );
    assert!(
        writer
            .upload_attachment("bad.png", "image/png", "eA==")
            .await
            .is_err()
    );
    assert!(
        other
            .command(
                RoomAction::MessageSend,
                json!({"content":"stolen pending", "attachment_ids":[id]})
            )
            .await
            .is_err()
    );
    let sent = writer
        .command(
            RoomAction::MessageSend,
            json!({"content":"", "attachment_ids":[id]}),
        )
        .await?;
    assert_eq!(sent["result"]["event"]["attachments"][0]["id"], id);
    assert_eq!(observer.read_attachment(id).await?.content, content);
    let image = writer.upload_attachment("pixel.png", "image/png", "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==").await?;
    let image_id = image["attachment"]["id"].as_str().ok_or("image id")?;
    writer
        .command(
            RoomAction::MessageSend,
            json!({"content":"image", "attachment_ids":[image_id]}),
        )
        .await?;
    assert!(observer.read_attachment(image_id).await?.is_image);
    for client in &clients {
        client
            .command(RoomAction::ParticipantLeave, json!({}))
            .await?;
        assert!(client.read_attachment(id).await.is_err());
        assert!(
            client
                .upload_attachment("ended.txt", "text/plain", "eA==")
                .await
                .is_err()
        );
        client.close();
    }
    server.stop().await;
    Ok(())
}
