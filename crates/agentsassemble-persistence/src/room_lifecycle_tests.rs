use agentsassemble_domain::{ClientKind, RoomStatus};
use serde_json::json;

use super::tests::{AGENT_ID, fixture};
use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn archive_retains_management_replay_and_requires_cleanup_before_restore()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let uid = store.snapshot("general", 0, 20).await?.room.room_uid;
    let archive = json!({"room_uid": uid, "archived": true});
    let archived = store
        .execute_room_lifecycle(&principal, "archive", "room.archive", &archive)
        .await?;
    assert_eq!(archived.outcome.result["room"]["status"], "archived");
    assert!(store.resolve_principal(&principal).await.is_err());
    assert!(
        store
            .resolve_room_lifecycle_principal(&principal)
            .await?
            .capabilities
            .room_manage
    );
    assert!(
        !store
            .command_requires_principal_budget(&principal, "archive", "room.archive", &archive)
            .await?
    );
    let restore = json!({"room_uid": uid, "archived": false});
    assert!(matches!(
        store
            .execute_room_lifecycle(&principal, "restore", "room.archive", &restore)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "runtime_cleanup_pending",
            ..
        })
    ));
    for key in &archived.cleanup {
        assert!(store.finish_room_runtime_cleanup(key).await?.is_some());
    }
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let replay = store
        .execute_room_lifecycle(&principal, "archive", "room.archive", &archive)
        .await?;
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.outcome.result, archived.outcome.result);
    assert!(replay.cleanup.is_empty());
    let restored = store
        .execute_room_lifecycle(&principal, "restore", "room.archive", &restore)
        .await?;
    assert_eq!(restored.outcome.result["room"]["status"], "active");
    assert!(store.resolve_principal(&principal).await.is_ok());
    assert!(
        store
            .prepare_agent_start(&principal, "after-restore", &json!({"agent_id": AGENT_ID}))
            .await
            .is_ok()
    );
    // A replay changes neither the restored room nor its newly prepared launch.
    assert!(
        store
            .execute_room_lifecycle(&principal, "archive", "room.archive", &archive)
            .await?
            .outcome
            .deduplicated
    );
    assert_eq!(
        store.snapshot("general", 0, 20).await?.room.status,
        RoomStatus::Active
    );
    assert!(
        store
            .load_room_runtime_cleanup_page(None)
            .await?
            .keys
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn closure_requires_exact_local_owner_and_incarnation_and_cannot_be_restored()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let uid = store.snapshot("general", 0, 20).await?.room.room_uid;
    let close = json!({"room_uid": uid});
    let mut guest = principal.clone();
    guest.is_operator = false;
    assert!(
        store
            .execute_room_lifecycle(&guest, "guest", "room.close", &close)
            .await
            .is_err()
    );
    let mut wrong_owner = principal.clone();
    wrong_owner.principal_id = "other-user".to_owned();
    assert!(
        store
            .execute_room_lifecycle(&wrong_owner, "other", "room.close", &close)
            .await
            .is_err()
    );
    let mut bridge = principal.clone();
    bridge.client_kind = ClientKind::AgentBridge;
    assert!(
        store
            .execute_room_lifecycle(&bridge, "bridge", "room.close", &close)
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .execute_room_lifecycle(
                &principal,
                "stale",
                "room.close",
                &json!({"room_uid": uuid::Uuid::new_v4()})
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "room_incarnation_changed",
            ..
        })
    ));
    let closed = store
        .execute_room_lifecycle(&principal, "close", "room.close", &close)
        .await?;
    assert_eq!(closed.outcome.result["room"]["status"], "closed");
    assert!(
        store
            .execute_room_lifecycle(&principal, "close", "room.close", &close)
            .await?
            .outcome
            .deduplicated
    );
    assert!(matches!(
        store
            .execute_room_lifecycle(
                &principal,
                "close",
                "room.archive",
                &json!({"room_uid": uid, "archived": false})
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(matches!(
        store
            .execute_room_lifecycle(
                &principal,
                "reopen",
                "room.archive",
                &json!({"room_uid": uid, "archived": false})
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "room_closed",
            ..
        })
    ));
    Ok(())
}
