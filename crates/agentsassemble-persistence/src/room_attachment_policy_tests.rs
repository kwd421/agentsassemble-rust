use crate::{RoomMutationAuthority::TrustedPrincipal, SqliteStore};
use agentsassemble_domain::{RoomSettings, public_settings};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn policy_changes_require_manager_revision_and_survive_backup_validation() -> TestResult {
    let (store, manager) = super::fixture().await;
    let settings = RoomSettings::defaults("General");
    let revision = public_settings(&settings)?.settings_revision;
    let patch = json!({"expected_revision": revision, "attachment_file_limit_bytes": 26214400,
        "attachment_storage_limit_bytes": 524288000});
    let mut member = manager.clone();
    member.capabilities.room_manage = false;
    super::assert_rejected_code(
        store
            .execute_room_settings_update(TrustedPrincipal(&member), "denied", &patch)
            .await,
        "permission_denied",
    );
    let outcome = store
        .execute_room_settings_update(TrustedPrincipal(&manager), "policy", &patch)
        .await?;
    super::assert_rejected_code(
        store
            .execute_room_settings_update(TrustedPrincipal(&manager), "stale", &patch)
            .await,
        "settings_conflict",
    );
    let encoded: String =
        sqlx::query_scalar("SELECT settings_json FROM rooms WHERE room_id = 'general'")
            .fetch_one(&store.pool)
            .await?;
    let restored = crate::backup_policy::json(
        "rooms",
        "settings_json",
        &encoded,
        crate::schema_version::CURRENT_SCHEMA_VERSION,
    )?;
    assert_eq!(
        serde_json::from_str::<RoomSettings>(&restored)?,
        serde_json::from_str::<RoomSettings>(&encoded)?
    );
    let restored_event = crate::backup_policy::json(
        "room_events",
        "event_json",
        &serde_json::to_string(&outcome.event)?,
        crate::schema_version::CURRENT_SCHEMA_VERSION,
    )?;
    assert_eq!(
        serde_json::from_str::<agentsassemble_domain::RoomEvent>(&restored_event)?,
        outcome.event
    );
    assert_eq!(
        store
            .execute_room_settings_update(TrustedPrincipal(&manager), "policy", &patch)
            .await?
            .result,
        outcome.result
    );
    let next = store
        .execute_room_settings_update(
            TrustedPrincipal(&manager),
            "remove-quota",
            &json!({
                "expected_revision": outcome.result["room_settings"]["settings_revision"],
                "attachment_storage_limit_bytes": null,
            }),
        )
        .await?;
    assert!(
        next.result["room_settings"]
            .get("attachment_storage_limit_bytes")
            .is_none()
    );
    assert_eq!(
        next.result["room_settings"]["attachment_file_limit_bytes"],
        26214400
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_uploads_count_pending_bound_and_connector_bytes_once() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("quota.sqlite");
    let store = SqliteStore::open(path.to_str().ok_or("path")?).await?;
    store
        .bootstrap_local_authority("1468c3c6-a7f5-4311-b262-a4e492db1032", "Host")
        .await?;
    store
        .create_room_for_local_operator(
            "6f218ed3-8fe3-4487-86e4-3120c41fdd9e",
            "general",
            "General",
        )
        .await?;
    let (_, manager) = super::fixture().await;
    store.execute_room_settings_update(TrustedPrincipal(&manager), "quota", &json!({
        "expected_revision": public_settings(&RoomSettings::defaults("General"))?.settings_revision,
        "attachment_storage_limit_bytes": 524288000,
    })).await?;
    // Real retained BLOBs at the already supported schema ceiling. Nine pending
    // message files plus one connector file leave exactly one byte of room quota.
    for index in 0..9 {
        sqlx::query("INSERT INTO room_message_attachments SELECT 'ma_'||printf('%032x',?),room_id,user_id,NULL,'retained.bin','application/octet-stream',zeroblob(52428800),52428800,0,1,'pending',9223372036854775807 FROM rooms CROSS JOIN user_profiles WHERE room_id='general' LIMIT 1")
            .bind(index).execute(&store.pool).await?;
    }
    sqlx::query("INSERT INTO room_connector_uploads SELECT 'ma_'||printf('%032x',9),room_id,participant_id,'connector.bin','application/octet-stream',zeroblob(52428799),52428799,0,1,9223372036854775807 FROM participants WHERE room_id='general' LIMIT 1").execute(&store.pool).await?;
    let initial = format!("ma_{:032x}", 0);
    store
        .execute_message_with_turn(
            &manager,
            "bind-initial",
            "message.send",
            &json!({"content":"", "attachment_ids":[initial]}),
        )
        .await?;
    let barrier = tokio::sync::Barrier::new(2);
    let upload = || async {
        barrier.wait().await;
        store
            .store_message_attachment(&manager, "last.bin", "application/octet-stream", vec![42])
            .await
    };
    let (left, right) = tokio::join!(upload(), upload());
    let winner = match (left, right) {
        (Ok(winner), denied) | (denied, Ok(winner)) => {
            super::assert_rejected_code(denied, "room_attachment_quota_reached");
            winner
        }
        _ => panic!("exactly one upload must consume the final byte"),
    };
    store
        .execute_message_with_turn(
            &manager,
            "bind-at-quota",
            "message.send",
            &json!({"content":"", "attachment_ids":[winner.id]}),
        )
        .await?;
    let bytes: i64 = sqlx::query_scalar("SELECT SUM(size) FROM (SELECT size FROM room_message_attachments UNION ALL SELECT size FROM room_connector_uploads)").fetch_one(&store.pool).await?;
    assert_eq!(bytes, 524288000);
    super::assert_rejected_code(
        store
            .store_message_attachment(&manager, "extra.bin", "application/octet-stream", vec![1])
            .await,
        "room_attachment_quota_reached",
    );
    let read = store
        .bound_message_attachment(
            "general",
            &manager.principal_id,
            &manager.participant_id,
            &winner.id,
        )
        .await?;
    assert_eq!(read.content, [42]);
    store.close().await?;
    Ok(())
}
