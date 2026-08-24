use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant, ParticipantStatus,
    Room, RoomStatus,
};
use serde_json::json;

use crate::{PersistenceError, SqliteStore};

async fn initialized_store() -> (SqliteStore, AuthenticatedPrincipal, Room, Participant) {
    let url = format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    );
    let store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open fixture: {error}"));
    store
        .bootstrap_local_authority("4528692e-9e3d-4c0a-b7bb-a5197641fe80", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap fixture: {error}"));
    let room = store
        .create_room_for_local_operator("general", "General")
        .await
        .unwrap_or_else(|error| panic!("create fixture room: {error}"))
        .room;
    let participant = store
        .active_participant(&room.room_id, "operator-local")
        .await
        .unwrap_or_else(|error| panic!("load fixture participant: {error}"));
    let principal = AuthenticatedPrincipal {
        principal_id: "operator-local-user".to_owned(),
        participant_id: participant.participant_id.clone(),
        display_name: participant.display_name.clone(),
        room_id: room.room_id.clone(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    (store, principal, room, participant)
}

#[tokio::test]
async fn filename_text_cannot_disable_the_file_writer_lease() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let path = directory.path().join("mode=memory.sqlite3");
    let url = format!("sqlite://{}", path.display());
    let first = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open deceptive filename: {error}"));
    assert!(path.is_file());
    assert!(matches!(
        SqliteStore::open(&url).await,
        Err(PersistenceError::WriterAlreadyActive(_))
    ));
    drop(first);
}

#[tokio::test]
async fn path_api_treats_query_characters_as_literal_filename_text() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let path = directory.path().join("authority?mode=memory");
    let store = SqliteStore::open_path(&path)
        .await
        .unwrap_or_else(|error| panic!("open literal query-shaped filename: {error}"));
    assert!(path.is_file());
    assert!(store.was_created());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_and_hardlink_database_aliases_are_rejected() {
    use std::os::unix::fs::symlink;

    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let original = directory.path().join("authority.sqlite3");
    let url = format!("sqlite://{}", original.display());
    let store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("create authority: {error}"));
    drop(store);

    let symbolic = directory.path().join("symbolic.sqlite3");
    symlink(&original, &symbolic).unwrap_or_else(|error| panic!("create symbolic alias: {error}"));
    assert!(matches!(
        SqliteStore::open(&format!("sqlite://{}", symbolic.display())).await,
        Err(PersistenceError::UnsafeDatabasePath(_))
    ));

    let hard = directory.path().join("hard.sqlite3");
    std::fs::hard_link(&original, &hard)
        .unwrap_or_else(|error| panic!("create hard-link alias: {error}"));
    assert!(matches!(
        SqliteStore::open(&format!("sqlite://{}", hard.display())).await,
        Err(PersistenceError::UnsafeDatabasePath(_))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn unlinked_hardlink_alias_cannot_become_a_second_writer() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let original = directory.path().join("authority.sqlite3");
    let alias = directory.path().join("alias.sqlite3");
    let seed = SqliteStore::open_path(&original)
        .await
        .unwrap_or_else(|error| panic!("create authority: {error}"));
    drop(seed);
    let first = SqliteStore::open_path(&original)
        .await
        .unwrap_or_else(|error| panic!("reopen existing authority: {error}"));
    std::fs::hard_link(&original, &alias)
        .unwrap_or_else(|error| panic!("create authority alias: {error}"));
    std::fs::remove_file(&original)
        .unwrap_or_else(|error| panic!("unlink original authority name: {error}"));

    assert!(SqliteStore::open_path(&alias).await.is_err());
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'schema_owner'",
    )
    .fetch_one(&first.pool)
    .await
    .unwrap_or_else(|error| panic!("original writer remains usable: {error}"));
    assert_eq!(owner, "agentsassemble-rust-v1");
}

#[cfg(unix)]
#[tokio::test]
async fn new_database_and_lease_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let path = directory.path().join("private.sqlite3");
    let url = format!("sqlite://{}", path.display());
    let _store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open private authority: {error}"));
    let database_mode = path
        .metadata()
        .unwrap_or_else(|error| panic!("database metadata: {error}"))
        .permissions()
        .mode();
    let lease_mode = path
        .with_extension("sqlite3.writer.lock")
        .metadata()
        .unwrap_or_else(|error| panic!("lease metadata: {error}"))
        .permissions()
        .mode();
    assert_eq!(database_mode & 0o077, 0);
    assert_eq!(lease_mode & 0o077, 0);
}

#[tokio::test]
async fn existing_authority_cannot_recreate_first_run_membership() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let path = directory.path().join("owned.sqlite3");
    let url = format!("sqlite://{}", path.display());
    let first = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open new authority: {error}"));
    let request_id = "675c30b1-4341-4ff0-b03b-772b549ee547";
    first
        .bootstrap_local_authority(request_id, "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap authority: {error}"));
    first
        .create_room_for_local_operator("general", "General")
        .await
        .unwrap_or_else(|error| panic!("create authority room: {error}"));
    drop(first);
    let reopened = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("reopen authority: {error}"));
    assert!(!reopened.was_created());
    assert!(
        reopened
            .bootstrap_local_authority(request_id, "Host")
            .await
            .unwrap_or_else(|error| panic!("replay bootstrap: {error}"))
            .deduplicated
    );
    assert!(
        reopened
            .bootstrap_local_authority("26d021bc-686a-4269-a4ee-8851bcf49a7c", "Host")
            .await
            .is_err()
    );
    assert_eq!(
        reopened
            .list_room_directory(true)
            .await
            .unwrap_or_else(|error| panic!("reopened room directory: {error}"))
            .len(),
        1
    );
}

#[tokio::test]
async fn inactive_room_rejects_commands_inside_the_write_transaction() {
    let (store, principal, mut room, _) = initialized_store().await;
    room.status = RoomStatus::Closed;
    sqlx::query("UPDATE rooms SET room_json = ? WHERE room_id = ?")
        .bind(serde_json::to_string(&room).unwrap_or_else(|error| panic!("encode room: {error}")))
        .bind(&room.room_id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("close room: {error}"));
    assert!(matches!(
        store
            .execute_message(
                &principal,
                "closed-room-command",
                "message.send",
                &json!({"content": "must fail"}),
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "room_inactive",
            ..
        })
    ));
}

#[tokio::test]
async fn revoked_participant_cannot_receive_an_authorized_snapshot() {
    let (store, principal, _, mut participant) = initialized_store().await;
    participant.status = ParticipantStatus::Kicked;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode participant: {error}")),
    )
    .bind(&participant.room_id)
    .bind(&participant.participant_id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("revoke participant: {error}"));
    assert!(matches!(
        store.snapshot_for(&principal, 0, 200).await,
        Err(PersistenceError::CommandRejected {
            code: "session_revoked",
            ..
        })
    ));
}
