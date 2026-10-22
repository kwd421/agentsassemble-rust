use crate::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_domain::{ClientKind, ParticipantStatus};
use serde_json::json;

use super::tests::{AGENT_ID, fixture};
use crate::{AgentStartPlan, PersistenceError, RuntimeReconciliationObservation, SqliteStore};

#[tokio::test]
async fn removal_fences_launch_and_reopen_retains_cleanup_until_absence_is_proven() {
    let (store, principal, directory) = fixture().await;
    let launch = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(TrustedPrincipal(&principal), "launch", &launch)
        .await
        .unwrap_or_else(|error| panic!("prepare launch: {error}"))
    else {
        panic!("fixture must prepare launch");
    };
    store
        .authorize_agent_start_effect(
            TrustedPrincipal(&principal),
            "launch",
            &launch,
            &effect.operation_id,
            "agent.start",
            "cleanup-handle",
            "cleanup-owner",
            "cleanup-lease",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize exact launch: {error}"));
    let payload = json!({"participant_id": AGENT_ID});
    let authority = TrustedPrincipal(&principal);
    let removal = store
        .execute_participant_removal(authority, "remove", "participant.kick", &payload)
        .await
        .unwrap_or_else(|error| panic!("remove participant: {error}"));
    assert_eq!(removal.outcome.result["participant"]["status"], "kicked");
    let key = removal
        .cleanups
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("runtime cleanup must remain durable"));
    assert!(
        store
            .finish_room_runtime_cleanup(&key)
            .await
            .unwrap_or_else(|error| panic!("inspect incomplete cleanup: {error}"))
            .is_none()
    );
    assert!(matches!(
        store
            .prepare_agent_start(TrustedPrincipal(&principal), "restart", &launch)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"runtime_cleanup_pending")
    ));
    let replay = store
        .execute_participant_removal(authority, "remove", "participant.kick", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay removal: {error}"));
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.outcome.result, removal.outcome.result);
    assert!(matches!(
        store
            .execute_participant_removal(authority, "remove", "participant.export", &payload)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    drop(store);
    verify_reopened_cleanup(directory.path(), &key).await;
}

async fn verify_reopened_cleanup(directory: &std::path::Path, key: &crate::RoomRuntimeCleanupKey) {
    let reopened = SqliteStore::open_path(&directory.join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("reopen removal: {error}"));
    let page = reopened
        .load_room_runtime_cleanup_page(None)
        .await
        .unwrap_or_else(|error| panic!("scan pending cleanup: {error}"));
    assert_eq!(page.keys, vec![key.clone()]);
    let candidate = reopened
        .load_runtime_reconciliation_candidate(&key.room_id, &key.session_id)
        .await
        .unwrap_or_else(|error| panic!("load exact custody: {error}"))
        .unwrap_or_else(|| panic!("unconfirmed runtime custody missing"));
    reopened
        .apply_runtime_shutdown_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await
        .unwrap_or_else(|error| panic!("checkpoint proven runtime absence: {error}"));
    assert!(
        reopened
            .finish_room_runtime_cleanup(key)
            .await
            .unwrap_or_else(|error| panic!("finish proven cleanup: {error}"))
            .is_some()
    );
    assert_eq!(
        reopened
            .participant("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("read removed membership: {error}"))
            .status,
        ParticipantStatus::Kicked
    );
    assert!(
        reopened
            .load_room_runtime_cleanup_page(None)
            .await
            .unwrap_or_else(|error| panic!("scan completed cleanup: {error}"))
            .keys
            .is_empty()
    );
}

#[tokio::test]
async fn removal_rejects_owner_bridge_and_payload_retargeting_without_side_effects() {
    let (store, principal, _directory) = fixture().await;
    for payload in [
        json!({"participant_id": "operator-local"}),
        json!({"participant_id": AGENT_ID, "room_id": "elsewhere"}),
        json!({"participant_id": format!(" {AGENT_ID}")}),
    ] {
        assert!(
            store
                .execute_participant_removal(
                    TrustedPrincipal(&principal),
                    "rejected",
                    "participant.kick",
                    &payload
                )
                .await
                .is_err()
        );
    }
    let mut bridge = principal.clone();
    bridge.client_kind = ClientKind::AgentBridge;
    assert!(matches!(
        store
            .execute_participant_removal(
                TrustedPrincipal(&bridge),
                "bridge",
                "participant.kick",
                &json!({"participant_id": AGENT_ID})
            )
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"permission_denied")
    ));
    assert!(
        store
            .load_room_runtime_cleanup_page(None)
            .await
            .unwrap_or_else(|error| panic!("scan rejected work: {error}"))
            .keys
            .is_empty()
    );
    assert_eq!(
        store
            .participant("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("read retained membership: {error}"))
            .status,
        ParticipantStatus::Detached
    );
}

#[tokio::test]
async fn exported_session_cannot_resume_after_successful_cleanup() {
    let (store, principal, _directory) = fixture().await;
    let removed = store
        .execute_participant_removal(
            TrustedPrincipal(&principal),
            "export",
            "participant.export",
            &json!({"participant_id": AGENT_ID}),
        )
        .await
        .unwrap_or_else(|error| panic!("export stopped session: {error}"));
    let snapshot = store
        .snapshot_for(TrustedPrincipal(&principal), 0, 200)
        .await
        .unwrap_or_else(|error| panic!("read managed export snapshot: {error}"));
    assert!(
        snapshot
            .participants
            .iter()
            .any(|participant| participant.participant_id == AGENT_ID
                && participant.status == ParticipantStatus::Exported)
    );
    let payload = json!({"participant_id": AGENT_ID});
    for (request_id, action) in [
        ("late-kick", "participant.kick"),
        ("new-export", "participant.export"),
    ] {
        assert!(matches!(
            store
                .execute_participant_removal(
                    TrustedPrincipal(&principal),
                    request_id,
                    action,
                    &payload
                )
                .await,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"participant_exported")
        ));
    }
    assert_eq!(
        store
            .participant("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("read terminal membership: {error}"))
            .status,
        ParticipantStatus::Exported
    );
    let replay = store
        .execute_participant_removal(
            TrustedPrincipal(&principal),
            "export",
            "participant.export",
            &payload,
        )
        .await
        .unwrap_or_else(|error| panic!("replay terminal export: {error}"));
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.outcome.result, removed.outcome.result);
    let key = removed
        .cleanups
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("export cleanup missing"));
    assert!(
        store
            .finish_room_runtime_cleanup(&key)
            .await
            .unwrap_or_else(|error| panic!("finish stopped session: {error}"))
            .is_some()
    );
    assert!(matches!(
        store
            .prepare_agent_start(
                TrustedPrincipal(&principal),
                "start-exported",
                &json!({"agent_id": AGENT_ID})
            )
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"participant_exported")
    ));
}

#[tokio::test]
async fn closed_room_cleanup_requires_positive_absence_for_a_disconnected_session() {
    let (store, principal, _directory) = fixture().await;
    sqlx::query("UPDATE agent_sessions SET session_json = json_set(session_json, '$.runtime_status', 'disconnected', '$.recovery_required', json('true')) WHERE room_id = 'general' AND session_id = ?")
        .bind(AGENT_ID).execute(&store.pool).await
        .unwrap_or_else(|error| panic!("seed uncertain disconnect: {error}"));
    let removed = store
        .execute_participant_removal(
            TrustedPrincipal(&principal),
            "remove-disconnected",
            "participant.kick",
            &json!({"participant_id": AGENT_ID}),
        )
        .await
        .unwrap_or_else(|error| panic!("remove disconnected session: {error}"));
    let key = removed
        .cleanups
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("cleanup missing"));
    sqlx::query("UPDATE rooms SET room_json = json_set(room_json, '$.status', 'closed') WHERE room_id = 'general'")
        .execute(&store.pool).await.unwrap_or_else(|error| panic!("close pending room: {error}"));
    assert!(
        store
            .finish_room_runtime_cleanup(&key)
            .await
            .unwrap_or_else(|error| panic!("inspect uncertain absence: {error}"))
            .is_none()
    );
    let candidate = store
        .load_room_runtime_cleanup_candidate(&key)
        .await
        .unwrap_or_else(|error| panic!("load closed cleanup custody: {error}"))
        .unwrap_or_else(|| panic!("closed room hid pending cleanup"));
    store
        .apply_runtime_shutdown_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await
        .unwrap_or_else(|error| panic!("checkpoint positive absence: {error}"));
    let finished = store
        .finish_room_runtime_cleanup(&key)
        .await
        .unwrap_or_else(|error| panic!("finish closed-room cleanup: {error}"))
        .unwrap_or_else(|| panic!("proven cleanup still pending"));
    assert!(finished.next_assignments.is_empty());
}

async fn add_guest_owning_the_agent(
    store: &SqliteStore,
) -> agentsassemble_domain::AuthenticatedPrincipal {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, InviteScope, Participant, ParticipantRole,
    };
    let now = chrono::Utc::now();
    let guest = Participant {
        room_id: "general".to_owned(),
        participant_id: "human-guest".to_owned(),
        display_name: "Guest".to_owned(),
        avatar_image_url: String::new(),
        avatar_label: None,
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: ParticipantRole::Human,
        owner_id: "human-guest-user".to_owned(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    sqlx::query("INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', 'human-guest', ?)")
        .bind(serde_json::to_string(&guest).unwrap_or_else(|error| panic!("encode guest: {error}")))
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert guest: {error}"));
    sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.owner_id', 'human-guest') WHERE participant_id = ?")
        .bind(AGENT_ID)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("hand the agent to the guest: {error}"));
    AuthenticatedPrincipal {
        principal_id: guest.owner_id,
        participant_id: guest.participant_id,
        display_name: guest.display_name,
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: false,
        capabilities: CapabilitySet::for_principal(
            ClientKind::Browser,
            InviteScope::ReadWrite,
            false,
        ),
    }
}

async fn agent_status(store: &SqliteStore) -> String {
    sqlx::query_scalar::<_, String>("SELECT json_extract(participant_json, '$.status') FROM participants WHERE participant_id = ?")
        .bind(AGENT_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("agent status: {error}"))
}

/// A person's AIs leave with them, whether they are kicked or leave themselves.
#[tokio::test]
async fn a_persons_ai_leaves_with_them() {
    let (store, principal, _directory) = fixture().await;
    add_guest_owning_the_agent(&store).await;
    let kicked = store
        .execute_participant_removal(
            TrustedPrincipal(&principal),
            "kick-guest",
            "participant.kick",
            &json!({"participant_id": "human-guest"}),
        )
        .await
        .unwrap_or_else(|error| panic!("kick guest: {error}"));
    assert_eq!(agent_status(&store).await, "kicked");
    assert_eq!(kicked.cleanups.len(), 1);
    assert_eq!(kicked.cleanups[0].session_id, AGENT_ID);
    let types: Vec<_> = kicked
        .outcome
        .events
        .iter()
        .map(|event| event.participant_id.clone())
        .collect();
    assert_eq!(
        types.last().cloned().flatten().as_deref(),
        Some("human-guest"),
        "the person's own removal stays last"
    );

    let (store, _principal, _directory) = fixture().await;
    let guest = add_guest_owning_the_agent(&store).await;
    let left = store
        .execute_participant_leave(&guest, "leave-guest", &json!({}))
        .await
        .unwrap_or_else(|error| panic!("guest leaves: {error}"));
    assert_eq!(agent_status(&store).await, "kicked");
    assert_eq!(left.cleanups.len(), 1);
}
