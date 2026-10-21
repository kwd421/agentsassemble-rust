//! A pending delegated confirmation belongs to the owner's decision, not to the
//! AI's turn or connection. These tests pin the reported defect: an app-run
//! (Provider) turn ending, or an attendee connection closing, must not cancel
//! the request. Approval re-proves current authority instead.
use super::*;
use crate::{AttendeeAdmissionRequest, CompanionInviteRequest, RoomSessionAuthorization};
use agentsassemble_domain::{
    AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeStatus, AgentSession,
    AgentSessionStatus, AgentTurnPhase, AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION,
    ConfirmationDecision, ConfirmationState, CreateTextChannel, DecideDelegatedAction,
    DeleteTextChannel, DirectRoomAction, DurableAgentSession, Participant, ParticipantRole,
    ParticipantStatus,
};

const ATTENDEE_ORIGIN: &str = "https://room.example.test";

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PROVIDER_DELEGATE: &str = "provider-delegate-0001";

fn local_owner() -> AuthenticatedPrincipal {
    crate::human_session_authority_tests::local_operator_principal()
}

async fn provider_store() -> Result<
    (
        SqliteStore,
        DelegateRoomAuthority,
        u64,
        String,
        crate::ProviderTurnStartAuthority,
        tempfile::TempDir,
    ),
    Box<dyn std::error::Error>,
> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("live.sqlite")).await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let now = Utc::now();
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: PROVIDER_DELEGATE.to_owned(),
        display_name: "DirectorAI".to_owned(),
        avatar_image_url: String::new(),
        avatar_label: None,
        participant_type: "agent".to_owned(),
        status: ParticipantStatus::Joined,
        role: ParticipantRole::Director,
        owner_id: agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind("general")
    .bind(PROVIDER_DELEGATE)
    .bind(serde_json::to_string(&participant)?)
    .execute(&store.pool)
    .await?;
    let session = DurableAgentSession {
        public: AgentSession {
            avatar_image_url: String::new(),
            room_id: "general".to_owned(),
            session_id: PROVIDER_DELEGATE.to_owned(),
            participant_id: PROVIDER_DELEGATE.to_owned(),
            display_name: "DirectorAI".to_owned(),
            status: AgentSessionStatus::Attached,
            runtime_status: AgentRuntimeStatus::Idle,
            enabled: true,
            provider_kind: "codex_live_session".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            connection_kind: "native_cli_bridge".to_owned(),
            external_owned: false,
            external_retained_interrupt: None,
            execution_os: None,
            process_ownership: "server".to_owned(),
            model: "gpt-5.6-terra".to_owned(),
            reasoning_effort: "medium".to_owned(),
            service_tier: "default".to_owned(),
            variant: String::new(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 0,
            catalog_revision: "catalog-1".to_owned(),
            persona_card_id: Box::default(),
            persona_card: None,
            transport: "stdio_jsonl".to_owned(),
            last_seen_event_id: String::new(),
            last_seen_seq: 0,
            last_provider_sync_event_id: String::new(),
            last_provider_sync_seq: 0,
            bootstrap_cutoff_seq: 0,
            turn_count: 0,
            active_turn_id: String::new(),
            turn_phase: AgentTurnPhase::None,
            last_error: String::new(),
            last_error_code: String::new(),
            recovery_required: false,
            provider_session_active: true,
            provider_session_reused: false,
            created_at: now,
            updated_at: now,
        },
        executable: "/owned/codex".to_owned(),
        executable_identity: "owned-codex-identity".to_owned(),
        workspace: "/owned/workspace".to_owned(),
        workspace_identity: "owned-workspace-identity".to_owned(),
        provider_endpoint: String::new(),
        runtime_profile_key: "profile-1".to_owned(),
        runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
        provider_session_id: "provider-thread-1".to_owned(),
        runtime_handle_id: "owned-runtime-1".to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        runtime_lease_token: "lease-generation-1".to_owned(),
        turn_generation: 0,
        schedule_requested: false,
        pending_inputs: Vec::new(),
        inflight_inputs: Vec::new(),
        active_source_event_id: String::new(),
        input_up_to_event_id: String::new(),
        input_up_to_seq: 0,
        lifecycle_intent_action: AgentLifecycleAction::None,
        lifecycle_intent_id: String::new(),
        lifecycle_intent_status: AgentLifecycleIntentStatus::None,
        trusts_ai_handoffs: false,
    };
    sqlx::query("INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES (?, ?, ?)")
        .bind("general")
        .bind(PROVIDER_DELEGATE)
        .bind(serde_json::to_string(&session)?)
        .execute(&store.pool)
        .await?;
    let local = local_owner();
    let mut tx = store.pool.begin().await?;
    crate::delegation_owner::OwnerBinding::from_mutation(
        &mut tx,
        crate::RoomMutationAuthority::TrustedPrincipal(&local),
    )
    .await?
    .save(&mut tx, "general", "participant", PROVIDER_DELEGATE)
    .await?;
    tx.commit().await?;
    let sent = store
        .execute_message_with_turn(
            &local,
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content": "A room update without a direct handoff."}),
        )
        .await?;
    let assignment = sent
        .assignments
        .into_iter()
        .find(|assignment| assignment.session.public.session_id == PROVIDER_DELEGATE)
        .ok_or("provider delegate was not assigned a turn")?;
    let start = store
        .authorize_provider_turn_start(
            "general",
            PROVIDER_DELEGATE,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await?;
    store
        .mark_provider_turn_running(&start, "provider-turn-test-1")
        .await?;
    let authority = DelegateRoomAuthority::Provider {
        room_id: "general".into(),
        session_id: PROVIDER_DELEGATE.into(),
        turn_id: assignment.turn_id,
        input_up_to_seq: assignment.session.input_up_to_seq,
        turn_generation: assignment.turn_generation,
        execution_id: assignment.execution_id.clone(),
    };
    Ok((
        store,
        authority,
        assignment.turn_generation,
        assignment.execution_id,
        start,
        directory,
    ))
}

async fn pending_channel_delete(
    store: &SqliteStore,
    authority: &DelegateRoomAuthority,
) -> Result<(String, DeleteTextChannel), Box<dyn std::error::Error>> {
    let created = store
        .execute_delegated_atomic(
            authority,
            &DirectRoomAction::CreateTextChannel(CreateTextChannel {
                request_id: Uuid::new_v4().to_string(),
                name: "Delete me".into(),
            }),
            |_, _, _| Ok(()),
        )
        .await?
        .0;
    let request = DeleteTextChannel {
        request_id: Uuid::new_v4().to_string(),
        channel_id: created.outcome.result["target_id"]
            .as_str()
            .ok_or("channel")?
            .into(),
        expected_settings_revision: None,
    };
    let pending = store
        .request_delegated_channel_delete::<()>(authority, &request, |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let confirmation_id = pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned();
    Ok((confirmation_id, request))
}

fn approve(confirmation_id: &str) -> DecideDelegatedAction {
    DecideDelegatedAction {
        confirmation_id: confirmation_id.into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    }
}

#[tokio::test]
async fn provider_pending_survives_turn_end_and_owner_approval_executes() -> TestResult {
    let (store, authority, generation, execution_id, _start, _directory) = provider_store().await?;
    let local = local_owner();
    let (confirmation_id, _) = pending_channel_delete(&store, &authority).await?;
    // The exact turn-end call from the report: it ends provider approval requests
    // but must leave the delegated confirmation alone.
    store
        .cancel_provider_execution_requests("general", PROVIDER_DELEGATE, generation, &execution_id)
        .await?;
    store.reconcile_delegated_confirmations("general").await?;
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(view.delegated_action_requests.len(), 1);
    assert!(matches!(
        view.delegated_action_requests[0].state,
        ConfirmationState::Pending
    ));
    assert_eq!(view.settings.channels.len(), 1);
    let ack = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &approve(&confirmation_id),
        )
        .await?;
    assert_eq!(ack.result["state"], "succeeded");
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert!(view.settings.channels.is_empty());
    assert!(
        ack.events.iter().any(|event| event
            .content
            .as_deref()
            .is_some_and(|content| content.contains("on behalf of Host"))),
        "approval must leave the attributed system line for the AI"
    );
    Ok(())
}

#[tokio::test]
async fn provider_director_loss_before_approval_cancels_without_effects() -> TestResult {
    let (store, authority, _, _, _start, _directory) = provider_store().await?;
    let local = local_owner();
    let (confirmation_id, _) = pending_channel_delete(&store, &authority).await?;
    store
        .execute_participant_role_update(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id": PROVIDER_DELEGATE, "role": "agent"}),
        )
        .await?;
    let ack = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &approve(&confirmation_id),
        )
        .await?;
    assert_eq!(ack.result["receipt"]["code"], "confirmation_cancelled");
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(view.settings.channels.len(), 1);
    Ok(())
}

async fn attendee_store() -> Result<
    (
        SqliteStore,
        DelegateRoomAuthority,
        crate::AttendeeConnectionAuthorization,
        tempfile::TempDir,
    ),
    Box<dyn std::error::Error>,
> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("live.sqlite")).await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let now = Utc::now();
    let manager = crate::room_user_identity::test_authority(&store).await;
    store
        .create_operator_pairing(
            &crate::RoomManagerAuthority::Local(manager),
            &[21; 32],
            ATTENDEE_ORIGIN,
            now,
        )
        .await?;
    let paired = store
        .redeem_operator_pairing(&[21; 32], &[22; 32], ATTENDEE_ORIGIN, now)
        .await?;
    let issuer = RoomSessionAuthorization::Operator(
        store
            .authorize_operator_session(
                paired.authorization.session_fingerprint(),
                &[22; 32],
                ATTENDEE_ORIGIN,
            )
            .await?,
    );
    let invite = store
        .create_companion_attendee_invite(
            &issuer,
            CompanionInviteRequest {
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session".into(),
                display_name: "Companion".into(),
            },
            now,
        )
        .await?;
    let fingerprint: [u8; 32] = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let session = store
        .admit_attendee(
            AttendeeAdmissionRequest {
                invite_fingerprint: &fingerprint,
                client_fingerprint: &[1; 32],
                request_id: Uuid::new_v4(),
                provider_kind: "codex_live_session".into(),
                display_name: "Companion".into(),
            },
            now,
        )
        .await?
        .authorization;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &crate::attendee::ready_tests::report(), now)
        .await?;
    let local = local_owner();
    store
        .execute_message_with_turn(
            &local,
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content": "Companion, please tidy up."}),
        )
        .await?;
    let turn = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("attendee turn missing")?;
    store
        .execute_participant_role_update(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id": session.principal().participant_id, "role": "director"}),
        )
        .await?;
    let authority = DelegateRoomAuthority::Attendee {
        session,
        connection_id: connection.connection_id(),
        turn_generation: turn.authority.turn_generation,
        execution_id: turn.authority.execution_id,
    };
    Ok((store, authority, connection, directory))
}

#[tokio::test]
async fn attendee_pending_survives_connection_end_and_owner_approval_executes() -> TestResult {
    let (store, authority, connection, _directory) = attendee_store().await?;
    let local = local_owner();
    let (confirmation_id, _) = pending_channel_delete(&store, &authority).await?;
    store
        .disconnect_attendee_connection(&connection, Utc::now())
        .await?;
    store.reconcile_delegated_confirmations("general").await?;
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(view.delegated_action_requests.len(), 1);
    assert!(matches!(
        view.delegated_action_requests[0].state,
        ConfirmationState::Pending
    ));
    assert_eq!(view.settings.channels.len(), 1);
    let ack = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &approve(&confirmation_id),
        )
        .await?;
    assert_eq!(ack.result["state"], "succeeded");
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert!(view.settings.channels.is_empty());
    assert!(
        ack.events.iter().any(|event| event
            .content
            .as_deref()
            .is_some_and(|content| content.contains("on behalf of Host"))),
        "approval must leave the attributed system line for the AI"
    );
    Ok(())
}

#[tokio::test]
async fn attendee_director_loss_before_approval_cancels_without_effects() -> TestResult {
    let (store, authority, _, _directory) = attendee_store().await?;
    let local = local_owner();
    let (confirmation_id, _) = pending_channel_delete(&store, &authority).await?;
    let delegate_id = authority.delegate_id().to_owned();
    store
        .execute_participant_role_update(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id": delegate_id, "role": "agent"}),
        )
        .await?;
    let ack = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &approve(&confirmation_id),
        )
        .await?;
    assert_eq!(ack.result["receipt"]["code"], "confirmation_cancelled");
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(view.settings.channels.len(), 1);
    Ok(())
}

// A pending owner confirmation belongs to the owner's decision, not to the
// delegate's runtime: the bridge exiting after the requesting turn detaches the
// participant (membership intact), and that churn must not cancel the request.
#[tokio::test]
async fn provider_pending_survives_runtime_detach_and_owner_approval_executes() -> TestResult {
    let (store, authority, _generation, _execution_id, start, _directory) =
        provider_store().await?;
    // Workspace-write session like the report; the request itself goes through
    // the real turn authority.
    sqlx::query(
        "UPDATE agent_sessions SET session_json = json_set(session_json, '$.permission_mode', 'workspace-write') WHERE room_id='general' AND session_id=?",
    )
    .bind(PROVIDER_DELEGATE)
    .execute(&store.pool)
    .await?;
    let (confirmation_id, _) = pending_channel_delete(&store, &authority).await?;
    // The real turn-completion transition the runtime performs.
    let completion_authority = crate::ProviderTurnAuthority {
        room_id: &start.room_id,
        session_id: &start.session_id,
        turn_id: &start.turn_id,
        turn_generation: start.turn_generation,
        execution_id: &start.execution_id,
        start_dispatch_nonce: &start.start_dispatch_nonce,
        runtime_handle_id: &start.runtime_handle_id,
        runtime_owner_id: &start.runtime_owner_id,
        runtime_lease_token: &start.runtime_lease_token,
        provider_turn_id: "provider-turn-test-1",
        provider_session_id: Some("provider-thread-1"),
    };
    store
        .complete_agent_turn(
            "general",
            PROVIDER_DELEGATE,
            completion_authority,
            "Finished the requested work.",
            "",
            None,
            &[],
        )
        .await?;
    // The managed bridge ending while idle detaches the participant through the
    // real runtime-failure path; the delegate keeps role, mute state and owner.
    let candidate = store
        .load_runtime_reconciliation_candidate("general", PROVIDER_DELEGATE)
        .await?
        .ok_or("no reconciliation candidate for the idle session")?;
    store.record_idle_runtime_failure(&candidate).await?;
    let participant: Participant = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id='general' AND participant_id=?",
        )
        .bind(PROVIDER_DELEGATE)
        .fetch_one(&store.pool)
        .await?,
    )?;
    assert_eq!(participant.status, ParticipantStatus::Detached);
    assert_eq!(participant.role, ParticipantRole::Director);
    assert!(!participant.muted);
    // Room-input reconciliation must keep the confirmation pending.
    store.reconcile_delegated_confirmations("general").await?;
    let local = local_owner();
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(view.delegated_action_requests.len(), 1);
    assert!(matches!(
        view.delegated_action_requests[0].state,
        ConfirmationState::Pending
    ));
    // The owner can still approve while the delegate is detached: approval runs
    // under the owner's own authority, not the delegate's runtime.
    let ack = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &approve(&confirmation_id),
        )
        .await?;
    assert_eq!(ack.result["state"], "succeeded");
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert!(view.settings.channels.is_empty());
    Ok(())
}

// Real authority loss still cancels in reconciliation, and the stored receipt
// keeps the underlying cause next to `confirmation_cancelled`.
#[tokio::test]
async fn detached_reconciliation_still_cancels_real_authority_loss_with_cause() -> TestResult {
    let (store, authority, _generation, _execution_id, start, _directory) =
        provider_store().await?;
    let (confirmation_id, _) = pending_channel_delete(&store, &authority).await?;
    let completion_authority = crate::ProviderTurnAuthority {
        room_id: &start.room_id,
        session_id: &start.session_id,
        turn_id: &start.turn_id,
        turn_generation: start.turn_generation,
        execution_id: &start.execution_id,
        start_dispatch_nonce: &start.start_dispatch_nonce,
        runtime_handle_id: &start.runtime_handle_id,
        runtime_owner_id: &start.runtime_owner_id,
        runtime_lease_token: &start.runtime_lease_token,
        provider_turn_id: "provider-turn-test-1",
        provider_session_id: Some("provider-thread-1"),
    };
    store
        .complete_agent_turn(
            "general",
            PROVIDER_DELEGATE,
            completion_authority,
            "Finished the requested work.",
            "",
            None,
            &[],
        )
        .await?;
    let candidate = store
        .load_runtime_reconciliation_candidate("general", PROVIDER_DELEGATE)
        .await?
        .ok_or("no reconciliation candidate for the idle session")?;
    store.record_idle_runtime_failure(&candidate).await?;
    store.reconcile_delegated_confirmations("general").await?;
    let local = local_owner();
    let view = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(view.delegated_action_requests.len(), 1);
    assert!(matches!(
        view.delegated_action_requests[0].state,
        ConfirmationState::Pending
    ));
    // Losing the director role is real authority loss: the next
    // reconciliation cancels, and the receipt explains itself.
    store
        .execute_participant_role_update(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id": PROVIDER_DELEGATE, "role": "agent"}),
        )
        .await?;
    store.reconcile_delegated_confirmations("general").await?;
    let mut tx = store.pool.begin().await?;
    let row =
        crate::delegated_confirmation_lifecycle::load_in(&mut tx, "general", &confirmation_id)
            .await?;
    let receipt = crate::delegated_confirmation_lifecycle::receipt(&row)?;
    assert_eq!(receipt["code"], "confirmation_cancelled");
    assert_eq!(receipt["cause"]["code"], "role_required");
    tx.rollback().await?;
    Ok(())
}

// A confirmed start_agent belongs to the owner's decision, not the requesting
// runtime: detaching the provider delegate (membership intact) must not cancel
// approval. Uses the same shared principal gate as the other confirmed actions.
#[tokio::test]
async fn provider_detached_start_agent_approval_executes() -> TestResult {
    let (store, authority, _generation, _execution_id, start, _directory) =
        provider_store().await?;
    sqlx::query(
        "UPDATE agent_sessions SET session_json = json_set(session_json, '$.permission_mode', 'workspace_write') WHERE room_id='general' AND session_id=?",
    )
    .bind(PROVIDER_DELEGATE)
    .execute(&store.pool)
    .await?;
    let request = agentsassemble_domain::StartAgent {
        request_id: Uuid::new_v4().to_string(),
        agent_id: PROVIDER_DELEGATE.into(),
    };
    let pending = store
        .request_delegated_agent_start(&authority, &request, |_, _, _| Ok(()))
        .await?
        .ok_or("confirmation missing")?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let confirmation_id = pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned();
    let completion_authority = crate::ProviderTurnAuthority {
        room_id: &start.room_id,
        session_id: &start.session_id,
        turn_id: &start.turn_id,
        turn_generation: start.turn_generation,
        execution_id: &start.execution_id,
        start_dispatch_nonce: &start.start_dispatch_nonce,
        runtime_handle_id: &start.runtime_handle_id,
        runtime_owner_id: &start.runtime_owner_id,
        runtime_lease_token: &start.runtime_lease_token,
        provider_turn_id: "provider-turn-test-1",
        provider_session_id: Some("provider-thread-1"),
    };
    store
        .complete_agent_turn(
            "general",
            PROVIDER_DELEGATE,
            completion_authority,
            "Finished the requested work.",
            "",
            None,
            &[],
        )
        .await?;
    let candidate = store
        .load_runtime_reconciliation_candidate("general", PROVIDER_DELEGATE)
        .await?
        .ok_or("no reconciliation candidate for the idle session")?;
    store.record_idle_runtime_failure(&candidate).await?;
    let participant: Participant = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id='general' AND participant_id=?",
        )
        .bind(PROVIDER_DELEGATE)
        .fetch_one(&store.pool)
        .await?,
    )?;
    assert_eq!(participant.status, ParticipantStatus::Detached);
    let local = local_owner();
    let ack = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &approve(&confirmation_id),
        )
        .await?;
    assert_eq!(
        ack.result["state"], "executing",
        "detached delegate start_agent approval must not cancel: {}",
        ack.result
    );
    Ok(())
}
