use agentsassemble_domain::{AttachmentStorageUsage, InviteScope, LOCAL_OPERATOR_USER_ID};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{RoomManagerAuthority, RoomMutationAuthority, SqliteStore};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn retained_storage_conserves_all_parts_across_binding_deletion_and_expiry() -> TestResult {
    let (store, principal, _directory) = crate::agent_lifecycle::tests::fixture().await;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "empty", "Empty room")
        .await?;
    // Seed the four raster custody tables; message/connector transitions use their real owners.
    sqlx::query("INSERT INTO profile_avatar_assets VALUES ('profile', ?, 'avatar.png', 'image/png', zeroblob(11), 11, 'now', 'current', NULL)")
        .bind(LOCAL_OPERATOR_USER_ID).execute(&store.pool).await?;
    sqlx::query("INSERT INTO prejoin_avatar_assets VALUES ('prejoin', 'general', zeroblob(32), zeroblob(32), 'avatar.png', 'image/png', zeroblob(2), 2, 'now', 1)")
        .execute(&store.pool).await?;
    sqlx::query("INSERT INTO room_appearance_assets VALUES ('ra_00000000000000000000000000000000', 'general', NULL, 'room.png', 'image/png', zeroblob(3), 3, 'now', 'bound', NULL)")
        .execute(&store.pool).await?;
    sqlx::query("INSERT INTO agent_avatar_assets VALUES ('aa_00000000000000000000000000000000', 'general', ?, 'agent.png', 'image/png', zeroblob(4), 4, 'now', 'current', NULL)")
        .bind(crate::agent_lifecycle::tests::AGENT_ID).execute(&store.pool).await?;
    // Persona bodies have no stored size metadata and must not be included.
    sqlx::query("INSERT INTO persona_assets VALUES ('persona', '{}', zeroblob(19))")
        .execute(&store.pool)
        .await?;
    let human = store
        .store_message_attachment(&principal, "human.txt", "text/plain", vec![1; 5])
        .await?;
    let manager =
        RoomManagerAuthority::Local(crate::room_user_identity::test_authority(&store).await);
    let now = chrono::Utc::now();
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let connector = store
        .admit_connector(&fingerprint, &[7; 32], Uuid::new_v4(), "External", now)
        .await?;
    let authority = RoomMutationAuthority::ConnectorSession(&connector.authorization);
    let upload = store
        .store_authorized_message_attachment(authority, "connector.txt", "text/plain", vec![2; 7])
        .await?;
    let pending = read(&store).await?;
    assert_eq!(pending.rooms.len(), 2);
    assert_eq!(pending.rooms[0].room_id, "empty");
    assert_eq!(pending.rooms[0].label, "Empty room");
    assert_eq!((pending.rooms[0].files, pending.rooms[0].bytes), (0, 0));
    assert_eq!(pending.rooms[1].room_id, "general");
    assert_eq!(pending.rooms[1].label, "General");
    assert_eq!((pending.rooms[1].files, pending.rooms[1].bytes), (5, 21));
    assert_eq!((pending.other.files, pending.other.bytes), (1, 11));

    let sent = store
        .execute_message_with_turn(
            &principal,
            "storage-human-send",
            "message.send",
            &json!({"content":"", "attachment_ids":[human.id]}),
        )
        .await?;
    assert_eq!(read(&store).await?, pending);
    store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::ConnectorSession(&connector.authorization),
            "storage-connector-send",
            "message.send",
            &json!({"content":"", "attachment_ids":[upload.id]}),
        )
        .await?;
    assert_eq!(read(&store).await?, pending);
    store
        .execute_message_mutation(
            &principal,
            "storage-delete",
            "message.delete",
            &json!({"event_id":sent.outcome.event.id}),
        )
        .await?;
    let deleted = read(&store).await?;
    assert_eq!(deleted.total.files, pending.total.files - 1);
    assert_eq!(deleted.rooms[1].bytes, pending.rooms[1].bytes - 5);

    let expiring = store
        .store_message_attachment(&principal, "expire.txt", "text/plain", vec![3; 13])
        .await?;
    let retained = read(&store).await?;
    sqlx::query("UPDATE room_message_attachments SET created_at = 1, expires_at = 2 WHERE attachment_id = ?")
        .bind(&expiring.id).execute(&store.pool).await?;
    assert_eq!(
        read(&store).await?,
        retained,
        "expiry alone does not remove retained bytes"
    );
    store
        .store_message_attachment(&principal, "cleanup.txt", "text/plain", vec![4])
        .await?;
    let cleaned = read(&store).await?;
    assert_eq!(cleaned.total.files, retained.total.files);
    assert_eq!(cleaned.rooms[1].bytes, retained.rooms[1].bytes - 12);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM room_message_attachment_chunks WHERE attachment_id=?"
        )
        .bind(&expiring.id)
        .fetch_one(&store.pool)
        .await?,
        0
    );
    for kind in [
        crate::room_file_content::RoomFileContent::Message,
        crate::room_file_content::RoomFileContent::Connector,
    ] {
        kind.require_complete(&mut *store.pool.acquire().await?)
            .await?;
    }
    Ok(())
}

async fn read(store: &SqliteStore) -> Result<AttachmentStorageUsage, crate::PersistenceError> {
    let usage = store.attachment_storage_usage().await?;
    assert_eq!(
        usage.total.files,
        usage.other.files + usage.rooms.iter().map(|room| room.files).sum::<i64>()
    );
    assert_eq!(
        usage.total.bytes,
        usage.other.bytes + usage.rooms.iter().map(|room| room.bytes).sum::<i64>()
    );
    Ok(usage)
}
