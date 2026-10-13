use super::tests::{AGENT_ID, fixture};
use crate::{
    AgentControlAuthority, AgentStartPlan, AgentStopPlan, DelegateRoomAuthority, PersistenceError,
    RoomMutationAuthority, SqliteStore,
};
use agentsassemble_domain::{DirectRoomAction, InviteScope, StartAgent, StopAgent};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn delegated_external_stop_keeps_custody_and_finishes_at_cleanup() -> TestResult {
    let (mut store, connection, _, _) =
        crate::attendee::turn_report_tests::assigned_report().await?;
    let delegate = director(&store).await?;
    let request = DirectRoomAction::StopAgent(StopAgent {
        request_id: Uuid::new_v4().to_string(),
        agent_id: connection.session().principal().participant_id.clone(),
    });
    let (control, _) = store
        .admit_delegated_agent_control(&delegate, &request, |_, _, _| Ok(()))
        .await?;
    let cleanup = store
        .authorize_attendee_cleanup(connection.session().session_fingerprint())
        .await?;
    for restarted in [false, true] {
        if restarted {
            store.runtime_generation = "restarted-server-generation".into();
        }
        let AgentStopPlan::ExternalPending(events) = store
            .prepare_agent_stop(
                AgentControlAuthority::Delegated(&control),
                control.request_id(),
                control.payload(),
            )
            .await?
        else {
            return Err("external stop transferred custody or completed prematurely".into());
        };
        assert_eq!(events.len(), usize::from(!restarted));
        assert_executor(&events, &control.principal().participant_id)?;
    }
    let report = crate::AttendeeCleanupReport {
        request_id: Uuid::new_v4(),
        stopped: store
            .load_attendee_cleanup(&cleanup)
            .await?
            .ok_or("missing cleanup")?,
    };
    store.record_attendee_cleanup(&cleanup, &report).await?;
    let AgentStopPlan::Outcome(outcome) = store
        .prepare_agent_stop(
            AgentControlAuthority::Delegated(&control),
            control.request_id(),
            control.payload(),
        )
        .await?
    else {
        return Err("delegated stop receipt missing".into());
    };
    assert!(outcome.deduplicated);
    assert_eq!(outcome.result["status"], "succeeded");
    assert_eq!(
        outcome.event.extra["actor_participant_id"],
        control.principal().participant_id
    );
    assert_executor(&outcome.events, &control.principal().participant_id)?;
    assert!(!serde_json::to_string(&outcome.result)?.contains(&report.stopped.runtime_lease_token));
    assert!(store.load_attendee_cleanup(&cleanup).await?.is_none());
    Ok(())
}

async fn director(
    store: &SqliteStore,
) -> Result<DelegateRoomAuthority, Box<dyn std::error::Error>> {
    let manager = crate::room_user_identity::test_authority(store).await;
    let invite = store
        .create_connector_invite(
            &crate::RoomManagerAuthority::Local(manager),
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let admitted = store
        .admit_connector(
            &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
            &[73; 32],
            Uuid::new_v4(),
            "Director",
            chrono::Utc::now(),
        )
        .await?;
    let operator = crate::human_session_authority_tests::local_operator_principal();
    store.execute_participant_role_update(RoomMutationAuthority::TrustedPrincipal(&operator), "director", &json!({"participant_id":admitted.authorization.principal().participant_id,"role":"director"})).await?;
    Ok(DelegateRoomAuthority::Connector(admitted.authorization))
}
fn request(start: bool) -> DirectRoomAction {
    let request_id = Uuid::new_v4().to_string();
    if start {
        DirectRoomAction::StartAgent(StartAgent {
            request_id,
            agent_id: AGENT_ID.into(),
        })
    } else {
        DirectRoomAction::StopAgent(StopAgent {
            request_id,
            agent_id: AGENT_ID.into(),
        })
    }
}
async fn session_json(store: &SqliteStore) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT session_json FROM agent_sessions WHERE room_id='general' AND session_id=?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
}

#[tokio::test]
async fn delegated_lifecycle_uses_shared_effect_receipts_and_replay() -> TestResult {
    let (store, _, _directory) = fixture().await;
    let delegate = director(&store).await?;
    let start_request = request(true);
    let (start, _) = store
        .admit_delegated_agent_control(&delegate, &start_request, |owner, delegate, _| {
            assert_ne!(owner, delegate);
            Ok(())
        })
        .await?;
    assert!(!start.principal().capabilities.agent_control);
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_launch(
            AgentControlAuthority::Delegated(&start),
            start.request_id(),
            start.payload(),
            start.action(),
        )
        .await?
    else {
        panic!("start effect missing")
    };
    store
        .authorize_agent_start_effect(
            AgentControlAuthority::Delegated(&start),
            start.request_id(),
            start.payload(),
            &effect.operation_id,
            start.action(),
            "owned-runtime-1",
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await?;
    let started = store
        .complete_agent_start(
            RoomMutationAuthority::TrustedPrincipal(start.principal()),
            start.request_id(),
            start.payload(),
            &effect.operation_id,
            &crate::AgentRuntimeStarted {
                runtime_handle_id: "owned-runtime-1".into(),
                runtime_owner_id: "supervisor-instance-1".into(),
                runtime_lease_token: "lease-generation-1".into(),
                provider_session_id: "provider-1".into(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await?;
    assert_eq!(started.result["status"], "succeeded");
    assert_eq!(started.event.extra["action"], "start_agent");
    let (retry, debit) = store
        .admit_delegated_agent_control(
            &delegate,
            &start_request,
            |_, _, _| -> Result<(), PersistenceError> { panic!("completed retry charged") },
        )
        .await?;
    assert!(debit.is_none());
    let AgentStartPlan::Outcome(replayed) = store
        .prepare_agent_launch(
            AgentControlAuthority::Delegated(&retry),
            retry.request_id(),
            retry.payload(),
            retry.action(),
        )
        .await?
    else {
        panic!("replay relaunched")
    };
    assert_eq!(started.result, replayed.result);
    let mut changed = start_request.clone();
    if let DirectRoomAction::StartAgent(r) = &mut changed {
        r.agent_id = "missing".into();
    }
    // Identity conflict is checked before target effects; missing targets cannot launch anything.
    assert!(matches!(
        store
            .admit_delegated_agent_control(&delegate, &changed, |_, _, _| Ok(()))
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    let (stop, _) = store
        .admit_delegated_agent_control(&delegate, &request(false), |_, _, _| Ok(()))
        .await?;
    let AgentStopPlan::Stop(effect) = store
        .prepare_agent_stop(
            AgentControlAuthority::Delegated(&stop),
            stop.request_id(),
            stop.payload(),
        )
        .await?
    else {
        panic!("stop effect missing")
    };
    let operator = crate::human_session_authority_tests::local_operator_principal();
    store
        .execute_participant_role_update(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            "demote-before-stop",
            &json!({"participant_id":stop.principal().participant_id,"role":"agent"}),
        )
        .await?;
    assert!(
        matches!(store.authorize_agent_stop_effect(AgentControlAuthority::Delegated(&stop), stop.request_id(), stop.payload(), &effect.operation_id).await,
        Err(PersistenceError::CommandRejected {code,..}) if code=="role_required")
    );
    store
        .execute_participant_role_update(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            "restore-before-stop",
            &json!({"participant_id":stop.principal().participant_id,"role":"director"}),
        )
        .await?;
    store
        .authorize_agent_stop_effect(
            AgentControlAuthority::Delegated(&stop),
            stop.request_id(),
            stop.payload(),
            &effect.operation_id,
        )
        .await?;
    store
        .record_agent_stop_effect("general", AGENT_ID, &effect.operation_id)
        .await?;
    // Reconciliation finalizes the same durable request with its attribution intact.
    let candidate = store
        .load_runtime_reconciliation_candidates()
        .await?
        .into_iter()
        .find(|c| c.session.public.session_id == AGENT_ID)
        .ok_or("missing candidate")?;
    store
        .apply_runtime_reconciliation(&candidate, &crate::RuntimeReconciliationObservation::Gone)
        .await?;
    let AgentStopPlan::Outcome(stopped) = store
        .prepare_agent_stop(
            AgentControlAuthority::Delegated(&stop),
            stop.request_id(),
            stop.payload(),
        )
        .await?
    else {
        panic!("stop did not replay")
    };
    assert_eq!(stopped.result["status"], "succeeded");
    for outcome in [&started, &*stopped] {
        assert_executor(&outcome.events, &start.principal().participant_id)?;
        assert_eq!(
            outcome.event.extra["actor_participant_id"],
            start.principal().participant_id
        );
        assert_eq!(
            outcome.event.extra["on_behalf_of_participant_id"],
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID
        );
        assert!(
            outcome
                .events
                .iter()
                .all(|event| event.extra.contains_key("on_behalf_of_participant_id"))
        );
    }
    let before = store.snapshot("general", 0, 200).await?.last_seq;
    let (retry_start, _) = store
        .admit_delegated_agent_control(&delegate, &request(true), |_, _, _| Ok(()))
        .await?;
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_launch(
            AgentControlAuthority::Delegated(&retry_start),
            retry_start.request_id(),
            retry_start.payload(),
            retry_start.action(),
        )
        .await?
    else {
        return Err("no retry start".into());
    };
    store
        .authorize_agent_start_effect(
            AgentControlAuthority::Delegated(&retry_start),
            retry_start.request_id(),
            retry_start.payload(),
            &effect.operation_id,
            retry_start.action(),
            "owned-runtime-2",
            "supervisor-instance-2",
            "lease-generation-2",
        )
        .await?;
    let pending = store
        .mark_agent_start_unconfirmed(
            retry_start.principal(),
            AGENT_ID,
            &effect.operation_id,
            "owned-runtime-2",
            "supervisor-instance-2",
            "controlled_unknown",
            "Controlled unconfirmed start.",
        )
        .await?;
    assert_executor(&pending, &retry_start.principal().participant_id)?;
    let candidate = store
        .load_runtime_reconciliation_candidates()
        .await?
        .into_iter()
        .find(|c| c.session.public.session_id == AGENT_ID)
        .ok_or("missing retry candidate")?;
    store
        .apply_runtime_reconciliation(&candidate, &crate::RuntimeReconciliationObservation::Gone)
        .await?;
    let recovered = store.snapshot("general", before, 200).await?.events;
    assert!(recovered.iter().any(|event| event.event_type == "error"));
    assert_executor(&recovered, &retry_start.principal().participant_id)?;
    Ok(())
}

#[tokio::test]
async fn control_revalidates_before_reservation_and_effect_and_refuses_execution_permissions()
-> TestResult {
    let (store, operator, _directory) = fixture().await;
    let delegate = director(&store).await?;
    let original = session_json(&store).await?;
    for mode in ["workspace_write", "full_access"] {
        sqlx::query("UPDATE agent_sessions SET session_json=json_set(session_json,'$.permission_mode',?) WHERE session_id=?").bind(mode).bind(AGENT_ID).execute(&store.pool).await?;
        let before = session_json(&store).await?;
        let error = store
            .admit_delegated_agent_control(
                &delegate,
                &request(true),
                |_, _, _| -> Result<(), PersistenceError> { panic!("refusal charged") },
            )
            .await
            .err()
            .ok_or("start accepted")?;
        assert!(
            matches!(error, PersistenceError::CommandRejected {code,..} if code=="owner_confirmation_required")
        );
        assert_eq!(before, session_json(&store).await?);
    }
    sqlx::query("UPDATE agent_sessions SET session_json=? WHERE session_id=?")
        .bind(&original)
        .bind(AGENT_ID)
        .execute(&store.pool)
        .await?;
    for start in [true, false] {
        let (control, _) = store
            .admit_delegated_agent_control(&delegate, &request(start), |_, _, _| Ok(()))
            .await?;
        for (role, muted, expected) in [
            ("agent", false, "role_required"),
            ("director", true, "owner_muted"),
        ] {
            store
                .execute_participant_role_update(
                    RoomMutationAuthority::TrustedPrincipal(&operator),
                    &Uuid::new_v4().to_string(),
                    &json!({"participant_id":control.principal().participant_id,"role":role}),
                )
                .await?;
            let mut owner = store
                .participant("general", &operator.participant_id)
                .await?;
            owner.muted = muted;
            let mut tx = store.pool.begin().await?;
            crate::participant_rows::save_participant_exact(
                &mut tx,
                "general",
                &owner.participant_id,
                &owner,
            )
            .await?;
            tx.commit().await?;
            let error = if start {
                store
                    .prepare_agent_launch(
                        AgentControlAuthority::Delegated(&control),
                        control.request_id(),
                        control.payload(),
                        control.action(),
                    )
                    .await
                    .err()
            } else {
                store
                    .prepare_agent_stop(
                        AgentControlAuthority::Delegated(&control),
                        control.request_id(),
                        control.payload(),
                    )
                    .await
                    .err()
            }
            .ok_or("stale authority accepted")?;
            assert!(matches!(error, PersistenceError::CommandRejected {code,..} if code==expected));
            owner.muted = false;
            let mut tx = store.pool.begin().await?;
            crate::participant_rows::save_participant_exact(
                &mut tx,
                "general",
                &owner.participant_id,
                &owner,
            )
            .await?;
            tx.commit().await?;
        }
        assert_eq!(original, session_json(&store).await?);
    }
    let (control, _) = store
        .admit_delegated_agent_control(&delegate, &request(true), |_, _, _| Ok(()))
        .await?;
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_launch(
            AgentControlAuthority::Delegated(&control),
            control.request_id(),
            control.payload(),
            control.action(),
        )
        .await?
    else {
        panic!("no start")
    };
    sqlx::query("UPDATE agent_sessions SET session_json=json_set(session_json,'$.permission_mode','full_access') WHERE session_id=?").bind(AGENT_ID).execute(&store.pool).await?;
    assert!(
        matches!(store.authorize_agent_start_effect(AgentControlAuthority::Delegated(&control), control.request_id(), control.payload(), &effect.operation_id, control.action(), "handle", "owner", "lease").await,
        Err(PersistenceError::CommandRejected {code,..}) if code=="owner_confirmation_required")
    );
    // A human request cannot smuggle the private delegation envelope into its own lifecycle.
    assert!(
        store
            .prepare_agent_start(
                RoomMutationAuthority::TrustedPrincipal(&operator),
                &Uuid::new_v4().to_string(),
                control.payload()
            )
            .await
            .is_err()
    );
    Ok(())
}

fn assert_executor(events: &[agentsassemble_domain::RoomEvent], actor: &str) -> TestResult {
    for event in events {
        assert_eq!(event.actor.participant_id, actor);
        assert_eq!(event.actor.participant_type, "agent");
        assert_eq!(event.actor_id.as_deref(), Some(actor));
        assert_eq!(event.actor_type.as_deref(), Some("agent"));
        assert_eq!(
            event.extra["on_behalf_of_participant_id"],
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID
        );
        let imported = crate::backup_json::event(serde_json::to_value(event)?)?;
        assert_eq!(imported.actor, event.actor);
        let viewer = crate::human_session_authority_tests::local_operator_principal();
        let projected = agentsassemble_domain::public_event_for_principal(event, &viewer);
        assert_eq!(projected.actor, event.actor);
        assert_eq!(
            projected.extra["on_behalf_of_participant_id"],
            event.extra["on_behalf_of_participant_id"]
        );
    }
    Ok(())
}

#[path = "delegated_confirmed_agent_tests.rs"]
mod confirmed;

#[path = "delegated_confirmed_create_tests.rs"]
mod confirmed_create;
