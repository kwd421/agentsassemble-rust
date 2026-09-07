use crate::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_domain::{ClientKind, RoomStatus};
use serde_json::json;

use super::tests::{AGENT_ID, fixture};
use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn deletion_waits_for_custody_and_publication_then_replays_after_recreation()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let room = store.snapshot("general", 0, 20).await?.room;
    let payload = json!({"room_uid": room.room_uid, "confirmation_name": room.label});
    let prepared = store
        .execute_room_delete(TrustedPrincipal(&principal), "delete", &payload)
        .await?;
    assert!(!prepared.complete);
    assert!(!prepared.outcome.deduplicated);
    assert_eq!(
        store.snapshot("general", 0, 20).await?.room.status,
        RoomStatus::Closed
    );
    assert!(store.resolve_principal(&principal).await.is_err());
    assert!(
        !store
            .command_requires_principal_budget(&principal, "delete", "room.delete", &payload)
            .await?
    );
    assert!(matches!(
        store
            .execute_room_lifecycle(
                TrustedPrincipal(&principal),
                "delete",
                "room.close",
                &json!({"room_uid": room.room_uid})
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(!store.finish_room_deletion("general").await?);
    for key in store.load_room_runtime_cleanup_page(None).await?.keys {
        assert!(store.finish_room_runtime_cleanup(&key).await?.is_some());
    }
    // No runtime remains, but deletion still cannot discard unpublished closure.
    assert!(!store.finish_room_deletion("general").await?);
    for event in store.pending_room_publications("general").await? {
        store
            .acknowledge_room_publication("general", event.seq)
            .await?;
    }
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    assert_eq!(
        store.pending_room_deletions(None).await?.room_ids,
        vec!["general"]
    );
    assert!(store.finish_room_deletion("general").await?);
    assert!(
        store
            .pending_room_deletions(None)
            .await?
            .room_ids
            .is_empty()
    );
    assert!(matches!(
        store.snapshot("general", 0, 20).await,
        Err(PersistenceError::RoomMissing)
    ));
    assert!(!sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE room_id = 'general' AND session_id = ?)")
        .bind(AGENT_ID).fetch_one(&store.pool).await?);
    assert_deletion_replay(&store, &principal, &room, &prepared, &payload).await
}

async fn assert_deletion_replay(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    room: &agentsassemble_domain::Room,
    prepared: &crate::RoomDeletionMutation,
    payload: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let replay = store
        .execute_room_delete(TrustedPrincipal(principal), "delete", payload)
        .await?;
    assert!(
        store
            .resolve_room_terminal_principal(
                principal,
                room.room_uid,
                &prepared.outcome.event.id,
                prepared.outcome.event.seq
            )
            .await
            .is_ok()
    );
    assert!(
        store
            .resolve_room_terminal_principal(
                principal,
                room.room_uid,
                &prepared.outcome.event.id,
                prepared.outcome.event.seq + 1
            )
            .await
            .is_err()
    );
    assert!(replay.complete && replay.outcome.deduplicated);
    assert_eq!(replay.outcome.result, prepared.outcome.result);
    assert!(
        store
            .resolve_room_delete_principal(principal, "delete", payload)
            .await
            .is_ok()
    );
    let new = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", &room.label)
        .await?;
    assert_ne!(new.room.room_uid, room.room_uid);
    assert!(
        store
            .execute_room_delete(TrustedPrincipal(principal), "delete", payload)
            .await?
            .complete
    );
    assert_eq!(
        store.snapshot("general", 0, 20).await?.room.room_uid,
        new.room.room_uid
    );
    assert!(matches!(
        store
            .execute_room_delete(
                TrustedPrincipal(principal),
                "delete",
                &json!({"room_uid": new.room.room_uid, "confirmation_name": room.label})
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(!store.finish_room_deletion("general").await?);
    Ok(())
}

#[tokio::test]
async fn deletion_rejects_wrong_name_authority_incarnation_and_second_intent()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let room = store.snapshot("general", 0, 20).await?.room;
    let payload = json!({"room_uid": room.room_uid, "confirmation_name": room.label});
    let mut bridge = principal.clone();
    bridge.client_kind = ClientKind::AgentBridge;
    assert!(
        store
            .execute_room_delete(TrustedPrincipal(&bridge), "denied", &payload)
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .execute_room_delete(
                TrustedPrincipal(&principal),
                "wrong",
                &json!({"room_uid": room.room_uid, "confirmation_name": "Wrong"})
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "confirmation_mismatch",
            ..
        })
    ));
    assert!(matches!(
        store
            .execute_room_delete(
                TrustedPrincipal(&principal),
                "stale",
                &json!({"room_uid": uuid::Uuid::new_v4(), "confirmation_name": room.label})
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "room_incarnation_changed",
            ..
        })
    ));
    assert_eq!(
        store.snapshot("general", 0, 20).await?.room.status,
        RoomStatus::Active
    );
    assert!(
        store
            .pending_room_deletions(None)
            .await?
            .room_ids
            .is_empty()
    );
    store
        .execute_room_delete(TrustedPrincipal(&principal), "delete", &payload)
        .await?;
    assert!(matches!(
        store
            .execute_room_delete(TrustedPrincipal(&principal), "other", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "room_deletion_pending",
            ..
        })
    ));
    assert!(
        store
            .resolve_room_delete_principal(&bridge, "delete", &payload)
            .await
            .is_err()
    );
    Ok(())
}
