use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{AgentTurnAssignment, PersistenceError, SqliteStore};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAttachmentReadIngress, ProviderRoomToolIngress,
};
use tokio::{
    sync::{broadcast, oneshot},
    task::JoinSet,
};

use crate::{
    provider_recovery_tracker::ProviderRecoveryGuard,
    provider_turn::{ProviderTurnTaskResult, spawn_recovered_provider_turn},
};

pub(super) struct RecoveredAssignment {
    pub(super) assignment: AgentTurnAssignment,
    pub(super) guard: ProviderRecoveryGuard,
}

pub(super) struct RecoveredAssignments {
    pub(super) assignments: Vec<RecoveredAssignment>,
    pub(super) reply: oneshot::Sender<Result<(), PersistenceError>>,
}

pub(super) struct RecoveryRuntime<'a> {
    pub(super) store: &'a SqliteStore,
    pub(super) event_tx: &'a broadcast::Sender<RoomEvent>,
    pub(super) room_id: &'a str,
    pub(super) turn_tasks: &'a mut JoinSet<ProviderTurnTaskResult>,
    pub(super) provider_adapter: &'a ProviderAdapter,
    pub(super) room_tool_ingress: &'a ProviderRoomToolIngress,
    pub(super) attachment_ingress: &'a ProviderAttachmentReadIngress,
}

impl RecoveryRuntime<'_> {
    pub(super) async fn publish_then_resume(
        self,
        recovery: RecoveredAssignments,
    ) -> crate::event_publication::PublicationAttempt {
        publish_before_recovery_entry(
            self.store,
            self.event_tx,
            self.room_id,
            recovery,
            |assignments| {
                for recovered in assignments {
                    spawn_recovered_provider_turn(
                        self.turn_tasks,
                        self.store.clone(),
                        self.provider_adapter.clone(),
                        recovered.assignment,
                        self.room_tool_ingress.clone(),
                        self.attachment_ingress.clone(),
                        recovered.guard,
                    );
                }
            },
        )
        .await
    }
}

async fn publish_before_recovery_entry(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<RoomEvent>,
    room_id: &str,
    recovery: RecoveredAssignments,
    enter_recovery: impl FnOnce(Vec<RecoveredAssignment>),
) -> crate::event_publication::PublicationAttempt {
    let result = crate::event_publication::drain_room_publications(store, event_tx, room_id).await;
    let publication = if result.is_ok() {
        crate::event_publication::PublicationAttempt::Drained
    } else {
        crate::event_publication::PublicationAttempt::Retry
    };
    if result.is_ok() {
        enter_recovery(recovery.assignments);
    }
    let _ = recovery.reply.send(result);
    publication
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::AuthenticatedPrincipal;
    use agentsassemble_persistence::{
        AgentRuntimeStarted, AgentStartPlan, AgentTurnAssignment, SqliteStore,
    };
    use agentsassemble_provider::{ProviderAdapter, ProviderStartReservation};
    use serde_json::json;
    use tokio::sync::broadcast;

    use super::{RecoveredAssignment, RecoveredAssignments, publish_before_recovery_entry};
    use crate::{
        provider_recovery_tracker::ProviderRecoveryTracker,
        runtime_reconciliation::tests::{draft, local_principal},
    };

    struct StartedAgentFixture {
        _directory: tempfile::TempDir,
        store: SqliteStore,
        principal: AuthenticatedPrincipal,
        session_id: String,
        lease_owner: ProviderAdapter,
        reservation: ProviderStartReservation,
    }

    struct RecoveryAssignmentFixture {
        started: StartedAgentFixture,
        assignment: AgentTurnAssignment,
        expected_event_seq: i64,
    }

    #[tokio::test]
    async fn recovered_turn_enters_provider_only_after_durable_publication() {
        let fixture = stage_recovery_assignment().await;
        let tracker = ProviderRecoveryTracker::default();
        let guard = tracker
            .try_claim(&fixture.assignment)
            .unwrap_or_else(|| panic!("claim exact recovered assignment"));
        let (event_tx, mut event_rx) = broadcast::channel(32);
        let (reply, response) = tokio::sync::oneshot::channel();
        let mut entered_recovery = false;
        let _ = publish_before_recovery_entry(
            &fixture.started.store,
            &event_tx,
            "general",
            RecoveredAssignments {
                assignments: vec![RecoveredAssignment {
                    assignment: fixture.assignment.clone(),
                    guard,
                }],
                reply,
            },
            |assignments| {
                let [entered] = assignments.as_slice() else {
                    panic!("recovery entry did not receive the exact assignment");
                };
                assert_eq!(
                    entered.assignment.session.public.session_id,
                    fixture.started.session_id
                );
                let published = event_rx.try_recv().unwrap_or_else(|error| {
                    panic!("publication was not visible at entry: {error}")
                });
                assert_eq!(published.seq, fixture.expected_event_seq);
                entered_recovery = true;
            },
        )
        .await;
        response
            .await
            .unwrap_or_else(|error| panic!("receive handoff completion: {error}"))
            .unwrap_or_else(|error| panic!("complete ordered recovery handoff: {error}"));

        assert!(entered_recovery);
        assert!(
            fixture
                .started
                .store
                .pending_room_publications("general")
                .await
                .unwrap_or_else(|error| panic!("read recovery publication cursor: {error}"))
                .is_empty()
        );
        assert!(tracker.try_claim(&fixture.assignment).is_some());

        fixture
            .started
            .lease_owner
            .cancel_start_reservation(
                "general",
                &fixture.started.session_id,
                &fixture.started.reservation,
            )
            .await;
    }

    async fn stage_started_agent() -> StartedAgentFixture {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("66fb7f89-b6a1-4b32-85a4-4c39c5fa5bf7", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
        store
            .create_room_for_local_operator(
                "97a426de-1418-40db-81d5-eea0c6a891fa",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        let principal = local_principal();
        let created = store
            .execute_agent_create(
                &principal,
                "create-recovery-publication-agent",
                &json!({"provider_id": "opencode"}),
                &draft(
                    directory.path(),
                    "codex-00000000-0000-5000-8000-000000000231",
                ),
            )
            .await
            .unwrap_or_else(|error| panic!("create recovery agent: {error}"));
        let session_id = created.result["agent_session"]["session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("created session has no id"))
            .to_owned();
        let payload = json!({"agent_id": session_id});
        let AgentStartPlan::Start(effect) = store
            .prepare_agent_start(&principal, "start-recovery-publication-agent", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare start: {error}"))
        else {
            panic!("stopped session must prepare start");
        };
        let lease_owner = ProviderAdapter::new();
        let reservation = lease_owner
            .reserve_start(&effect.session)
            .await
            .unwrap_or_else(|error| panic!("reserve runtime authority: {error}"));
        store
            .authorize_agent_start_effect(
                &principal,
                "start-recovery-publication-agent",
                &payload,
                &effect.operation_id,
                "agent.start",
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
                &reservation.runtime_lease_token,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize runtime start: {error}"));
        store
            .complete_agent_start(
                &principal,
                "start-recovery-publication-agent",
                &payload,
                &effect.operation_id,
                &AgentRuntimeStarted {
                    runtime_handle_id: reservation.runtime_handle_id.clone(),
                    runtime_owner_id: reservation.runtime_owner_id.clone(),
                    runtime_lease_token: reservation.runtime_lease_token.clone(),
                    provider_session_id: "provider-session-recovery-publication".to_owned(),
                    runtime_reused: false,
                    provider_session_reused: false,
                    provider_session_active: true,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("persist runtime start: {error}"));

        StartedAgentFixture {
            _directory: directory,
            store,
            principal,
            session_id,
            lease_owner,
            reservation,
        }
    }

    async fn stage_recovery_assignment() -> RecoveryAssignmentFixture {
        let started = stage_started_agent().await;
        let (baseline_tx, _) = broadcast::channel(32);
        crate::event_publication::drain_room_publications(&started.store, &baseline_tx, "general")
            .await
            .unwrap_or_else(|error| panic!("drain fixture baseline: {error}"));
        let mutation = started
            .store
            .execute_message_with_turn(
                &started.principal,
                "recovery-publication-order",
                "message.send",
                &json!({"content": "@Terra publish before recovered provider entry"}),
            )
            .await
            .unwrap_or_else(|error| panic!("commit recovery assignment: {error}"));
        let assignment = mutation
            .assignments
            .first()
            .unwrap_or_else(|| panic!("message did not create a provider assignment"))
            .clone();
        let expected_event_seq = mutation.outcome.event.seq;
        RecoveryAssignmentFixture {
            started,
            assignment,
            expected_event_seq,
        }
    }
}
