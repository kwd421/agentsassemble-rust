use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector::client::RoomConnectorClient;
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
    // Humans may retain larger files; connector reads still fail with a specific
    // metadata-only refusal that survives the real HTTP client mapping.
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, public_settings,
    };
    use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;
    let principal = AuthenticatedPrincipal {
        principal_id: super::LOCAL_OPERATOR_USER_ID.into(),
        participant_id: super::LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        room_id: "general".into(),
        display_name: "Host".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let settings = store.snapshot("general", 0, 100).await?.settings;
    store.execute_room_settings_update(TrustedPrincipal(&principal), "large-policy", &json!({
        "expected_revision":public_settings(&settings)?.settings_revision,"attachment_file_limit_bytes":26214400
    })).await?;
    let large = store
        .store_message_attachment(
            &principal,
            "large.bin",
            "application/octet-stream",
            vec![42; 10485761],
        )
        .await?;
    store
        .execute_message_with_turn(
            &principal,
            "large-file",
            "message.send",
            &json!({"content":"", "attachment_ids":[large.id]}),
        )
        .await?;
    let refusal = observer
        .read_attachment(&large.id)
        .await
        .expect_err("oversized connector read");
    assert_eq!(refusal.code, "attachment_too_large");
    assert!(refusal.message.contains("10485761") && refusal.message.contains("10485760"));
    assert!(refusal.to_string().contains("attachment_too_large"));
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
