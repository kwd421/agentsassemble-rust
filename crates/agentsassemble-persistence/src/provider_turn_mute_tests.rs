use agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID;
use serde_json::json;

use super::{
    AGENT_ID, SECOND_AGENT_ID, attached_session, fixture, insert_agent, participant, stored_session,
};
use agentsassemble_domain::ParticipantRole;
use chrono::Utc;

#[tokio::test]
async fn mute_preempts_unstarted_exact_turn_and_unmute_reschedules_once() {
    let (store, principal, _directory) = fixture().await;
    let assigned = store
        .execute_message_with_turn(
            &principal,
            "mute-source",
            "message.send",
            &json!({"content": "@Terra inspect this exact turn"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign turn before mute: {error}"));
    let assignment = assigned
        .assignments
        .first()
        .unwrap_or_else(|| panic!("assigned exact Agent turn"));
    let muted = store
        .execute_participant_mute(
            &principal,
            "mute-agent",
            &json!({"participant_id": AGENT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("mute exact Agent turn: {error}"));
    assert_eq!(muted.outcome.event.event_type, "participant_muted");
    assert_eq!(muted.outcome.event.extra["muted"], json!(true));
    let muted_event_seq = muted.outcome.event.seq;
    assert_eq!(muted.outcome.result["event_seq"], json!(muted_event_seq));
    let effect = muted
        .interrupt_effect
        .unwrap_or_else(|| panic!("active Agent interrupt effect"));
    assert_eq!(effect.execution_id, assignment.execution_id);
    assert_eq!(effect.turn_generation, assignment.turn_generation);
    assert_eq!(
        store
            .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
            .await
            .unwrap_or_else(|error| panic!("read muted execution: {error}"))
            .phase,
        crate::ProviderTurnExecutionPhase::InterruptPending
    );
    assert!(
        store
            .authorize_provider_turn_start(
                "general",
                AGENT_ID,
                assignment.turn_generation,
                &assignment.turn_id,
            )
            .await
            .is_err()
    );
    let replay = store
        .execute_participant_mute(
            &principal,
            "mute-agent",
            &json!({"participant_id": AGENT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("replay Agent mute: {error}"));
    assert!(replay.outcome.deduplicated);
    assert!(replay.interrupt_effect.is_none());
    assert_eq!(replay.outcome.result["event_seq"], json!(muted_event_seq));
    let claim = store
        .claim_provider_turn_interrupt(&effect, "10000000-0000-4000-8000-000000000099")
        .await
        .unwrap_or_else(|error| panic!("claim exact interrupt: {error}"));
    let waiting = store
        .mark_unstarted_interrupt_waiting(&claim)
        .await
        .unwrap_or_else(|error| panic!("record unstarted quiescence wait: {error}"));
    let finalized = store
        .finalize_interrupted_turn_retained(&waiting)
        .await
        .unwrap_or_else(|error| panic!("finalize retained runtime interrupt: {error}"));
    assert!(finalized.next_assignments.is_empty());
    let retained = stored_session(&store).await;
    assert_eq!(retained.public.runtime_status, "idle");
    assert!(!retained.runtime_handle_id.is_empty());
    assert_eq!(retained.pending_inputs.len(), 1);
    assert!(retained.inflight_inputs.is_empty());
    let unmuted = store
        .execute_participant_mute(
            &principal,
            "unmute-agent",
            &json!({"participant_id": AGENT_ID, "muted": false}),
        )
        .await
        .unwrap_or_else(|error| panic!("unmute and reschedule Agent: {error}"));
    assert_eq!(unmuted.assignments.len(), 1);
    assert_eq!(
        unmuted.assignments[0].turn_generation,
        assignment.turn_generation + 1
    );
    assert!(!stored_session(&store).await.schedule_requested);
}

#[tokio::test]
async fn human_mute_changes_only_room_participant_authority() {
    let (store, principal, _directory) = fixture().await;
    let muted = store
        .execute_participant_mute(
            &principal,
            "mute-human",
            &json!({"participant_id": LOCAL_OPERATOR_PARTICIPANT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("mute human participant: {error}"));
    assert!(muted.interrupt_effect.is_none());
    assert!(muted.assignments.is_empty());
    assert!(
        store
            .participant("general", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await
            .unwrap_or_else(|error| panic!("read muted human: {error}"))
            .muted
    );
}

#[tokio::test]
async fn pre_dispatch_provider_task_death_is_checkpointed_and_requeues_once() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "task-death-source",
            "message.send",
            &json!({"content": "@Terra own this task"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign task-death turn: {error}"));
    let assignment = mutation
        .assignments
        .first()
        .unwrap_or_else(|| panic!("assigned task-death turn"));
    let commit = store
        .record_provider_turn_task_death(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.execution_id,
        )
        .await
        .unwrap_or_else(|error| panic!("checkpoint provider task death: {error}"));
    assert_eq!(
        commit
            .events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        ["error", "turn_finished", "agent_session_state"]
    );
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read task-death execution: {error}"));
    assert_eq!(execution.phase, crate::ProviderTurnExecutionPhase::Failed);
    assert!(execution.requeue_finalized);
    let session = stored_session(&store).await;
    assert_eq!(session.pending_inputs.len(), 1);
    assert!(session.inflight_inputs.is_empty());
    assert!(session.public.recovery_required);
}

#[tokio::test]
async fn authorized_turn_mute_fences_interrupt_before_finalization() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "started-mute-source",
            "message.send",
            &json!({"content": "@Terra start before mute"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign started mute turn: {error}"));
    let assignment = &mutation.assignments[0];
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start before mute: {error}"));
    store
        .mark_provider_turn_running(&start, "provider-turn-started")
        .await
        .unwrap_or_else(|error| panic!("mark turn running before mute: {error}"));
    let effect = store
        .execute_participant_mute(
            &principal,
            "mute-started-agent",
            &json!({"participant_id": AGENT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("mute started Agent turn: {error}"))
        .interrupt_effect
        .unwrap_or_else(|| panic!("started Agent interrupt effect"));
    assert_eq!(effect.start_dispatch_nonce, start.start_dispatch_nonce);
    let claim = store
        .claim_provider_turn_interrupt(&effect, "10000000-0000-4000-8000-000000000100")
        .await
        .unwrap_or_else(|error| panic!("claim started interrupt: {error}"));
    let dispatched = store
        .authorize_provider_interrupt_dispatch(&claim)
        .await
        .unwrap_or_else(|error| panic!("authorize exact interrupt dispatch: {error}"));
    assert!(!dispatched.dispatch_nonce.is_empty());
    let waiting = store
        .mark_provider_interrupt_issued(&dispatched)
        .await
        .unwrap_or_else(|error| panic!("record exact interrupt issue: {error}"));
    let commit = store
        .finalize_interrupted_turn_retained(&waiting)
        .await
        .unwrap_or_else(|error| panic!("finalize started interrupt: {error}"));
    assert!(commit.next_assignments.is_empty());
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read interrupted execution: {error}"));
    assert_eq!(
        execution.phase,
        crate::ProviderTurnExecutionPhase::Interrupted
    );
    assert!(execution.requeue_finalized);
}

#[tokio::test]
async fn exact_live_control_can_resume_a_quarantined_interrupt_without_reissuing_start() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "recovery-mute-source",
            "message.send",
            &json!({"content": "@Terra retain exact recovery control"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign recovery turn: {error}"));
    let assignment = &mutation.assignments[0];
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize recovery turn: {error}"));
    store
        .mark_provider_turn_running(&start, "provider-turn-recovery")
        .await
        .unwrap_or_else(|error| panic!("mark recovery turn running: {error}"));
    let effect = store
        .execute_participant_mute(
            &principal,
            "mute-recovery-agent",
            &json!({"participant_id": AGENT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("mute recovery Agent: {error}"))
        .interrupt_effect
        .unwrap_or_else(|| panic!("recovery interrupt effect"));
    let claim = store
        .claim_provider_turn_interrupt(&effect, "10000000-0000-4000-8000-000000000101")
        .await
        .unwrap_or_else(|error| panic!("claim recovery interrupt: {error}"));
    store
        .mark_provider_interrupt_recovery_required(&claim.effect)
        .await
        .unwrap_or_else(|error| panic!("quarantine lost control: {error}"));
    let quarantined = store
        .provider_turn_interrupt_effect("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("load quarantined interrupt: {error}"));
    let waiting = store
        .authorize_provider_interrupt_recovery_wait(&quarantined)
        .await
        .unwrap_or_else(|error| panic!("authorize exact recovery wait: {error}"));
    assert_eq!(
        waiting.phase,
        crate::ProviderTurnEffectPhase::IssuedWaitingQuiescence
    );
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read recovery execution: {error}"));
    assert_eq!(
        execution.phase,
        crate::ProviderTurnExecutionPhase::Quiescing
    );
    store
        .finalize_interrupted_turn_retained(&waiting)
        .await
        .unwrap_or_else(|error| panic!("finalize recovered interrupt: {error}"));
}

#[tokio::test]
async fn task_death_after_interrupt_dispatch_preserves_one_ambiguous_effect() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "dispatch-death-source",
            "message.send",
            &json!({"content": "@Terra preserve ambiguous dispatch"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign dispatch-death turn: {error}"));
    let assignment = &mutation.assignments[0];
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize dispatch-death turn: {error}"));
    let effect = store
        .execute_participant_mute(
            &principal,
            "mute-dispatch-death-agent",
            &json!({"participant_id": AGENT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("mute dispatch-death Agent: {error}"))
        .interrupt_effect
        .unwrap_or_else(|| panic!("dispatch-death interrupt effect"));
    let claim = store
        .claim_provider_turn_interrupt(&effect, "10000000-0000-4000-8000-000000000102")
        .await
        .unwrap_or_else(|error| panic!("claim dispatch-death interrupt: {error}"));
    store
        .authorize_provider_interrupt_dispatch(&claim)
        .await
        .unwrap_or_else(|error| panic!("authorize ambiguous dispatch: {error}"));
    store
        .record_provider_turn_task_death(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &start.execution_id,
        )
        .await
        .unwrap_or_else(|error| panic!("checkpoint dispatch task death: {error}"));
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read ambiguous execution: {error}"));
    assert_eq!(
        execution.phase,
        crate::ProviderTurnExecutionPhase::InterruptAmbiguous
    );
    assert!(!execution.requeue_finalized);
    let ambiguous = store
        .provider_turn_interrupt_effect("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read ambiguous effect: {error}"));
    assert_eq!(
        ambiguous.phase,
        crate::ProviderTurnEffectPhase::InterruptAmbiguous
    );
}

#[tokio::test]
async fn blocking_turn_is_reconciled_before_lifecycle_and_exact_gone_detaches() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "gone-source",
            "message.send",
            &json!({"content": "@Terra survive restart"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign runtime-gone turn: {error}"));
    let assignment = &mutation.assignments[0];
    let assigned_page = store
        .load_provider_turn_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("scan pre-dispatch assignment: {error}"));
    let recovered = store
        .recover_assigned_provider_turn(&assigned_page.candidates[0])
        .await
        .unwrap_or_else(|error| panic!("recover immutable pre-dispatch assignment: {error}"));
    assert_eq!(recovered.turn_id, assignment.turn_id);
    assert_eq!(recovered.turn_generation, assignment.turn_generation);
    assert_eq!(recovered.execution_id, assignment.execution_id);
    assert_eq!(recovered.delivery_kind, assignment.delivery_kind);
    assert_eq!(recovered.provider_input, assignment.provider_input);
    assert_eq!(recovered.room_view, assignment.room_view);
    assert_eq!(recovered.room_agent_ids, assignment.room_agent_ids);
    assert_eq!(recovered.tabletop_tools, assignment.tabletop_tools);
    store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize runtime-gone turn: {error}"));
    assert!(
        store
            .load_runtime_reconciliation_candidates()
            .await
            .unwrap_or_else(|error| panic!("scan lifecycle candidates: {error}"))
            .is_empty()
    );
    let page = store
        .load_provider_turn_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("scan provider turn candidates: {error}"));
    assert_eq!(page.candidates.len(), 1);
    let commit = store
        .finalize_provider_turn_runtime_gone(&page.candidates[0])
        .await
        .unwrap_or_else(|error| panic!("finalize exact runtime gone: {error}"));
    assert!(commit.next_assignments.is_empty());
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read runtime-gone execution: {error}"));
    assert_eq!(execution.phase, crate::ProviderTurnExecutionPhase::Failed);
    assert!(execution.requeue_finalized);
    let session = stored_session(&store).await;
    assert_eq!(session.public.status, "detached");
    assert_eq!(session.public.runtime_status, "stopped");
    assert!(session.runtime_handle_id.is_empty());
    assert!(session.provider_session_id.is_empty());
    assert_eq!(session.pending_inputs.len(), 1);
}

#[tokio::test]
async fn runtime_gone_releases_ordered_floor_and_consumes_an_unmute_wake() {
    let (store, principal, _directory) = fixture().await;
    let now = Utc::now();
    let flash_participant = participant(
        SECOND_AGENT_ID,
        "Flash",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    let mut flash_session = attached_session(now);
    SECOND_AGENT_ID.clone_into(&mut flash_session.public.session_id);
    SECOND_AGENT_ID.clone_into(&mut flash_session.public.participant_id);
    "Flash".clone_into(&mut flash_session.public.display_name);
    "provider-thread-2".clone_into(&mut flash_session.provider_session_id);
    "owned-runtime-2".clone_into(&mut flash_session.runtime_handle_id);
    insert_agent(&store, &flash_participant, &flash_session).await;

    let flash_turn = store
        .execute_message_with_turn(
            &principal,
            "flash-floor-source",
            "message.send",
            &json!({"content": "@Flash hold the ordered floor"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign Flash floor: {error}"));
    let flash_assignment = &flash_turn.assignments[0];
    let terra_waiting = store
        .execute_message_with_turn(
            &principal,
            "terra-waiting-source",
            "message.send",
            &json!({"content": "@Terra wait behind Flash"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue Terra behind floor: {error}"));
    assert!(terra_waiting.assignments.is_empty());

    let flash_effect = store
        .execute_participant_mute(
            &principal,
            "mute-flash-floor",
            &json!({"participant_id": SECOND_AGENT_ID, "muted": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("mute Flash floor: {error}"))
        .interrupt_effect
        .unwrap_or_else(|| panic!("Flash floor had no interrupt effect"));
    assert_eq!(flash_effect.execution_id, flash_assignment.execution_id);
    let flash_claim = store
        .claim_provider_turn_interrupt(&flash_effect, "10000000-0000-4000-8000-000000000103")
        .await
        .unwrap_or_else(|error| panic!("claim Flash floor interrupt: {error}"));
    let flash_waiting = store
        .mark_unstarted_interrupt_waiting(&flash_claim)
        .await
        .unwrap_or_else(|error| panic!("wait for unstarted Flash floor: {error}"));
    let terra_started = store
        .finalize_interrupted_turn_retained(&flash_waiting)
        .await
        .unwrap_or_else(|error| panic!("release Flash floor: {error}"));
    assert_eq!(terra_started.next_assignments.len(), 1);
    assert_eq!(
        terra_started.next_assignments[0].session.public.session_id,
        AGENT_ID
    );

    let unmuted = store
        .execute_participant_mute(
            &principal,
            "unmute-flash-waiting",
            &json!({"participant_id": SECOND_AGENT_ID, "muted": false}),
        )
        .await
        .unwrap_or_else(|error| panic!("unmute waiting Flash: {error}"));
    assert!(unmuted.assignments.is_empty());

    let page = store
        .load_provider_turn_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("load Terra floor recovery: {error}"));
    let terra_candidate = page
        .candidates
        .iter()
        .find(|candidate| candidate.execution.session_id == AGENT_ID)
        .unwrap_or_else(|| panic!("missing Terra floor candidate"));
    let resumed = store
        .finalize_provider_turn_runtime_gone(terra_candidate)
        .await
        .unwrap_or_else(|error| panic!("finalize Terra runtime Gone: {error}"));
    assert_eq!(resumed.next_assignments.len(), 1);
    assert_eq!(
        resumed.next_assignments[0].session.public.session_id,
        SECOND_AGENT_ID
    );
    assert!(!resumed.next_assignments[0].session.schedule_requested);
}
