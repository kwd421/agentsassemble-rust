use agentsassemble_domain::InviteScope;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::human_session_authority_tests::local_operator_principal;
use crate::{
    ConnectorFileAuthority, ConnectorSessionAuthorization, MESSAGE_UPLOAD_PIECE_BYTES,
    MessageAttachmentReadAuthority, MessageUploadAuthority, PersistenceError, RoomManagerAuthority,
    RoomMutationAuthority,
};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn fixture() -> Result<(crate::SqliteStore, RoomManagerAuthority), Box<dyn std::error::Error>>
{
    let store = crate::SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let authority = crate::room_user_identity::test_authority(&store).await;
    Ok((store, RoomManagerAuthority::Local(authority)))
}

async fn admit(
    store: &crate::SqliteStore,
    manager: &RoomManagerAuthority,
    scope: InviteScope,
    name: &str,
) -> Result<ConnectorSessionAuthorization, Box<dyn std::error::Error>> {
    let now = Utc::now();
    let invite = store
        .create_connector_invite(manager, Uuid::new_v4(), scope, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    Ok(store
        .admit_connector(&fingerprint, &[1; 32], Uuid::new_v4(), name, now)
        .await?
        .authorization)
}

async fn set_file_limit(store: &crate::SqliteStore, request_id: &str, limit: usize) -> TestResult {
    let operator = local_operator_principal();
    let snapshot = store.snapshot("general", 0, 100).await?;
    store
        .execute_room_settings_update(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            request_id,
            &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_file_limit_bytes":limit}),
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn connector_transfer_uses_current_limits_participant_custody_and_streamed_reads()
-> TestResult {
    let (store, manager) = fixture().await?;
    let authorization = admit(&store, &manager, InviteScope::ReadWrite, "Transfer AI").await?;
    let participant_id = authorization.principal().participant_id.clone();
    let custody = MessageUploadAuthority::Connector(Box::new(ConnectorFileAuthority {
        session_fingerprint: *authorization.session_fingerprint(),
        room_id: "general".into(),
        participant_id: participant_id.clone(),
    }));
    set_file_limit(&store, "file-limit-50", 50 * 1024 * 1024).await?;
    let size = 30 * 1024 * 1024;
    let transfer = store
        .begin_message_upload(
            &custody,
            "large",
            "../proof.bin",
            "application/octet-stream",
            size,
        )
        .await?;
    let piece = vec![71; MESSAGE_UPLOAD_PIECE_BYTES];
    let mut offset = 0;
    while offset < size {
        offset = store
            .append_message_upload(&custody, &transfer.handle, offset, &piece)
            .await?;
    }
    let completed = store
        .finish_message_upload(&custody, &transfer.handle)
        .await?;
    assert_eq!(completed.attachment.as_ref().unwrap().filename, "proof.bin");
    // Publishing binds the participant-owned pending upload to the room message.
    let publication = store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::ConnectorSession(&authorization),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"attached","attachment_ids":[completed.attachment_id]}),
        )
        .await?;
    assert_eq!(
        publication.outcome.event.extra["attachments"][0]["id"],
        json!(completed.attachment_id)
    );
    // Later reads stream the bound file piece-wise through the connector reader.
    let read_authority =
        MessageAttachmentReadAuthority::Connector(Box::new(ConnectorFileAuthority {
            session_fingerprint: *authorization.session_fingerprint(),
            room_id: "general".into(),
            participant_id: participant_id.clone(),
        }));
    let (reader, ()) = store
        .open_message_attachment_reader(read_authority, &completed.attachment_id, |_| Ok(()))
        .await?;
    for sequence in 0..size / MESSAGE_UPLOAD_PIECE_BYTES {
        assert_eq!(reader.read_piece(sequence).await?, piece);
    }
    // Existing stored bytes and new reservations share the same quota.
    let operator = local_operator_principal();
    let snapshot = store.snapshot("general", 0, 100).await?;
    store.execute_room_settings_update(RoomMutationAuthority::TrustedPrincipal(&operator), "quota", &json!({"expected_revision":agentsassemble_domain::public_settings(&snapshot.settings)?.settings_revision,"attachment_storage_limit_bytes":1024*1024*1024})).await?;
    for n in 0..33 {
        store
            .begin_message_upload(
                &custody,
                &format!("reserve-{n}"),
                "reserve.bin",
                "application/octet-stream",
                size,
            )
            .await?;
    }
    assert!(
        matches!(store.begin_message_upload(&custody, "quota-over", "refused.bin", "application/octet-stream", size).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="room_attachment_quota_reached")
    );
    // The transfer path is held to the current room limit; the inline path keeps
    // its fixed 10 MiB bound and exemption.
    set_file_limit(&store, "file-limit-10", 10 * 1024 * 1024).await?;
    assert!(
        matches!(store.begin_message_upload(&custody, "small-refused", "refused.bin", "application/octet-stream", size).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="attachment_too_large")
    );
    let inline = store
        .store_authorized_message_attachment(
            RoomMutationAuthority::ConnectorSession(&authorization),
            "inline.bin",
            "application/octet-stream",
            vec![3; 1024],
        )
        .await?;
    assert_eq!(inline.size, 1024);
    // A read-only connector cannot open transfer custody.
    let readonly = admit(&store, &manager, InviteScope::ReadOnly, "Observer AI").await?;
    let readonly_custody = MessageUploadAuthority::Connector(Box::new(ConnectorFileAuthority {
        session_fingerprint: *readonly.session_fingerprint(),
        room_id: "general".into(),
        participant_id: readonly.principal().participant_id.clone(),
    }));
    assert!(
        matches!(store.begin_message_upload(&readonly_custody, "conversation-refused", "refused.bin", "application/octet-stream", 1).await, Err(PersistenceError::CommandRejected { code, .. }) if code=="permission_denied")
    );
    // Per-piece authority rejects an ended session, even for an already-open large reader.
    store
        .leave_connector_session(&authorization, "connector-leave")
        .await?;
    assert!(reader.read_piece(0).await.is_err());
    assert!(
        store
            .append_message_upload(&custody, &transfer.handle, 0, &piece)
            .await
            .is_err()
    );
    Ok(())
}
