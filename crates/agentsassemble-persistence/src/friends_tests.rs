use agentsassemble_domain::{FriendDetails, FriendParticipantType, SaveFriend};
use uuid::Uuid;

use crate::{PersistenceError, ServerOwnerAuthority, SqliteStore};

const LOCAL: ServerOwnerAuthority = ServerOwnerAuthority::LocalOperator;

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("friend fixture: {error:?}"))
}

fn draft() -> SaveFriend {
    SaveFriend {
        friend_id: Uuid::new_v4(),
        expected_revision: 0,
        details: FriendDetails {
            display_name: "친구".to_owned(),
            handle: String::new(),
            participant_type: FriendParticipantType::Human,
            provider_kind: "codex".to_owned(),
            connection_kind: "external".to_owned(),
            agent_id: String::new(),
            source_agent_id: "supplied-source".to_owned(),
            last_meeting_id: String::new(),
            status: "offline".to_owned(),
            source: "manual".to_owned(),
            last_seen_at: None,
        },
    }
}

#[tokio::test]
async fn friend_retries_conflicts_restart_and_deletion_preserve_directory_contract() {
    let directory = checked(tempfile::tempdir());
    let path = directory.path().join("friends.sqlite3");
    let store = checked(SqliteStore::open_path(&path).await);
    let create = draft();
    assert!(store.saved_friends(&LOCAL).await.is_err());
    assert!(store.save_friend(&LOCAL, &create).await.is_err());
    checked(
        store
            .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
            .await,
    );
    let first = checked(store.save_friend(&LOCAL, &create).await);
    assert_eq!(checked(store.save_friend(&LOCAL, &create).await), first);
    assert_eq!(first.details.participant_type, FriendParticipantType::Human);
    let another = checked(store.save_friend(&LOCAL, &draft()).await);
    assert_ne!(first.friend_id, another.friend_id);
    let mut edit = create.clone();
    edit.expected_revision = first.revision;
    edit.details.display_name = "수정".to_owned();
    let mut competing = edit.clone();
    competing.details.display_name = "다른 수정".to_owned();
    let (left, right) = tokio::join!(
        store.save_friend(&LOCAL, &edit),
        store.save_friend(&LOCAL, &competing)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let (winner, retry, loser) = match (left, right) {
        (Ok(winner), Err(error)) => (winner, edit, error),
        (Err(error), Ok(winner)) => (winner, competing, error),
        result => panic!("one edit must commit: {result:?}"),
    };
    assert!(matches!(
        loser,
        PersistenceError::CommandRejected { code, .. } if matches!(code.as_bytes(), b"friend_conflict")
    ));
    assert_eq!(checked(store.save_friend(&LOCAL, &retry).await), winner);
    drop(store);
    let store = checked(SqliteStore::open_path(&path).await);
    assert!(checked(store.saved_friends(&LOCAL).await).contains(&winner));
    assert!(checked(store.delete_friend(&LOCAL, first.friend_id).await));
    assert!(!checked(store.delete_friend(&LOCAL, first.friend_id).await));
    assert!(store.save_friend(&LOCAL, &create).await.is_err());
    assert!(store.save_friend(&LOCAL, &retry).await.is_err());
    assert_eq!(checked(store.saved_friends(&LOCAL).await), vec![another]);
    let raw: Option<String> = checked(
        sqlx::query_scalar("SELECT friend_json FROM saved_friends WHERE friend_id = ?")
            .bind(first.friend_id.to_string())
            .fetch_one(&store.pool)
            .await,
    );
    assert!(raw.is_none());
    checked(
        sqlx::query(
            "UPDATE saved_friends SET friend_json = 'broken' WHERE friend_json IS NOT NULL",
        )
        .execute(&store.pool)
        .await,
    );
    assert!(store.saved_friends(&LOCAL).await.is_err());
}
