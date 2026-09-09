use crate::RoomMutationAuthority::TrustedPrincipal;
use crate::participant_rows::save_participant_exact as save_participant;
use crate::{
    PersistenceError, SqliteStore,
    agent_lifecycle::{
        load_participant, load_session,
        tests::{AGENT_ID, fixture},
    },
};
use agentsassemble_domain::{ClientKind, ParticipantStatus};
use serde_json::json;

#[tokio::test]
async fn external_profile_ack_reuses_state_projection_and_exact_replay()
-> Result<(), Box<dyn std::error::Error>> {
    for capability in [None, Some(false), Some(true)] {
        let (store, attendee, now) = crate::attendee_connection_tests::fixture().await?;
        let agent_id = &attendee.principal().participant_id;
        if let Some(capability) = capability {
            let connection = store
                .claim_attendee_connection(&attendee, uuid::Uuid::new_v4(), now)
                .await?
                .authorization;
            let mut ready = crate::attendee_ready_tests::report();
            ready.retained_interrupt = capability;
            store
                .record_attendee_ready(&connection, &ready, now)
                .await?;
        }
        let operator = crate::human_session_authority_tests::local_operator_principal();
        let payload = json!({"agent_id": agent_id, "display_name": "External renamed"});
        let outcome = store
            .execute_agent_profile_update(TrustedPrincipal(&operator), "rename", &payload)
            .await?;
        assert_eq!(
            outcome.result["agent_session"],
            outcome.event.extra["agent_session"]
        );
        assert_eq!(
            outcome.result["agent_session"]["external_retained_interrupt"],
            capability == Some(true)
        );
        assert_eq!(
            outcome.result["participant"]["display_name"],
            "External renamed"
        );
        let stored: String = sqlx::query_scalar(
            "SELECT session_json FROM agent_sessions WHERE room_id='general' AND session_id=?",
        )
        .bind(agent_id)
        .fetch_one(&store.pool)
        .await?;
        assert!(
            serde_json::from_str::<serde_json::Value>(&stored)?
                .get("external_retained_interrupt")
                .is_none()
        );
        // A later connection report must not rewrite the earlier committed receipt.
        if capability.is_none() {
            let connection = store
                .claim_attendee_connection(&attendee, uuid::Uuid::new_v4(), now)
                .await?
                .authorization;
            store
                .record_attendee_ready(&connection, &crate::attendee_ready_tests::report(), now)
                .await?;
            assert_eq!(
                store.snapshot("general", 0, 200).await?.agent_sessions[0]
                    .external_retained_interrupt,
                Some(true)
            );
        }
        let replay = store
            .execute_agent_profile_update(TrustedPrincipal(&operator), "rename", &payload)
            .await?;
        assert!(replay.deduplicated);
        assert_eq!(replay.result, outcome.result);
    }
    Ok(())
}

#[tokio::test]
async fn identity_update_preserves_custody_and_membership_and_replays_after_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let mut transaction = store.pool.begin().await?;
    let before_session = load_session(&mut transaction, "general", AGENT_ID).await?;
    let mut before_participant = load_participant(&mut transaction, "general", AGENT_ID).await?;
    before_participant.muted = true;
    before_participant.status = ParticipantStatus::Kicked;
    save_participant(
        &mut transaction,
        &before_participant.room_id,
        &before_participant.participant_id,
        &before_participant,
    )
    .await?;
    transaction.commit().await?;
    let payload = json!({"agent_id": AGENT_ID, "display_name": "  새 이름  "});
    let outcome = store
        .execute_agent_profile_update(TrustedPrincipal(&principal), "rename", &payload)
        .await?;
    assert_eq!(outcome.result["agent_session"]["display_name"], "새 이름");
    assert_eq!(outcome.events.len(), 2);
    assert_eq!(outcome.result["event_seq"], outcome.event.seq);
    assert_eq!(outcome.events[0].event_type, "participant_updated");
    assert_eq!(outcome.events[1].event_type, "agent_session_state");
    assert!(matches!(
        store
            .execute_agent_profile_update(
                TrustedPrincipal(&principal),
                "rename",
                &json!({"agent_id": AGENT_ID, "display_name": "Conflict"})
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    store.pool.close().await;
    drop(store);
    let reopened = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let replay = reopened
        .execute_agent_profile_update(TrustedPrincipal(&principal), "rename", &payload)
        .await?;
    assert!(replay.deduplicated);
    assert_eq!(replay.result, outcome.result);
    let mut transaction = reopened.pool.begin().await?;
    let mut after_session = load_session(&mut transaction, "general", AGENT_ID).await?;
    let mut after_participant = load_participant(&mut transaction, "general", AGENT_ID).await?;
    assert_eq!(after_participant.display_name, "새 이름");
    assert_eq!(
        after_participant.updated_at,
        after_session.public.updated_at
    );
    assert_eq!(after_participant.updated_at, outcome.events[0].created_at);
    assert_eq!(
        outcome.result["participant"]["updated_at"],
        json!(after_participant.updated_at)
    );
    after_session.public.display_name = before_session.public.display_name.clone();
    after_session.public.updated_at = before_session.public.updated_at;
    after_participant.display_name = before_participant.display_name.clone();
    after_participant.updated_at = before_participant.updated_at;
    assert_eq!(
        serde_json::to_value(after_session)?,
        serde_json::to_value(before_session)?
    );
    assert_eq!(after_participant, before_participant);
    Ok(())
}

#[tokio::test]
async fn identity_update_rejects_wrong_authority_and_malformed_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let valid = json!({"agent_id": AGENT_ID, "display_name": "Name"});
    {
        let mut denied = principal.clone();
        denied.capabilities.agent_control = false;
        assert!(matches!(
            store
                .execute_agent_profile_update(TrustedPrincipal(&denied), "denied", &valid)
                .await,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"permission_denied")
        ));
    }
    let mut bridge = principal.clone();
    bridge.client_kind = ClientKind::AgentBridge;
    assert!(matches!(
        store
            .execute_agent_profile_update(TrustedPrincipal(&bridge), "bridge", &valid)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"permission_denied")
    ));
    for value in [
        json!(null),
        json!(42),
        json!(""),
        json!(" "),
        json!("a\nb"),
        json!("a".repeat(81)),
    ] {
        assert!(matches!(
            store
                .execute_agent_profile_update(
                    TrustedPrincipal(&principal),
                    "invalid",
                    &json!({"agent_id": AGENT_ID, "display_name": value})
                )
                .await,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"bad_request")
        ));
    }
    for payload in [
        json!({"agent_id": AGENT_ID}),
        json!({"agent_id": AGENT_ID, "display_name": "Name", "role": "host"}),
        json!({"agent_id": AGENT_ID, "display_name": "Name", "avatar_image_url": "/foreign"}),
    ] {
        assert!(matches!(
            store
                .execute_agent_profile_update(TrustedPrincipal(&principal), "invalid", &payload)
                .await,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"bad_request")
        ));
    }
    let mut other_room = principal.clone();
    other_room.room_id = "other".into();
    assert!(
        store
            .execute_agent_profile_update(TrustedPrincipal(&other_room), "foreign", &valid)
            .await
            .is_err()
    );
    let mut transaction = store.pool.begin().await?;
    let mut participant = load_participant(&mut transaction, "general", AGENT_ID).await?;
    participant.participant_type = "human".into();
    save_participant(
        &mut transaction,
        &participant.room_id,
        &participant.participant_id,
        &participant,
    )
    .await?;
    transaction.commit().await?;
    assert!(matches!(
        store
            .execute_agent_profile_update(TrustedPrincipal(&principal), "human", &valid)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"stored_agent_identity_invalid")
    ));
    Ok(())
}

#[tokio::test]
async fn missing_exact_participant_write_rolls_back_identity_and_replay()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let before = store.participant("general", AGENT_ID).await?;
    sqlx::query("CREATE TRIGGER ignore_participant_write BEFORE UPDATE ON participants BEGIN SELECT RAISE(IGNORE); END")
        .execute(&store.pool).await?;
    let payload = json!({"agent_id": AGENT_ID, "display_name": "Changed"});
    assert!(matches!(
        store
            .execute_agent_profile_update(TrustedPrincipal(&principal), "exact-write", &payload)
            .await,
        Err(PersistenceError::ParticipantMissing)
    ));
    assert_eq!(store.participant("general", AGENT_ID).await?, before);
    let mut transaction = store.pool.begin().await?;
    assert_eq!(
        load_session(&mut transaction, "general", AGENT_ID)
            .await?
            .public
            .display_name,
        before.display_name
    );
    transaction.rollback().await?;
    sqlx::query("DROP TRIGGER ignore_participant_write")
        .execute(&store.pool)
        .await?;
    let retry = store
        .execute_agent_profile_update(TrustedPrincipal(&principal), "exact-write", &payload)
        .await?;
    assert!(!retry.deduplicated);
    assert_eq!(retry.result["participant"]["display_name"], "Changed");
    Ok(())
}
