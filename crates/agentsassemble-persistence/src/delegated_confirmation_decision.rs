//! The exact human decides; the transaction claims and executes the stored channel delta.
use crate::delegated_confirmation::{rejected, settings_in};
use crate::delegated_confirmation_lifecycle::{finish_in, item, load_in, receipt, validate_in};
use crate::{CommandOutcome, PersistenceError, RoomMutationAuthority, SqliteStore};
use agentsassemble_domain::{
    ClientKind, ConfirmationDecision, ConfirmationState, ConfirmedRoomAction, DecideDelegatedAction,
};
use serde_json::{Value, json};
use sqlx::Row;

impl SqliteStore {
    /// Claims one immutable request and executes through the human settings gate.
    /// # Errors
    /// Rejects non-owners and conflicting decisions; action failures become terminal receipts.
    pub async fn decide_delegated_action(
        &self,
        authority: RoomMutationAuthority<'_>,
        decision: &DecideDelegatedAction,
    ) -> Result<CommandOutcome, PersistenceError> {
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
        let encoded = serde_json::to_string(decision)?;
        let first_seq =
            crate::room_event_sequence::next_sequence(&mut tx, &principal.room_id).await?;
        let deduplicated = if let Some(previous) = row.get::<Option<String>, _>("decision_json") {
            if previous != encoded {
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
                    sqlx::query("SAVEPOINT confirmed_action")
                        .execute(&mut *tx)
                        .await?;
                    let execution = async {
                        match &request.action {
                            ConfirmedRoomAction::DeleteTextChannel(action) => {
                        let proof = validate_in(&mut tx, &principal.room_id, &row).await?;
                        let mut settings = settings_in(&mut tx, &principal.room_id).await?;
                        if agentsassemble_domain::public_settings(&settings)?.settings_revision != action.expected_settings_revision {
                            return Err(rejected("target_changed", "The target settings changed before approval."));
                        }
                        crate::room_channels::require_text_channel(&mut tx, &principal.room_id, &action.channel_id).await?;
                        settings.channels.retain(|c| c.id != action.channel_id);
                        let delta = json!({"expected_revision":action.expected_settings_revision,"channels":settings.channels});
                        let mut outcome = crate::room_settings::update_settings_in(&mut tx, &proof.owner, &delta).await?;
                        let mut event = crate::delegated_pin::attributed_action_event(&mut tx, proof.attribution(), &action.request_id, "delete_text_channel", &action.channel_id, "deleted a text channel and its messages").await?;
                        event.extra.insert("confirmation_id".into(), json!(request.confirmation_id));
                        crate::room_turns::support::insert_event(&mut tx, &event).await?;
                        outcome.events.push(event.clone());
                        crate::delegated_event_attribution::attribute_since(&mut tx, &proof.delegate, &proof.owner.participant_id, Some(&request.confirmation_id), first_seq, &mut outcome.events).await?;
                        for canonical in &mut outcome.events {
                            for key in ["action", "target_id", "request_id", "confirmation_id"] { canonical.extra.insert(key.into(),event.extra[key].clone()); }
                            sqlx::query("UPDATE room_events SET event_json=? WHERE room_id=? AND seq=?")
                                .bind(serde_json::to_string(canonical)?).bind(&principal.room_id).bind(canonical.seq).execute(&mut *tx).await?;
                        }
                        Ok::<Value,PersistenceError>(json!({"status":"succeeded","request_id":action.request_id,"action":"delete_text_channel","target_id":action.channel_id,"confirmation_id":request.confirmation_id,"event_id":event.id}))
                            }
                            ConfirmedRoomAction::StartAgent(_) => {
                                let mut executing = request.clone(); executing.state = ConfirmationState::Executing;
                                sqlx::query("UPDATE delegated_action_requests SET state='executing',request_json=? WHERE room_id=? AND confirmation_id=?").bind(serde_json::to_string(&executing)?).bind(&principal.room_id).bind(&request.confirmation_id).execute(&mut *tx).await?;
                                let row=load_in(&mut tx,&principal.room_id,&request.confirmation_id).await?;
                                let control=crate::delegated_confirmed_agent::control_in(&mut tx,&principal.room_id,&row).await?;
                                let plan=self.prepare_agent_launch_in(&mut tx,crate::AgentControlAuthority::Delegated(&control),control.request_id(),control.payload(),"agent.start").await?;
                                match plan {
                                    crate::AgentStartPlan::Start(effect)=>Ok(json!({"status":"in_progress","request_id":control.request_id(),"action":"start_agent","target_id":control.payload()["agent_id"],"confirmation_id":request.confirmation_id,"operation_id":effect.operation_id})),
                                    crate::AgentStartPlan::Outcome(outcome)=>Ok(outcome.result),
                                }
                            }
                            ConfirmedRoomAction::CreateAgent(create) => {
                                // Both authorities are revalidated now; the room actor
                                // performs the catalog-selected creation after commit.
                                let proof = validate_in(&mut tx, &principal.room_id, &row).await?;
                                crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
                                let mut executing = request.clone(); executing.state = ConfirmationState::Executing;
                                sqlx::query("UPDATE delegated_action_requests SET state='executing',request_json=? WHERE room_id=? AND confirmation_id=?").bind(serde_json::to_string(&executing)?).bind(&principal.room_id).bind(&request.confirmation_id).execute(&mut *tx).await?;
                                Ok(json!({"status":"in_progress","request_id":create.request_id,"action":"create_agent","target_id":create.draft.agent_id,"confirmation_id":request.confirmation_id}))
                            }
                        }
                    }.await;
                    match execution {
                        Ok(result) => {
                            sqlx::query("RELEASE confirmed_action")
                                .execute(&mut *tx)
                                .await?;
                            finish_in(
                                &mut tx,
                                &principal.room_id,
                                &row,
                                if result["status"] == "in_progress" {
                                    ConfirmationState::Executing
                                } else {
                                    ConfirmationState::Succeeded
                                },
                                &result,
                            )
                            .await?;
                        }
                        Err(PersistenceError::CommandRejected { code, message }) => {
                            sqlx::query("ROLLBACK TO confirmed_action")
                                .execute(&mut *tx)
                                .await?;
                            sqlx::query("RELEASE confirmed_action")
                                .execute(&mut *tx)
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
                            let state = if ended {
                                ConfirmationState::Cancelled
                            } else {
                                ConfirmationState::Failed
                            };
                            let code = if ended {
                                std::borrow::Cow::Borrowed("confirmation_cancelled")
                            } else {
                                code
                            };
                            finish_in(&mut tx, &principal.room_id, &row, state, &json!({"status":"rejected","code":code,"message":message,"retryable":false})).await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            false
        };
        let current = load_in(&mut tx, &principal.room_id, &decision.confirmation_id).await?;
        let result = serde_json::to_value(agentsassemble_domain::DelegatedActionDecisionAck {
            confirmation_id: decision.confirmation_id.clone(),
            state: item(&current)?.state,
            receipt: receipt(&current)?,
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
        Ok(CommandOutcome {
            result,
            event,
            events,
            deduplicated,
        })
    }
}

fn requires_full_access_acknowledgement(action: &ConfirmedRoomAction) -> bool {
    match action {
        ConfirmedRoomAction::StartAgent(start) => {
            start.configuration.permission_mode == "full_access"
        }
        ConfirmedRoomAction::CreateAgent(create) => create.draft.permission_mode == "full_access",
        ConfirmedRoomAction::DeleteTextChannel(_) => false,
    }
}
