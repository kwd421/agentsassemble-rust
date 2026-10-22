//! The exact human decides; the transaction claims and executes the stored channel delta.
use crate::delegated_confirmation::{rejected, settings_in};
use crate::delegated_confirmation_lifecycle::{finish_in, item, load_in, receipt, validate_in};
use crate::{
    CommandOutcome, PersistenceError, RoomMutationAuthority, SqliteStore,
    delegated_confirmed_invite::InviteApprover,
};
use agentsassemble_domain::{
    ClientKind, ConfirmationDecision, ConfirmationState, ConfirmedRoomAction,
    DecideDelegatedAction, DelegatedActionRequest, DeleteTextChannel,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

impl SqliteStore {
    /// Claims one immutable request and executes through the human settings gate.
    /// # Errors
    /// Rejects non-owners and conflicting decisions; action failures become terminal receipts.
    pub async fn decide_delegated_action(
        &self,
        authority: RoomMutationAuthority<'_>,
        decision: &DecideDelegatedAction,
    ) -> Result<CommandOutcome, PersistenceError> {
        Ok(self
            .decide_delegated_action_inner(authority, decision)
            .await?
            .0)
    }

    /// The same decision, also returning the live human sessions it revoked so the room
    /// actor closes them exactly as it closes the owner's own kick.
    /// # Errors
    /// Same as `decide_delegated_action`.
    pub async fn decide_delegated_action_with_revocations(
        &self,
        authority: RoomMutationAuthority<'_>,
        decision: &DecideDelegatedAction,
    ) -> Result<(CommandOutcome, Vec<[u8; 32]>), PersistenceError> {
        self.decide_delegated_action_inner(authority, decision)
            .await
    }

    async fn decide_delegated_action_inner(
        &self,
        authority: RoomMutationAuthority<'_>,
        decision: &DecideDelegatedAction,
    ) -> Result<(CommandOutcome, Vec<[u8; 32]>), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let principal = authority.resolve(&mut tx).await?.into_owned();
        let row = load_in(&mut tx, &principal.room_id, &decision.confirmation_id).await?;
        let request = item(&row)?;
        let (_, human) = crate::authority::load_active_membership(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        if principal.client_kind != ClientKind::Browser
            || human.participant_type != "human"
            || principal.participant_id != request.owner_participant_id
        {
            return Err(rejected(
                "permission_denied",
                "Only the exact human owner may decide.",
            ));
        }
        if decision.decision == ConfirmationDecision::Approve
            && requires_full_access_acknowledgement(&request.action)
            && decision.full_access_acknowledged != Some(true)
        {
            return Err(rejected(
                "full_access_acknowledgement_required",
                "Approving this full-access agent requires the owner's explicit acknowledgement.",
            ));
        }
        // An invite approval additionally requires the deciding session itself to
        // hold owner-route invite authority right now; the recorded session is the
        // only authority the later mint may use.
        let invite_approver = if decision.decision == ConfirmationDecision::Approve
            && matches!(request.action, ConfirmedRoomAction::CreateHumanInvite(_))
        {
            Some(require_invite_approval_authority(&mut tx, authority).await?)
        } else {
            None
        };
        let encoded = match &invite_approver {
            Some(approver) => {
                serde_json::to_string(&crate::delegated_confirmed_invite::RecordedInviteDecision {
                    decision: decision.clone(),
                    approver: approver.clone(),
                })?
            }
            None => serde_json::to_string(decision)?,
        };
        let first_seq =
            crate::room_event_sequence::next_sequence(&mut tx, &principal.room_id).await?;
        let mut revoked: Vec<[u8; 32]> = Vec::new();
        let deduplicated = if let Some(previous) = row.get::<Option<String>, _>("decision_json") {
            let same = match &invite_approver {
                Some(_) => crate::delegated_confirmed_invite::recorded_invite_decision_matches(
                    &previous, decision,
                ),
                None => previous == encoded,
            };
            if !same {
                return Err(rejected(
                    "request_conflict",
                    "This confirmation already has another decision.",
                ));
            }
            true
        } else {
            sqlx::query("UPDATE delegated_action_requests SET decision_json=? WHERE room_id=? AND confirmation_id=?")
                .bind(encoded).bind(&principal.room_id).bind(&decision.confirmation_id).execute(&mut *tx).await?;
            if row.get::<&str, _>("state") == "pending"
                && row.get::<i64, _>("expires_at") <= chrono::Utc::now().timestamp_millis()
            {
                finish_in(
                    &mut tx,
                    &principal.room_id,
                    &row,
                    ConfirmationState::Expired,
                    &json!({"status":"rejected","code":"confirmation_expired","retryable":false}),
                )
                .await?;
            }
            let current = load_in(&mut tx, &principal.room_id, &decision.confirmation_id).await?;
            if current.get::<&str, _>("state") == "pending" {
                if decision.decision == ConfirmationDecision::Decline {
                    finish_in(&mut tx, &principal.room_id, &row, ConfirmationState::Declined, &json!({"status":"rejected","code":"confirmation_declined","retryable":false})).await?;
                } else {
                    revoked = self
                        .run_approved(&mut tx, &principal.room_id, &row, &request, first_seq)
                        .await?;
                }
            }
            false
        };
        let current = load_in(&mut tx, &principal.room_id, &decision.confirmation_id).await?;
        let result = serde_json::to_value(agentsassemble_domain::DelegatedActionDecisionAck {
            confirmation_id: decision.confirmation_id.clone(),
            state: item(&current)?.state,
            receipt: receipt(&current)?,
            invite: None,
        })?;
        let events: Vec<String> = sqlx::query_scalar(
            "SELECT event_json FROM room_events WHERE room_id=? AND seq>=? ORDER BY seq",
        )
        .bind(&principal.room_id)
        .bind(first_seq)
        .fetch_all(&mut *tx)
        .await?;
        let mut events = events
            .into_iter()
            .map(|e| serde_json::from_str(&e))
            .collect::<Result<Vec<agentsassemble_domain::RoomEvent>, _>>()?;
        // Repeats return the recorded ACK, without republishing any events.
        let event = if let Some(event) = events.last() {
            event.clone()
        } else {
            let encoded: String = sqlx::query_scalar("SELECT event_json FROM room_events WHERE room_id=? AND json_extract(event_json,'$.delegated_action_request.confirmation_id')=? ORDER BY seq DESC LIMIT 1")
                .bind(&principal.room_id).bind(&decision.confirmation_id).fetch_one(&mut *tx).await?;
            serde_json::from_str(&encoded)?
        };
        authority.record_success(&mut tx).await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        if deduplicated {
            events.clear();
        }
        Ok((
            CommandOutcome {
                result,
                event,
                events,
                deduplicated,
            },
            revoked,
        ))
    }

    /// Runs the approved action inside a savepoint. An action refusal rolls the
    /// action back and ends the request with a terminal receipt instead of an error.
    async fn run_approved(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        room_id: &str,
        row: &SqliteRow,
        request: &DelegatedActionRequest,
        first_seq: i64,
    ) -> Result<Vec<[u8; 32]>, PersistenceError> {
        sqlx::query("SAVEPOINT confirmed_action")
            .execute(&mut **tx)
            .await?;
        match self
            .execute_approved(tx, room_id, row, request, first_seq)
            .await
        {
            Ok((result, revoked)) => {
                sqlx::query("RELEASE confirmed_action")
                    .execute(&mut **tx)
                    .await?;
                let state = if result["status"] == "in_progress" {
                    ConfirmationState::Executing
                } else {
                    ConfirmationState::Succeeded
                };
                finish_in(tx, room_id, row, state, &result).await?;
                Ok(revoked)
            }
            Err(PersistenceError::CommandRejected { code, message }) => {
                sqlx::query("ROLLBACK TO confirmed_action")
                    .execute(&mut **tx)
                    .await?;
                sqlx::query("RELEASE confirmed_action")
                    .execute(&mut **tx)
                    .await?;
                let ended = matches!(
                    code.as_bytes(),
                    b"role_required"
                        | b"owner_muted"
                        | b"owner_not_permitted"
                        | b"owner_authority_unavailable"
                        | b"owner_scope_read_only"
                        | b"delegate_authority_expired"
                        | b"stale_provider_turn"
                        | b"session_revoked"
                        | b"bridge_connection_replaced"
                );
                let (state, code) = if ended {
                    (
                        ConfirmationState::Cancelled,
                        std::borrow::Cow::Borrowed("confirmation_cancelled"),
                    )
                } else {
                    (ConfirmationState::Failed, code)
                };
                finish_in(
                    tx,
                    room_id,
                    row,
                    state,
                    &json!({"status":"rejected","code":code,"message":message,"retryable":false}),
                )
                .await?;
                Ok(Vec::new())
            }
            Err(error) => Err(error),
        }
    }

    /// Executes one approved action and returns its receipt and any human
    /// sessions it revoked.
    async fn execute_approved(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        room_id: &str,
        row: &SqliteRow,
        request: &DelegatedActionRequest,
        first_seq: i64,
    ) -> Result<(Value, Vec<[u8; 32]>), PersistenceError> {
        let result = match &request.action {
            ConfirmedRoomAction::DeleteTextChannel(action) => {
                delete_text_channel_in(tx, room_id, row, request, action, first_seq).await?
            }
            ConfirmedRoomAction::StartAgent(_) => {
                mark_executing(tx, room_id, request).await?;
                let row = load_in(tx, room_id, &request.confirmation_id).await?;
                let control =
                    crate::delegated_confirmed_agent::control_in(tx, room_id, &row).await?;
                let plan = self
                    .prepare_agent_launch_in(
                        tx,
                        crate::AgentControlAuthority::Delegated(&control),
                        control.request_id(),
                        control.payload(),
                        "agent.start",
                    )
                    .await?;
                match plan {
                    crate::AgentStartPlan::Start(effect) => {
                        json!({"status":"in_progress","request_id":control.request_id(),"action":"start_agent","target_id":control.payload()["agent_id"],"confirmation_id":request.confirmation_id,"operation_id":effect.operation_id})
                    }
                    crate::AgentStartPlan::Outcome(outcome) => outcome.result,
                }
            }
            ConfirmedRoomAction::CreateAgent(create) => {
                // Both authorities are revalidated now; the room actor
                // performs the catalog-selected creation after commit.
                let proof = validate_in(tx, room_id, row).await?;
                crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
                mark_executing(tx, room_id, request).await?;
                json!({"status":"in_progress","request_id":create.request_id,"action":"create_agent","target_id":create.draft.agent_id,"confirmation_id":request.confirmation_id})
            }
            ConfirmedRoomAction::ReconfigureAgent(reconfigure) => {
                // Both authorities are revalidated now; the room actor
                // re-resolves the catalog and runs the configuration core
                // after commit, binding the target's configuration revision.
                let proof = validate_in(tx, room_id, row).await?;
                crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
                mark_executing(tx, room_id, request).await?;
                json!({"status":"in_progress","request_id":reconfigure.request_id,"action":"reconfigure_agent","target_id":reconfigure.agent_id,"confirmation_id":request.confirmation_id})
            }
            ConfirmedRoomAction::RemoveAgent(remove) => {
                // Both authorities are revalidated now, then the owner's own
                // removal core runs in this transaction with the target's
                // creation identity re-bound.
                let proof = validate_in(tx, room_id, row).await?;
                crate::delegated_confirmed_remove::remove_agent_in(
                    tx, &proof, request, remove, first_seq,
                )
                .await?
            }
            ConfirmedRoomAction::KickParticipant(kick) => {
                // Both authorities are revalidated now, then the owner's own
                // removal core runs in this transaction with the target's
                // admission identity re-bound.
                let proof = validate_in(tx, room_id, row).await?;
                return crate::delegated_confirmed_kick::kick_participant_in(
                    tx, &proof, request, kick, first_seq,
                )
                .await;
            }
            ConfirmedRoomAction::CreateHumanInvite(invite) => {
                // Both authorities are revalidated now and the owner's
                // provenance must resolve to the owner's own room-manager
                // authority; the server actor mints the credential after
                // commit and returns the link only in this acknowledgement.
                let proof = validate_in(tx, room_id, row).await?;
                crate::delegation_owner::manager_authority_for_delegate(
                    tx,
                    room_id,
                    &proof.delegate.participant_id,
                )
                .await?;
                mark_executing(tx, room_id, request).await?;
                json!({"status":"in_progress","request_id":invite.request_id,"action":"create_human_invite","confirmation_id":request.confirmation_id})
            }
        };
        Ok((result, Vec::new()))
    }
}

fn requires_full_access_acknowledgement(action: &ConfirmedRoomAction) -> bool {
    match action {
        ConfirmedRoomAction::StartAgent(start) => {
            start.configuration.permission_mode == "full_access"
        }
        ConfirmedRoomAction::CreateAgent(create) => create.draft.permission_mode == "full_access",
        ConfirmedRoomAction::ReconfigureAgent(reconfigure) => {
            reconfigure.draft.permission_mode == "full_access"
        }
        ConfirmedRoomAction::RemoveAgent(_)
        | ConfirmedRoomAction::DeleteTextChannel(_)
        | ConfirmedRoomAction::KickParticipant(_)
        | ConfirmedRoomAction::CreateHumanInvite(_) => false,
    }
}

/// Proves the deciding session could create a human invite through the owner's own
/// route right now, and records how the mint must re-resolve it. The desktop's own
/// local session (local socket provenance, never a participant id) proves current
/// local-bootstrap lineage; a paired operator browser proves a live central-owner
/// session with current manager binding. Any other deciding session cannot approve
/// an invite, while decline stays allowed for every session.
/// # Errors
/// Refuses `permission_denied` for any session without current owner-route invite
/// authority; storage failures propagate.
async fn require_invite_approval_authority(
    tx: &mut Transaction<'_, Sqlite>,
    authority: RoomMutationAuthority<'_>,
) -> Result<InviteApprover, PersistenceError> {
    let denied = || {
        rejected(
            "permission_denied",
            "Only a session that could create a human invite itself may approve one.",
        )
    };
    match authority {
        RoomMutationAuthority::TrustedPrincipal(principal) => {
            let (binding, _) = crate::room_user_identity::resolve_local_room_manager(
                tx,
                &principal.room_id,
                &principal.principal_id,
                &principal.participant_id,
            )
            .await
            .map_err(|error| match error {
                PersistenceError::CommandRejected { .. } => denied(),
                other => other,
            })?;
            Ok(InviteApprover::Local {
                server_id: binding.server_id.clone(),
                authority_lineage_id: binding.authority_lineage_id.clone(),
                room_uid: binding.room_uid,
                user_id: binding.manager.user_id.clone(),
                participant_id: binding.manager.participant_id.clone(),
            })
        }
        RoomMutationAuthority::OperatorSession(session) => {
            if !session.is_central_owner() {
                return Err(denied());
            }
            let current = crate::operator_pairing::resolve_operator_session(
                tx,
                session.session_fingerprint(),
                session.delegation_device(),
                session.delegation_origin(),
                Utc::now(),
            )
            .await
            .map_err(|error| match error {
                PersistenceError::CommandRejected { .. } => denied(),
                other => other,
            })?;
            if !current.is_central_owner() {
                return Err(denied());
            }
            crate::RoomManagerAuthority::Operator(Box::new(current.clone()))
                .exact_binding(tx)
                .await
                .map_err(|error| match error {
                    PersistenceError::CommandRejected { .. } => denied(),
                    other => other,
                })?;
            Ok(InviteApprover::Operator {
                fingerprint: *session.session_fingerprint(),
                device: *session.delegation_device(),
                origin: session.delegation_origin().to_owned(),
                user_id: current.principal().principal_id.clone(),
                participant_id: current.principal().participant_id.clone(),
            })
        }
        RoomMutationAuthority::HumanSession(_)
        | RoomMutationAuthority::ConnectorSession(_)
        | RoomMutationAuthority::ServerOwnerLifecycle(_) => Err(denied()),
    }
}

/// Records that the approved action is now executing; the room actor finishes it.
async fn mark_executing(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    request: &DelegatedActionRequest,
) -> Result<(), PersistenceError> {
    let mut executing = request.clone();
    executing.state = ConfirmationState::Executing;
    sqlx::query("UPDATE delegated_action_requests SET state='executing',request_json=? WHERE room_id=? AND confirmation_id=?")
        .bind(serde_json::to_string(&executing)?)
        .bind(room_id)
        .bind(&request.confirmation_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Deletes the reviewed text channel through the owner's settings gate.
async fn delete_text_channel_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    row: &SqliteRow,
    request: &DelegatedActionRequest,
    action: &DeleteTextChannel,
    first_seq: i64,
) -> Result<Value, PersistenceError> {
    let proof = validate_in(tx, room_id, row).await?;
    let mut settings = settings_in(tx, room_id).await?;
    let current_revision = agentsassemble_domain::public_settings(&settings)?.settings_revision;
    // Only this channel may refuse approval: a deleted target is a changed
    // target, and a renamed one no longer matches the card the owner reviewed.
    // Any other room change leaves this approval alone.
    if let Err(error) =
        crate::room_channels::require_text_channel(tx, room_id, &action.channel_id).await
    {
        if matches!(&error, PersistenceError::CommandRejected { code, .. } if code == "channel_not_found")
        {
            return Err(rejected(
                "target_changed",
                "The text channel no longer exists.",
            ));
        }
        return Err(error);
    }
    let target = settings
        .channels
        .iter()
        .find(|channel| channel.id == action.channel_id)
        .ok_or_else(|| rejected("target_changed", "The text channel no longer exists."))?;
    if target.name != request.target_name {
        return Err(rejected(
            "target_changed",
            "The text channel changed before approval.",
        ));
    }
    settings.channels.retain(|c| c.id != action.channel_id);
    for (position, channel) in settings.channels.iter_mut().enumerate() {
        channel.position = u32::try_from(position).unwrap_or(u32::MAX);
    }
    let delta = json!({"expected_revision":current_revision,"channels":settings.channels});
    let mut outcome = crate::room_settings::update_settings_in(tx, &proof.owner, &delta).await?;
    let mut event = crate::delegated_pin::attributed_action_event(
        tx,
        proof.attribution(),
        &action.request_id,
        "delete_text_channel",
        &action.channel_id,
        "deleted a text channel and its messages",
    )
    .await?;
    event
        .extra
        .insert("confirmation_id".into(), json!(request.confirmation_id));
    crate::room_turns::support::insert_event(tx, &event).await?;
    outcome.events.push(event.clone());
    crate::delegated_event_attribution::attribute_since(
        tx,
        &proof.delegate,
        &proof.owner.participant_id,
        Some(&request.confirmation_id),
        first_seq,
        &mut outcome.events,
    )
    .await?;
    for canonical in &mut outcome.events {
        for key in ["action", "target_id", "request_id", "confirmation_id"] {
            canonical.extra.insert(key.into(), event.extra[key].clone());
        }
        sqlx::query("UPDATE room_events SET event_json=? WHERE room_id=? AND seq=?")
            .bind(serde_json::to_string(canonical)?)
            .bind(room_id)
            .bind(canonical.seq)
            .execute(&mut **tx)
            .await?;
    }
    Ok(
        json!({"status":"succeeded","request_id":action.request_id,"action":"delete_text_channel","target_id":action.channel_id,"confirmation_id":request.confirmation_id,"event_id":event.id}),
    )
}
