use crate::human_admission_store::tests::{admitted, fixture, insert_invite, prepared};
use crate::{
    backup_codec::{BackupManifest, measure_payload},
    restore_activation::RestoreReceipt,
    restore_job::RestoreJob,
    *,
};
use agentsassemble_domain::InviteScope;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row};
type ResultTest = Result<(), Box<dyn std::error::Error>>;

async fn restored(
    source: &SqliteStore,
    directory: &std::path::Path,
) -> Result<SqliteStore, Box<dyn std::error::Error>> {
    let payload = directory.join("payload.sqlite");
    let mut output = crate::backup_database::create_payload(&payload).await?;
    let mut connection = source.pool.acquire().await?;
    crate::backup_database::reconstruct(&mut connection, &mut output, false).await?;
    drop(connection);
    output.close().await?;
    let (rooms, messages, assets) = crate::backup_database::payload_counts(&payload).await?;
    let (database_bytes, database_sha256) = measure_payload(&mut std::fs::File::open(&payload)?)?;
    let manifest = BackupManifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        server_id: source.host_identity().await?.server_id().into(),
        server_name: "Test".into(),
        issuer: "https://central.example".into(),
        owner_person_id: "owner".into(),
        source_epoch: "epoch".into(),
        source_key_fingerprint: "x".repeat(43),
        snapshot_at: Utc::now(),
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let target = directory.join("runtime.sqlite3");
    let mut job = RestoreJob::begin(&target)?;
    job.prepare(&payload, manifest).await?;
    job.bind_switch("epoch", &"y".repeat(43))?;
    job.mark_switch_unknown()?;
    job.record_commit(&RestoreReceipt {
        endpoint_generation: 4_000_000_000_000,
        operation_id: job.journal().operation_id.to_string(),
        expected_epoch: "epoch".into(),
        new_key_fingerprint: "y".repeat(43),
        registration_epoch: "new-epoch".into(),
        status: "committed".into(),
    })?;
    job.activate().await?;
    drop(job);
    Ok(SqliteStore::open_path(&target).await?)
}

#[tokio::test]
async fn restored_host_denies_old_sessions_and_reconnects_exact_membership_and_pending_companion()
-> ResultTest {
    let (source, now) = fixture().await;
    source.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&source, [1; 32], [2; 32], "member", 1, now).await;
    let member = MemberAdmission {
        secure: None,
        projection_id: "old-projection".into(),
        issuer: "https://central.example".into(),
        person_id: "member-person".into(),
        display_name: "Member".into(),
        registration_epoch: "epoch".into(),
        challenge_fingerprint: [4; 32],
        challenge_expires_at: now + Duration::minutes(5),
    };
    let old = admitted(
        source
            .admit_human(
                &prepared(
                    [2; 32],
                    [3; 32],
                    &uuid::Uuid::new_v4().to_string(),
                    "Member",
                )
                .with_member(member.clone()),
                now,
            )
            .await?,
    );
    let old_fingerprint: [u8; 32] = Sha256::digest(old.session_bearer().as_bytes()).into();
    let human = source.authorize_human_session(&old_fingerprint).await?;
    let owner = RoomSessionAuthorization::Human(human.clone());
    let pending = source
        .create_companion_attendee_invite(
            &owner,
            CompanionInviteRequest {
                request_id: uuid::Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Pending AI",
            },
            now,
        )
        .await?;
    let used = source
        .create_companion_attendee_invite(
            &owner,
            CompanionInviteRequest {
                request_id: uuid::Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Used AI",
            },
            now,
        )
        .await?;
    let pending_fingerprint = Sha256::digest(pending.invite_bearer.as_bytes()).into();
    let used_fingerprint = Sha256::digest(used.invite_bearer.as_bytes()).into();
    let attendee = source
        .admit_attendee(
            AttendeeAdmissionRequest {
                invite_fingerprint: &used_fingerprint,
                client_fingerprint: &[11; 32],
                request_id: uuid::Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Used AI",
            },
            now,
        )
        .await?;
    let manager =
        RoomManagerAuthority::Local(crate::room_user_identity::test_authority(&source).await);
    let connector_invite = source
        .create_connector_invite(&manager, uuid::Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let connector_fp = Sha256::digest(connector_invite.invite_bearer.as_bytes()).into();
    let connector = source
        .admit_connector(
            &connector_fp,
            &[12; 32],
            uuid::Uuid::new_v4(),
            "Connector",
            now,
        )
        .await?;
    let unredeemed = source
        .create_connector_invite(&manager, uuid::Uuid::new_v4(), InviteScope::ReadOnly, now)
        .await?;
    let unredeemed_fp = Sha256::digest(unredeemed.invite_bearer.as_bytes()).into();
    let binding = OwnerAdmissionBinding {
        secure: None,
        entry_fingerprint: [20; 32],
        server_id: source.host_identity().await?.server_id().into(),
        person_id: "owner-person".into(),
        device_id: "owner-device".into(),
        browser_fingerprint: [21; 32],
        origin: "https://old.example".into(),
        generation: source.next_central_endpoint_generation().await?,
    };
    let entry = crate::host_owner_session_tests::admission(binding.clone())?;
    let host_owner = source
        .create_owner_session(&entry, &crate::host_owner_session_tests::description()?)
        .await?;
    let server_owner =
        ServerOwnerAuthority::CentralSession(Box::new(host_owner.authorization.clone()));
    let room_uid = source.snapshot("general", 0, 200).await?.room.room_uid;
    let operator = source
        .create_central_owner_session(
            &server_owner,
            &CentralOwnerSessionRequest::host_owned(
                "general",
                room_uid,
                &[22; 32],
                &binding.browser_fingerprint,
                &binding.origin,
                now,
            ),
        )
        .await?;
    let roles_before: Vec<(String, String)> =
        sqlx::query_as("SELECT participant_id,json_extract(participant_json,'$.role') FROM participants WHERE json_extract(participant_json,'$.participant_type')='human' ORDER BY participant_id")
            .fetch_all(&source.pool)
            .await?;
    let directory = tempfile::tempdir()?;
    let target = restored(&source, directory.path()).await?;
    assert!(
        target
            .authorize_human_session(&old_fingerprint)
            .await
            .is_err()
    );
    assert!(
        target
            .revalidate_human_session_authorization(&human)
            .await
            .is_err()
    );
    assert!(
        target
            .revalidate_attendee_session(&attendee.authorization, now)
            .await
            .is_err()
    );
    assert!(
        target
            .revalidate_connector_session(&connector.authorization, now)
            .await
            .is_err()
    );
    assert!(
        target
            .authorize_owner_session(
                host_owner.authorization.fingerprint(),
                &binding.browser_fingerprint,
                &binding.origin
            )
            .await
            .is_err()
    );
    assert!(
        target
            .revalidate_room_session_authorization(&RoomSessionAuthorization::Operator(
                operator.authorization
            ))
            .await
            .is_err()
    );
    assert!(
        target
            .admit_connector(
                &connector_fp,
                &[12; 32],
                uuid::Uuid::new_v4(),
                "Connector",
                now
            )
            .await
            .is_err()
    );
    let make = || AttendeeAdmissionRequest {
        invite_fingerprint: &pending_fingerprint,
        client_fingerprint: &[30; 32],
        request_id: uuid::Uuid::new_v4(),
        provider_kind: "codex_live_session",
        display_name: "Pending AI",
    };
    assert!(target.admit_attendee(make(), now).await.is_err());
    assert!(target.member_connect_rooms(&member, now).await.is_err());
    let stranger = MemberAdmission {
        registration_epoch: "new-epoch".into(),
        person_id: "snapshot-missing-person".into(),
        ..member.clone()
    };
    assert!(
        target
            .member_connect_rooms(&stranger, now)
            .await?
            .is_empty()
    );
    let payload_bytes = std::fs::read(directory.path().join("payload.sqlite"))?;
    for retired in [
        old_fingerprint.as_slice(),
        host_owner.authorization.fingerprint().as_slice(),
        attendee.authorization.session_fingerprint().as_slice(),
        connector.authorization.session_fingerprint().as_slice(),
    ] {
        assert!(
            !payload_bytes
                .windows(retired.len())
                .any(|bytes| bytes == retired)
        );
    }

    let fresh = MemberAdmission {
        registration_epoch: "new-epoch".into(),
        projection_id: "new-projection".into(),
        challenge_fingerprint: [31; 32],
        ..member
    };
    let rooms = target.member_connect_rooms(&fresh, now).await?;
    assert_eq!(rooms.len(), 1);
    let joined = admitted(
        target
            .select_member_connect_room(
                &fresh,
                "general",
                &[32; 32],
                &uuid::Uuid::new_v4().to_string(),
                "fresh-browser",
                now,
            )
            .await?,
    );
    assert_eq!(joined.result().agent_id, old.result().agent_id);
    let new_fingerprint: [u8; 32] = Sha256::digest(joined.session_bearer().as_bytes()).into();
    assert_ne!(new_fingerprint, old_fingerprint);
    target.authorize_human_session(&new_fingerprint).await?;
    let pending_admitted = target.admit_attendee(make(), now).await?;
    assert_eq!(
        pending_admitted.authorization.principal().invite_scope,
        InviteScope::ReadWrite
    );
    assert!(
        target
            .admit_attendee(
                AttendeeAdmissionRequest {
                    invite_fingerprint: &used_fingerprint,
                    ..make()
                },
                now
            )
            .await
            .is_err()
    );
    let retained_connector = target
        .admit_connector(
            &unredeemed_fp,
            &[40; 32],
            uuid::Uuid::new_v4(),
            "Retained connector",
            now,
        )
        .await?;
    assert_eq!(
        retained_connector.authorization.principal().invite_scope,
        InviteScope::ReadOnly
    );
    let roles_after: Vec<(String, String)> =
        sqlx::query_as("SELECT participant_id,json_extract(participant_json,'$.role') FROM participants WHERE json_extract(participant_json,'$.participant_type')='human' ORDER BY participant_id")
            .fetch_all(&target.pool)
            .await?;
    assert_eq!(roles_before, roles_after);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT use_count FROM room_invites")
            .fetch_one(&target.pool)
            .await?,
        1
    );
    let row=sqlx::query("SELECT parent_fingerprint,restore_parent_kind,expires_at FROM room_attendee_invites WHERE invite_id=?").bind(pending.invite_id.to_string()).fetch_one(&target.pool).await?;
    assert_eq!(row.get::<Vec<u8>, _>("parent_fingerprint"), new_fingerprint);
    assert!(
        row.get::<Option<String>, _>("restore_parent_kind")
            .is_none()
    );
    assert_eq!(
        row.get::<i64, _>("expires_at"),
        pending.expires_at.timestamp_micros()
    );
    target.close().await?;
    source.close().await?;
    Ok(())
}
