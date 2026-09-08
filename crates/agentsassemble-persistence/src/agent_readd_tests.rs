use crate::RoomMutationAuthority::TrustedPrincipal;
use crate::participant_rows::save_participant_exact as save_participant;
use agentsassemble_domain::ParticipantStatus;
use serde_json::json;

use crate::{
    AgentRuntimeStarted, AgentStartPlan, PersistenceError,
    agent_lifecycle::{
        load_participant, load_session,
        tests::{AGENT_ID, fixture},
    },
};

#[tokio::test]
async fn listing_readd_preserves_room_authority_and_replays_after_reopen()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let mut transaction = store.pool.begin().await?;
    let mut participant = load_participant(&mut transaction, "general", AGENT_ID).await?;
    participant.status = ParticipantStatus::Kicked;
    participant.muted = true;
    participant.display_name = "Room-owned name".to_owned();
    save_participant(
        &mut transaction,
        &participant.room_id,
        &participant.participant_id,
        &participant,
    )
    .await?;
    transaction.commit().await?;
    let mut transaction = store.pool.begin().await?;
    let before = load_session(&mut transaction, "general", AGENT_ID).await?;
    transaction.commit().await?;
    let payload = json!({"agent_id": AGENT_ID, "start": false});
    let AgentStartPlan::Outcome(outcome) = store
        .prepare_agent_launch(
            TrustedPrincipal(&principal),
            "readd-list",
            &payload,
            "agent.readd",
        )
        .await?
    else {
        panic!("listing must not launch")
    };
    assert_eq!(outcome.events.len(), 1);
    assert_eq!(outcome.events[0].event_type, "agent_session_reactivated");
    assert_eq!(outcome.result["participant"]["status"], "detached");
    assert_eq!(outcome.result["participant"]["muted"], true);
    assert_eq!(
        outcome.result["participant"]["display_name"],
        "Room-owned name"
    );
    let mut transaction = store.pool.begin().await?;
    let after = load_session(&mut transaction, "general", AGENT_ID).await?;
    transaction.commit().await?;
    let mut normalized = after.clone();
    normalized.public.updated_at = before.public.updated_at;
    assert_eq!(
        serde_json::to_value(normalized)?,
        serde_json::to_value(before)?
    );
    assert!(matches!(
        store
            .prepare_agent_launch(
                TrustedPrincipal(&principal),
                "readd-list",
                &json!({"agent_id": AGENT_ID, "start": true}),
                "agent.readd"
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    store.pool.close().await;
    drop(store);
    let reopened = crate::SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let AgentStartPlan::Outcome(replay) = reopened
        .prepare_agent_launch(
            TrustedPrincipal(&principal),
            "readd-list",
            &payload,
            "agent.readd",
        )
        .await?
    else {
        panic!("replay must not launch")
    };
    assert!(replay.deduplicated);
    assert_eq!(replay.result, outcome.result);
    Ok(())
}

#[tokio::test]
async fn started_readd_joins_only_after_exact_effect_and_replays_once()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let authority = TrustedPrincipal(&principal);
    let mut transaction = store.pool.begin().await?;
    let mut participant = load_participant(&mut transaction, "general", AGENT_ID).await?;
    participant.status = ParticipantStatus::Kicked;
    save_participant(
        &mut transaction,
        &participant.room_id,
        &participant.participant_id,
        &participant,
    )
    .await?;
    transaction.commit().await?;
    let payload = json!({"agent_id": AGENT_ID, "start_now": true});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_launch(authority, "readd-start", &payload, "agent.readd")
        .await?
    else {
        panic!("launch required")
    };
    let mut transaction = store.pool.begin().await?;
    assert_eq!(
        load_participant(&mut transaction, "general", AGENT_ID)
            .await?
            .status,
        ParticipantStatus::Kicked
    );
    transaction.commit().await?;
    store
        .authorize_agent_start_effect(
            authority,
            "readd-start",
            &payload,
            &effect.operation_id,
            "agent.readd",
            "runtime",
            "owner",
            "lease",
        )
        .await?;
    let started = AgentRuntimeStarted {
        runtime_handle_id: "runtime".to_owned(),
        runtime_owner_id: "owner".to_owned(),
        runtime_lease_token: "lease".to_owned(),
        provider_session_id: "conversation".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    };
    let outcome = store
        .complete_agent_launch(
            &principal,
            "readd-start",
            &payload,
            &effect.operation_id,
            &started,
            "agent.readd",
        )
        .await?;
    assert_eq!(outcome.result["status"], "readded");
    assert_eq!(outcome.result["participant"]["status"], "joined");
    assert_eq!(outcome.result["agent_session"]["runtime_status"], "idle");
    assert_eq!(
        outcome
            .events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        [
            "participant_joined",
            "session_attached",
            "agent_session_state"
        ]
    );
    let AgentStartPlan::Outcome(replay) = store
        .prepare_agent_launch(authority, "readd-start", &payload, "agent.readd")
        .await?
    else {
        panic!("must not relaunch")
    };
    assert_eq!(outcome.result, replay.result);
    assert!(
        store
            .prepare_agent_launch(authority, "readd-active", &payload, "agent.readd")
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn readd_rejects_untrusted_flags_and_missing_control_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, mut principal, _directory) = fixture().await;
    for payload in [
        json!({"agent_id": AGENT_ID, "start": "true"}),
        json!({"agent_id": AGENT_ID, "start": false, "start_now": true}),
    ] {
        assert!(matches!(
            store
                .prepare_agent_launch(
                    TrustedPrincipal(&principal),
                    "invalid-readd",
                    &payload,
                    "agent.readd"
                )
                .await,
            Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"bad_request")
        ));
    }
    principal.is_operator = false;
    principal.capabilities.agent_control = false;
    assert!(
        store
            .prepare_agent_launch(
                TrustedPrincipal(&principal),
                "forbidden-readd",
                &json!({"agent_id": AGENT_ID}),
                "agent.readd"
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn readd_rejects_cross_bound_stored_identity_before_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    for start in [false, true] {
        for (participant_row, field) in [
            (false, "room_id"),
            (false, "session_id"),
            (false, "participant_id"),
            (true, "room_id"),
            (true, "participant_id"),
        ] {
            let (store, principal, _directory) = fixture().await;
            let (update, read) = if participant_row {
                (
                    "UPDATE participants SET participant_json = json_set(participant_json, ?, 'foreign-id') WHERE room_id = 'general' AND participant_id = ?",
                    "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
                )
            } else {
                (
                    "UPDATE agent_sessions SET session_json = json_set(session_json, ?, 'foreign-id') WHERE room_id = 'general' AND session_id = ?",
                    "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
                )
            };
            sqlx::query(update)
                .bind(format!("$.{field}"))
                .bind(AGENT_ID)
                .execute(&store.pool)
                .await?;
            let before: String = sqlx::query_scalar(read)
                .bind(AGENT_ID)
                .fetch_one(&store.pool)
                .await?;
            assert!(matches!(
                store
                    .prepare_agent_launch(
                        TrustedPrincipal(&principal),
                        "corrupt-readd",
                        &json!({"agent_id": AGENT_ID, "start": start}),
                        "agent.readd"
                    )
                    .await,
                Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"stored_agent_identity_invalid")
            ));
            let after: String = sqlx::query_scalar(read)
                .bind(AGENT_ID)
                .fetch_one(&store.pool)
                .await?;
            assert_eq!(before, after);
            let effects: i64 = sqlx::query_scalar("SELECT count(*) FROM lifecycle_command_reservations WHERE request_id = 'corrupt-readd'").fetch_one(&store.pool).await?;
            assert_eq!(effects, 0);
        }
    }
    Ok(())
}
