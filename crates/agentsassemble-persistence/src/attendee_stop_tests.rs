use agentsassemble_domain::{AgentRuntimeStatus, ParticipantStatus};
use serde_json::json;
use uuid::Uuid;

use crate::{
    AgentStopPlan, AttendeeCleanupReport, PersistenceError,
    RoomMutationAuthority::TrustedPrincipal, attendee_turn_report_tests::assigned_report,
    human_session_authority_tests::local_operator_principal,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn external_operator_stop_survives_restart_and_commits_both_receipts_after_exact_report()
-> TestResult {
    let (mut store, connection, turn, now) = assigned_report().await?;
    let operator = local_operator_principal();
    let payload = json!({"agent_id": connection.session().principal().participant_id});
    let authority = store
        .authorize_attendee_cleanup(connection.session().session_fingerprint())
        .await?;
    let AgentStopPlan::ExternalPending(events) = store
        .prepare_agent_stop(TrustedPrincipal(&operator), "operator-stop", &payload)
        .await?
    else {
        return Err("external stop dispatched to host".into());
    };
    assert_eq!(events.len(), 1);
    let stopped = store
        .load_attendee_cleanup(&authority)
        .await?
        .ok_or("stop missing")?;
    assert_eq!(stopped.runtime_handle_id, turn.runtime_handle_id);
    store.runtime_generation = "restarted-server-generation".into();
    let AgentStopPlan::ExternalPending(retry_events) = store
        .prepare_agent_stop(TrustedPrincipal(&operator), "operator-stop", &payload)
        .await?
    else {
        return Err("restart transferred external custody".into());
    };
    assert!(retry_events.is_empty());
    assert!(store.load_attendee_cleanup(&authority).await? == Some(stopped.clone()));
    assert!(matches!(
        store
            .prepare_agent_stop(
                TrustedPrincipal(&operator),
                "operator-stop",
                &json!({"agent_id":"changed"}),
            )
            .await,
        Err(PersistenceError::CommandConflict | PersistenceError::CommandRejected { .. })
    ));
    assert!(
        store
            .record_attendee_turn_report(&connection, &turn, now)
            .await
            .is_err()
    );
    let pending = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        pending.agent_sessions[0].runtime_status,
        AgentRuntimeStatus::Stopping
    );
    assert!(pending.agent_sessions[0].provider_session_active);
    let report = AttendeeCleanupReport {
        request_id: Uuid::new_v4(),
        stopped,
    };
    let completed = store.record_attendee_cleanup(&authority, &report).await?;
    assert_eq!(
        completed
            .outcome
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    let replay = store.record_attendee_cleanup(&authority, &report).await?;
    assert!(replay.outcome.deduplicated);
    assert_eq!(completed.outcome.event.id, replay.outcome.event.id);
    let AgentStopPlan::Outcome(command) = store
        .prepare_agent_stop(TrustedPrincipal(&operator), "operator-stop", &payload)
        .await?
    else {
        return Err("operator receipt missing".into());
    };
    assert!(command.deduplicated);
    assert_eq!(command.result["process"]["ownership"], "external");
    assert_eq!(command.result["process"]["confirmed"], true);
    assert!(!serde_json::to_string(&command.result)?.contains(&report.stopped.runtime_lease_token));
    assert!(store.load_attendee_cleanup(&authority).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn external_stop_can_finish_after_room_archive_without_reacquiring_room_authority()
-> TestResult {
    let (store, connection, _, _) = assigned_report().await?;
    let operator = local_operator_principal();
    let payload = json!({"agent_id":connection.session().principal().participant_id});
    assert!(matches!(
        store
            .prepare_agent_stop(TrustedPrincipal(&operator), "stop-before-archive", &payload)
            .await?,
        AgentStopPlan::ExternalPending(_)
    ));
    let uid = store.snapshot("general", 0, 200).await?.room.room_uid;
    store
        .execute_room_lifecycle(
            TrustedPrincipal(&operator),
            "archive-after-stop",
            "room.archive",
            &json!({"room_uid":uid,"archived":true}),
        )
        .await?;
    let authority = store
        .authorize_attendee_cleanup(connection.session().session_fingerprint())
        .await?;
    let stopped = store
        .load_attendee_cleanup(&authority)
        .await?
        .ok_or("stop missing")?;
    let completed = store
        .record_attendee_cleanup(
            &authority,
            &AttendeeCleanupReport {
                request_id: Uuid::new_v4(),
                stopped,
            },
        )
        .await?;
    assert!(completed.assignments.is_empty());
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot.agent_sessions[0].runtime_status,
        AgentRuntimeStatus::Stopped
    );
    assert!(!snapshot.agent_sessions[0].provider_session_active);
    assert!(snapshot.participants.iter().any(|p| p.participant_id
        == connection.session().principal().participant_id
        && p.status != ParticipantStatus::Joined));
    Ok(())
}
