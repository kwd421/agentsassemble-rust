use super::*;
use crate::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_domain::{CapabilitySet, InviteScope};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn fixture()
-> Result<(SqliteStore, AuthenticatedPrincipal, tempfile::TempDir), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("room.sqlite3")).await?;
    store
        .bootstrap_local_authority("11111111-1111-4111-8111-111111111111", "Host")
        .await?;
    store
        .create_room_for_local_operator(
            "22222222-2222-4222-8222-222222222222",
            "general",
            "General",
        )
        .await?;
    let principal = AuthenticatedPrincipal {
        principal_id: "operator-local-user".into(),
        participant_id: "operator-local".into(),
        display_name: "Untrusted display".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    Ok((store, principal, directory))
}

#[tokio::test]
async fn bootstrap_overlap_replay_and_mutable_human_authority() -> TestResult {
    let (store, principal, _directory) = fixture().await?;
    let now = Utc::now();
    let mut live = store
        .subscribe_side_chat(
            TrustedPrincipal(&principal),
            store.snapshot("general", 0, 1).await?.room.room_uid,
        )
        .await?;
    let initial = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    let payload = json!({"generation":initial.generation,"after_seq":0,"content":"private text"});
    let sent = store
        .execute_side_chat(TrustedPrincipal(&principal), "one", &payload, now)
        .await?;
    let bootstrap = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    assert_eq!(bootstrap.messages, vec![sent.update.message.clone()]);
    assert_eq!(live.try_recv()?, sent.update);
    assert_eq!(sent.update.message.display_name, "Host");
    let retry = store
        .execute_side_chat(TrustedPrincipal(&principal), "one", &payload, now)
        .await?;
    assert!(retry.deduplicated);
    assert_eq!(retry.update, sent.update);
    assert!(matches!(
        live.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    let mut conflict = payload.clone();
    conflict["content"] = json!("changed");
    assert!(matches!(
        store
            .execute_side_chat(TrustedPrincipal(&principal), "one", &conflict, now)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    let mut restricted = principal.clone();
    restricted.invite_scope = InviteScope::ReadOnly;
    restricted.capabilities =
        CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadOnly);
    assert!(
        store
            .side_chat_snapshot(TrustedPrincipal(&restricted), now)
            .await
            .is_ok()
    );
    assert!(
        store
            .execute_side_chat(TrustedPrincipal(&restricted), "one", &payload, now)
            .await
            .is_err()
    );
    restricted.client_kind = ClientKind::AgentBridge;
    assert!(
        store
            .subscribe_side_chat(TrustedPrincipal(&restricted), initial.room_uid)
            .await
            .is_err()
    );
    sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.muted', json('true')) WHERE room_id = 'general'").execute(&store.pool).await?;
    assert!(matches!(
        store
            .execute_side_chat(TrustedPrincipal(&principal), "one", &payload, now)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"muted")
    ));
    sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.participant_type', 'agent') WHERE room_id = 'general'").execute(&store.pool).await?;
    assert!(
        store
            .side_chat_snapshot(TrustedPrincipal(&principal), now)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn count_and_clock_retention_bound_exact_retry_custody() -> TestResult {
    let (store, principal, _directory) = fixture().await?;
    let now = Utc::now();
    let initial = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    for index in 0..201 {
        let payload = json!({"generation":initial.generation,"after_seq":index,"content":format!("private-{index}")});
        store
            .execute_side_chat(
                TrustedPrincipal(&principal),
                &index.to_string(),
                &payload,
                now,
            )
            .await?;
    }
    let retained = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    assert_eq!(retained.messages.len(), 200);
    assert_eq!(retained.retained_after_seq, 1);
    assert_eq!(retained.latest_seq, 201);
    let old = json!({"generation":initial.generation,"after_seq":0,"content":"private-0"});
    assert!(matches!(
        store
            .execute_side_chat(TrustedPrincipal(&principal), "0", &old, now)
            .await,
        Err(PersistenceError::CommandUnresolved { code, .. }) if matches!(code.as_bytes(), b"side_chat_retry_expired")
    ));
    let expired_at = now + chrono::Duration::seconds(SIDE_CHAT_TTL_SECONDS);
    let expired = store
        .side_chat_snapshot(TrustedPrincipal(&principal), expired_at)
        .await?;
    assert!(expired.messages.is_empty());
    assert_eq!(expired.retained_after_seq, 201);
    let fresh = json!({"generation":initial.generation,"after_seq":201,"content":"fresh intent"});
    assert_eq!(
        store
            .execute_side_chat(TrustedPrincipal(&principal), "fresh", &fresh, expired_at)
            .await?
            .update
            .message
            .seq,
        202
    );
    Ok(())
}

#[tokio::test]
async fn restart_discards_content_receipts_and_generation() -> TestResult {
    let (store, principal, directory) = fixture().await?;
    let now = Utc::now();
    let initial = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    let payload = json!({"generation":initial.generation,"after_seq":0,"content":"side-chat-private-sentinel"});
    store
        .execute_side_chat(
            TrustedPrincipal(&principal),
            "private-request",
            &payload,
            now,
        )
        .await?;
    assert!(
        !serde_json::to_string(&store.snapshot("general", 0, 100).await?.events)?
            .contains("side-chat-private-sentinel")
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM command_results WHERE request_id = 'private-request'"
        )
        .fetch_one(&store.pool)
        .await?,
        0
    );
    drop(store);
    let bytes = std::fs::read(directory.path().join("room.sqlite3"))?;
    assert!(
        !bytes
            .windows(b"side-chat-private-sentinel".len())
            .any(|window| window == b"side-chat-private-sentinel")
    );
    let reopened = SqliteStore::open_path(&directory.path().join("room.sqlite3")).await?;
    let empty = reopened
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    assert!(empty.messages.is_empty());
    assert_ne!(empty.generation, initial.generation);
    assert!(matches!(
        reopened
            .execute_side_chat(
                TrustedPrincipal(&principal),
                "private-request",
                &payload,
                now
            )
            .await,
        Err(PersistenceError::CommandUnresolved { code, .. }) if matches!(code.as_bytes(), b"side_chat_retry_expired")
    ));
    Ok(())
}

#[tokio::test]
async fn room_isolation_and_deletion_close_ephemeral_lifetime() -> TestResult {
    let (store, principal, _directory) = fixture().await?;
    let now = Utc::now();
    let mut live = store
        .subscribe_side_chat(
            TrustedPrincipal(&principal),
            store.snapshot("general", 0, 1).await?.room.room_uid,
        )
        .await?;
    let initial = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "other", "Other")
        .await?;
    let mut other = principal.clone();
    other.room_id = "other".into();
    let other_state = store
        .side_chat_snapshot(TrustedPrincipal(&other), now)
        .await?;
    assert_ne!(other_state.generation, initial.generation);
    let payload = json!({"generation":initial.generation,"after_seq":0,"content":"only general"});
    assert!(
        store
            .execute_side_chat(TrustedPrincipal(&other), "one", &payload, now)
            .await
            .is_err()
    );
    store
        .execute_side_chat(TrustedPrincipal(&principal), "one", &payload, now)
        .await?;
    live.try_recv()?;
    assert!(
        store
            .side_chat_snapshot(TrustedPrincipal(&other), now)
            .await?
            .messages
            .is_empty()
    );
    let room = store.snapshot("general", 0, 20).await?.room;
    store
        .execute_room_delete(
            TrustedPrincipal(&principal),
            "delete",
            &json!({"room_uid":room.room_uid,"confirmation_name":room.label}),
        )
        .await?;
    for event in store.pending_room_publications("general").await? {
        store
            .acknowledge_room_publication("general", event.seq)
            .await?;
    }
    assert!(store.finish_room_deletion("general").await?);
    assert!(matches!(
        live.try_recv(),
        Err(broadcast::error::TryRecvError::Closed)
    ));
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let recreated = store
        .side_chat_snapshot(TrustedPrincipal(&principal), now)
        .await?;
    assert!(recreated.messages.is_empty());
    assert_ne!(recreated.generation, initial.generation);
    assert!(
        store
            .subscribe_side_chat(TrustedPrincipal(&principal), room.room_uid)
            .await
            .is_err()
    );
    Ok(())
}
