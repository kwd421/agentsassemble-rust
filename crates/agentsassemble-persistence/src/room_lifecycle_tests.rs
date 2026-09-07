use crate::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_domain::{ClientKind, RoomStatus};
use serde_json::json;

use super::tests::{AGENT_ID, fixture};
use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn archive_after_confirmed_shutdown_and_reopen_can_finish_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let crate::AgentStartPlan::Start(start) = store
        .prepare_agent_start(
            TrustedPrincipal(&principal),
            "start-before-shutdown",
            &payload,
        )
        .await?
    else {
        panic!("stopped fixture must prepare a start");
    };
    store
        .authorize_agent_start_effect(
            TrustedPrincipal(&principal),
            "start-before-shutdown",
            &payload,
            &start.operation_id,
            "agent.start",
            "shutdown-handle",
            "shutdown-owner",
            "shutdown-lease",
        )
        .await?;
    store
        .complete_agent_start(
            &principal,
            "start-before-shutdown",
            &payload,
            &start.operation_id,
            &crate::AgentRuntimeStarted {
                runtime_handle_id: "shutdown-handle".to_owned(),
                runtime_owner_id: "shutdown-owner".to_owned(),
                runtime_lease_token: "shutdown-lease".to_owned(),
                provider_session_id: "shutdown-thread".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await?;
    let candidate = store
        .load_runtime_reconciliation_candidate("general", AGENT_ID)
        .await?
        .ok_or("running session has no custody")?;
    store
        .apply_runtime_shutdown_reconciliation(
            &candidate,
            &crate::RuntimeReconciliationObservation::Gone,
        )
        .await?;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let snapshot = store.snapshot("general", 0, 20).await?;
    assert_eq!(
        snapshot.agent_sessions[0].runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Stopped
    );
    assert!(!snapshot.agent_sessions[0].recovery_required);
    let uid = snapshot.room.room_uid;
    let archived = store
        .execute_room_lifecycle(
            TrustedPrincipal(&principal),
            "archive-after-shutdown",
            "room.archive",
            &json!({"room_uid": uid, "archived": true}),
        )
        .await?;
    for key in &archived.cleanup {
        assert!(store.finish_room_runtime_cleanup(key).await?.is_some());
    }
    let restored = store
        .execute_room_lifecycle(
            TrustedPrincipal(&principal),
            "restore-after-shutdown",
            "room.archive",
            &json!({"room_uid": uid, "archived": false}),
        )
        .await?;
    assert_eq!(restored.outcome.result["room"]["status"], "active");
    Ok(())
}

#[tokio::test]
async fn archive_retains_management_replay_and_requires_cleanup_before_restore()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let uid = store.snapshot("general", 0, 20).await?.room.room_uid;
    let archive = json!({"room_uid": uid, "archived": true});
    let archived = store
        .execute_room_lifecycle(
            TrustedPrincipal(&principal),
            "archive",
            "room.archive",
            &archive,
        )
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
            .execute_room_lifecycle(
                TrustedPrincipal(&principal),
                "restore",
                "room.archive",
                &restore
            )
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
        .execute_room_lifecycle(
            TrustedPrincipal(&principal),
            "archive",
            "room.archive",
            &archive,
        )
        .await?;
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.outcome.result, archived.outcome.result);
    assert!(replay.cleanup.is_empty());
    let restored = store
        .execute_room_lifecycle(
            TrustedPrincipal(&principal),
            "restore",
            "room.archive",
            &restore,
        )
        .await?;
    assert_eq!(restored.outcome.result["room"]["status"], "active");
    assert!(store.resolve_principal(&principal).await.is_ok());
    assert!(
        store
            .prepare_agent_start(
                TrustedPrincipal(&principal),
                "after-restore",
                &json!({"agent_id": AGENT_ID})
            )
            .await
            .is_ok()
    );
    // A replay changes neither the restored room nor its newly prepared launch.
    assert!(
        store
            .execute_room_lifecycle(
                TrustedPrincipal(&principal),
                "archive",
                "room.archive",
                &archive
            )
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
            .execute_room_lifecycle(TrustedPrincipal(&guest), "guest", "room.close", &close)
            .await
            .is_err()
    );
    let mut wrong_owner = principal.clone();
    wrong_owner.principal_id = "other-user".to_owned();
    assert!(
        store
            .execute_room_lifecycle(
                TrustedPrincipal(&wrong_owner),
                "other",
                "room.close",
                &close
            )
            .await
            .is_err()
    );
    let mut bridge = principal.clone();
    bridge.client_kind = ClientKind::AgentBridge;
    assert!(
        store
            .execute_room_lifecycle(TrustedPrincipal(&bridge), "bridge", "room.close", &close)
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .execute_room_lifecycle(
                TrustedPrincipal(&principal),
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
        .execute_room_lifecycle(TrustedPrincipal(&principal), "close", "room.close", &close)
        .await?;
    assert_eq!(closed.outcome.result["room"]["status"], "closed");
    assert!(
        store
            .execute_room_lifecycle(TrustedPrincipal(&principal), "close", "room.close", &close)
            .await?
            .outcome
            .deduplicated
    );
    assert!(matches!(
        store
            .execute_room_lifecycle(
                TrustedPrincipal(&principal),
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
                TrustedPrincipal(&principal),
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
