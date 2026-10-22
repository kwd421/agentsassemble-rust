//! Owner-confirmed agent reconfiguration binds the target's configuration revision.
//!
//! The AI supplies only the owner's own `agent.configure` change fields; the sealed
//! runtime profile key and the resulting catalog-resolved profile are bound on the
//! server. The request paths store the normalized change fields plus the current
//! configuration revision and the resolved draft without mutation. The approval
//! driver re-resolves the owner's catalog selection and re-runs the same
//! configuration core as the owner's own stopped `agent.configure`, so a moved
//! revision or a missing target refuses with `target_changed` instead of
//! reconfiguring a stale session.
use std::borrow::Cow;

use crate::delegated_confirmation::{Binding, create_in, rejected};
use crate::delegated_confirmation_lifecycle::{
    finish_in, item, load_in, receipt, refresh_in, validate_in,
};
use crate::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    AgentSessionDraft, AuthenticatedPrincipal, ConfirmationDecision, ConfirmationState,
    ConfirmedAgentReconfigure, ConfirmedRoomAction, DecideDelegatedAction, DelegatedActionRequest,
    DurableAgentSession, RequestReconfigureAgent, canonical_payload_hash,
};
use serde_json::{Map, Value, json};
use sqlx::Row;
use uuid::Uuid;

/// Builds the owner-flow `agent.configure` selection payload for a delegated request.
///
/// Only the mutable change fields are carried; an omitted field keeps the current
/// profile through the owner's own selection merge. The acknowledgement placeholder
/// validates the candidate only and confers no authority: the human acknowledgement
/// that matters is enforced in the approval decision.
#[must_use]
pub fn reconfigure_selection_payload(params: &RequestReconfigureAgent) -> Value {
    let mut payload = Map::new();
    payload.insert("agent_id".into(), json!(params.agent_id.clone()));
    payload.insert(
        "catalog_revision".into(),
        json!(params.catalog_revision.clone()),
    );
    for (key, value) in [
        ("model", params.model.as_ref()),
        ("reasoning_effort", params.reasoning_effort.as_ref()),
        ("permission_mode", params.permission_mode.as_ref()),
        ("workspace", params.workspace.as_ref()),
    ] {
        if let Some(value) = value {
            payload.insert(key.into(), json!(value));
        }
    }
    payload.insert("full_access_acknowledged".into(), json!(true));
    Value::Object(payload)
}

/// Builds the `agent.configure` execution payload for an approved drive.
///
/// `_delegation` carries only attribution, never authority.
#[must_use]
pub fn reconfigure_execution_payload(
    params: &RequestReconfigureAgent,
    request: &DelegatedActionRequest,
) -> Value {
    let mut payload = reconfigure_selection_payload(params);
    payload["_delegation"] = json!({
        "actor_participant_id": request.delegate_participant_id,
        "on_behalf_of_participant_id": request.owner_participant_id,
        "confirmation_id": request.confirmation_id,
    });
    payload
}

fn consequences_for(permission_mode: &str) -> &'static str {
    match permission_mode {
        "full_access" => {
            "Reconfigures this stopped agent to full access. Starting it later is a separate step that needs your explicit acknowledgement and permits host execution."
        }
        "workspace_write" => {
            "Reconfigures this stopped agent to workspace write, so a later start can change files."
        }
        _ => {
            "Reconfigures this stopped conversation-only agent. It cannot touch the workspace until reconfigured again."
        }
    }
}

/// What one approved-reconfigure drive produced. `new_events` need publication only
/// when this call executed; otherwise the receipt is the recorded terminal state.
pub struct ApprovedReconfigureDrive {
    pub receipt: Value,
    pub state: ConfirmationState,
    pub new_events: Vec<agentsassemble_domain::RoomEvent>,
    pub event_id: Option<String>,
}

impl SqliteStore {
    /// Resolves the current owner principal and the exact stopped target for a
    /// delegated reconfigure request. Read-only; the request transaction re-resolves
    /// authoritatively.
    /// # Errors
    /// Rejects ended custody, role/scope changes and a missing target.
    pub async fn delegated_reconfigure_candidate(
        &self,
        authority: &DelegateRoomAuthority,
        agent_id: &str,
    ) -> Result<(AuthenticatedPrincipal, DurableAgentSession), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        // Current authority, so a retry from a later turn can still read its stored receipt.
        let proof = crate::delegated_pin::DelegatedRoomActionAuthority::current(
            &mut tx,
            authority,
            "request_reconfigure_agent",
        )
        .await?;
        crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
        let session = load_reconfigure_target(&mut tx, authority.room_id(), agent_id).await?;
        tx.commit().await?;
        Ok((proof.owner, session))
    }

    /// Loads the current target for a drive-time catalog re-resolution.
    /// # Errors
    /// Rejects a missing target and propagates storage failures.
    pub async fn delegated_reconfigure_session(
        &self,
        room_id: &str,
        agent_id: &str,
    ) -> Result<DurableAgentSession, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let session = load_reconfigure_target(&mut tx, room_id, agent_id).await?;
        tx.commit().await?;
        Ok(session)
    }

    /// Stores only an immutable reconfiguration confirmation. Exact retries never
    /// debit again. The static change fields bind the request identity; the sealed
    /// configuration revision is re-read on every retry because it moves when the
    /// approved reconfiguration executes.
    /// # Errors
    /// Rejects stale custody, conflicting retries and exhausted budgets.
    pub async fn request_delegated_agent_reconfigure<A>(
        &self,
        authority: &DelegateRoomAuthority,
        params: &RequestReconfigureAgent,
        expected_configuration_revision: &str,
        draft: &AgentSessionDraft,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(Value, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&params.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        if params.agent_id.is_empty() || draft.agent_id != params.agent_id {
            return Err(rejected(
                "bad_request",
                "A catalog-selected agent draft is required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = sqlx::query("SELECT * FROM delegated_action_requests WHERE room_id=? AND delegate_id=? AND request_id=?")
            .bind(authority.room_id())
            .bind(authority.delegate_id())
            .bind(&params.request_id)
            .fetch_optional(&mut *tx)
            .await?
        {
            let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
            let stored = item(&row)?;
            let ConfirmedRoomAction::ReconfigureAgent(bound) = &stored.action else {
                return Err(rejected(
                    "request_conflict",
                    "This request ID identifies another action or execution.",
                ));
            };
            if !binding.authority.same_delegate(authority) || bound.params != *params {
                return Err(rejected(
                    "request_conflict",
                    "This request ID identifies another action or execution.",
                ));
            }
            refresh_in(&mut tx, authority.room_id(), &row).await?;
            let row = load_in(&mut tx, authority.room_id(), &stored.confirmation_id).await?;
            let result = receipt(&row)?;
            tx.commit().await?;
            return Ok((result, None));
        }
        let proof = crate::delegated_pin::DelegatedRoomActionAuthority::direct(
            &mut tx,
            authority,
            "request_reconfigure_agent",
        )
        .await?;
        crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &params.request_id,
            "request_reconfigure_agent",
            &canonical_payload_hash(&serde_json::to_value(params)?),
        )
        .await
        .map_err(|error| match error {
            PersistenceError::CommandConflict => rejected(
                "request_conflict",
                "This request ID already identifies another action.",
            ),
            other => other,
        })?;
        let action = ConfirmedRoomAction::ReconfigureAgent(ConfirmedAgentReconfigure {
            request_id: params.request_id.clone(),
            agent_id: params.agent_id.clone(),
            params: params.clone(),
            expected_configuration_revision: expected_configuration_revision.to_owned(),
            draft: draft.clone(),
        });
        let result = create_in(
            &mut tx,
            authority,
            &proof,
            action,
            draft.display_name.clone(),
            consequences_for(&draft.permission_mode),
            admit,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Loads the bound reconfigure request, its owner reference and current receipt.
    /// # Errors
    /// Rejects missing confirmations and storage failures.
    pub async fn approved_reconfigure_snapshot(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<(DelegatedActionRequest, Value, Value), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if !matches!(request.action, ConfirmedRoomAction::ReconfigureAgent(_)) {
            return Err(rejected(
                "bad_request",
                "An agent reconfiguration is required.",
            ));
        }
        let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
        let receipt = receipt(&row)?;
        tx.commit().await?;
        Ok((
            request,
            serde_json::to_value(binding.owner_reference)?,
            receipt,
        ))
    }

    /// Records a drive-time selection or target failure as the terminal receipt.
    /// # Errors
    /// Propagates storage failures; a non-executing request returns its receipt.
    pub async fn fail_approved_agent_reconfigure(
        &self,
        room_id: &str,
        confirmation_id: &str,
        code: &str,
        message: &str,
    ) -> Result<ApprovedReconfigureDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if request.state != ConfirmationState::Executing {
            let receipt = receipt(&row)?;
            tx.commit().await?;
            return Ok(ApprovedReconfigureDrive {
                state: request.state,
                event_id: receipt
                    .get("event_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                receipt,
                new_events: Vec::new(),
            });
        }
        let receipt = json!({"status":"rejected","code":code,"message":message,"retryable":false});
        finish_in(&mut tx, room_id, &row, ConfirmationState::Failed, &receipt).await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(ApprovedReconfigureDrive {
            receipt,
            state: ConfirmationState::Failed,
            new_events: Vec::new(),
            event_id: None,
        })
    }

    /// Executes one approved reconfiguration through the owner's configuration core
    /// with delegate attribution, after re-binding the target's configuration
    /// revision. Idempotent: exact retries return the recorded receipt.
    /// # Errors
    /// Returns the recorded terminal receipt for rejections; storage failures roll
    /// back without changing state so a later retry can re-drive.
    #[allow(clippy::too_many_lines)]
    pub async fn drive_approved_agent_reconfigure(
        &self,
        room_id: &str,
        confirmation_id: &str,
        draft: &AgentSessionDraft,
        payload: &Value,
        owner_reference: &Value,
    ) -> Result<ApprovedReconfigureDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        let ConfirmedRoomAction::ReconfigureAgent(stored) = &request.action else {
            return Err(rejected(
                "bad_request",
                "An agent reconfiguration is required.",
            ));
        };
        if request.state != ConfirmationState::Executing {
            if request.state == ConfirmationState::Pending {
                return Err(rejected(
                    "permission_denied",
                    "The owner has not approved this execution.",
                ));
            }
            let receipt = receipt(&row)?;
            tx.commit().await?;
            return Ok(ApprovedReconfigureDrive {
                state: request.state,
                event_id: receipt
                    .get("event_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                receipt,
                new_events: Vec::new(),
            });
        }
        let decision: DecideDelegatedAction = serde_json::from_str(
            row.get::<Option<&str>, _>("decision_json")
                .ok_or_else(|| rejected("permission_denied", "The owner has not approved."))?,
        )?;
        if decision.decision != ConfirmationDecision::Approve {
            return Err(rejected(
                "permission_denied",
                "The owner has not approved this execution.",
            ));
        }
        if stored.draft != *draft {
            return Err(rejected(
                "request_conflict",
                "This execution is not the approved action.",
            ));
        }
        let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
        if serde_json::to_value(&binding.owner_reference)? != *owner_reference {
            return Err(rejected(
                "request_conflict",
                "This execution is not the approved action.",
            ));
        }
        let proof = match validate_in(&mut tx, room_id, &row).await {
            Ok(proof) => proof,
            Err(
                PersistenceError::CommandRejected { .. }
                | PersistenceError::ParticipantMissing
                | PersistenceError::RoomMissing,
            ) => {
                let receipt = json!({"status":"rejected","code":"confirmation_cancelled","message":"The owner or delegate authority ended before execution.","retryable":false});
                finish_in(
                    &mut tx,
                    room_id,
                    &row,
                    ConfirmationState::Cancelled,
                    &receipt,
                )
                .await?;
                tx.commit().await?;
                self.notify_room_directory_changed();
                return Ok(ApprovedReconfigureDrive {
                    receipt,
                    state: ConfirmationState::Cancelled,
                    new_events: Vec::new(),
                    event_id: None,
                });
            }
            Err(error) => return Err(error),
        };
        if let Err(error) = crate::agent_lifecycle_authority::authorize_control(&proof.owner) {
            let (state, code) = authority_end(&error);
            let message = authority_message(&error);
            let receipt =
                json!({"status":"rejected","code":code,"message":message,"retryable":false});
            finish_in(&mut tx, room_id, &row, state, &receipt).await?;
            tx.commit().await?;
            self.notify_room_directory_changed();
            return Ok(ApprovedReconfigureDrive {
                receipt,
                state,
                new_events: Vec::new(),
                event_id: None,
            });
        }
        let owner = proof.owner.clone();
        // The bound target: the configuration sealed at request time must still hold,
        // and the execution must be the exact profile the owner approved.
        let Ok(current) = load_reconfigure_target(&mut tx, room_id, &stored.agent_id).await else {
            let outcome =
                finish_target_changed(&mut tx, room_id, &row, "The target agent no longer exists.")
                    .await?;
            tx.commit().await?;
            self.notify_room_directory_changed();
            return Ok(outcome);
        };
        if current.runtime_profile_key != stored.expected_configuration_revision {
            let outcome = finish_target_changed(
                &mut tx,
                room_id,
                &row,
                "The target agent configuration changed before execution.",
            )
            .await?;
            tx.commit().await?;
            self.notify_room_directory_changed();
            return Ok(outcome);
        }
        let hash = canonical_payload_hash(payload);
        let size =
            crate::room_write_budget::command_size(&stored.request_id, "agent.configure", payload)?;
        if let Some(_outcome) = crate::command_admission::admit_non_lifecycle_command(
            &mut tx,
            room_id,
            &owner.principal_id,
            &stored.request_id,
            "agent.configure",
            &hash,
            size,
        )
        .await?
        {
            // A committed reconfiguration already owns this identity; the
            // confirmation receipt is the recorded terminal state.
            let current = load_in(&mut tx, room_id, confirmation_id).await?;
            let receipt = receipt(&current)?;
            tx.commit().await?;
            return Ok(ApprovedReconfigureDrive {
                state: item(&current)?.state,
                event_id: receipt
                    .get("event_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                receipt,
                new_events: Vec::new(),
            });
        }
        let execution = async {
            crate::filesystem_authority::revalidate_runtime_authority(draft).await?;
            let first_seq = crate::room_event_sequence::next_sequence(&mut tx, room_id).await?;
            let (session, _participant, configure_event) =
                crate::agent_configuration::configure_session_in(
                    &mut tx,
                    &owner,
                    &stored.expected_configuration_revision,
                    draft,
                )
                .await?;
            let mut events = vec![configure_event];
            let mut system = crate::delegated_pin::attributed_action_event(
                &mut tx,
                proof.attribution(),
                &stored.request_id,
                "reconfigure_agent",
                &draft.agent_id,
                &format!("reconfigured the agent {}", draft.display_name),
            )
            .await?;
            system
                .extra
                .insert("confirmation_id".into(), json!(request.confirmation_id));
            crate::room_turns::support::insert_event(&mut tx, &system).await?;
            events.push(system.clone());
            crate::delegated_event_attribution::attribute_since(
                &mut tx,
                &proof.delegate,
                &proof.owner.participant_id,
                Some(&request.confirmation_id),
                first_seq,
                &mut events,
            )
            .await?;
            let mut result = json!({
                "status": "configured",
                "agent_session": session.public,
                "events": serde_json::to_value(&events)?,
                "event": serde_json::to_value(events.last())?,
            });
            result["event_seq"] = events
                .last()
                .map_or(Value::Null, |event| Value::from(event.seq));
            let _stored = crate::agent_lifecycle_events::store_result(
                &mut tx,
                &owner,
                &stored.request_id,
                "agent.configure",
                hash,
                result,
                events.clone(),
            )
            .await?;
            Ok::<_, PersistenceError>((events, system.id.clone()))
        }
        .await;
        match execution {
            Ok((events, event_id)) => {
                let receipt = json!({"status":"succeeded","request_id":stored.request_id,"action":"reconfigure_agent","target_id":draft.agent_id,"confirmation_id":request.confirmation_id,"event_id":event_id});
                finish_in(
                    &mut tx,
                    room_id,
                    &row,
                    ConfirmationState::Succeeded,
                    &receipt,
                )
                .await?;
                tx.commit().await?;
                self.notify_room_directory_changed();
                Ok(ApprovedReconfigureDrive {
                    receipt,
                    state: ConfirmationState::Succeeded,
                    new_events: events,
                    event_id: Some(event_id),
                })
            }
            Err(PersistenceError::CommandRejected { code, message }) => {
                if code == "write_budget_exceeded" {
                    // Transient: keep executing so a later retry re-drives.
                    return Err(PersistenceError::CommandRejected { code, message });
                }
                let receipt =
                    json!({"status":"rejected","code":code,"message":message,"retryable":false});
                finish_in(&mut tx, room_id, &row, ConfirmationState::Failed, &receipt).await?;
                tx.commit().await?;
                self.notify_room_directory_changed();
                Ok(ApprovedReconfigureDrive {
                    receipt,
                    state: ConfirmationState::Failed,
                    new_events: Vec::new(),
                    event_id: None,
                })
            }
            Err(error) => Err(error),
        }
    }
}

async fn load_reconfigure_target(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    agent_id: &str,
) -> Result<DurableAgentSession, PersistenceError> {
    crate::agent_lifecycle::load_session(tx, room_id, agent_id)
        .await
        .map_err(|error| match error {
            PersistenceError::CommandRejected { code, .. } if code == "not_found" => {
                rejected("target_changed", "The target agent no longer exists.")
            }
            other => other,
        })
}

async fn finish_target_changed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    row: &sqlx::sqlite::SqliteRow,
    message: &str,
) -> Result<ApprovedReconfigureDrive, PersistenceError> {
    let receipt =
        json!({"status":"rejected","code":"target_changed","message":message,"retryable":false});
    finish_in(tx, room_id, row, ConfirmationState::Failed, &receipt).await?;
    Ok(ApprovedReconfigureDrive {
        receipt,
        state: ConfirmationState::Failed,
        new_events: Vec::new(),
        event_id: None,
    })
}

fn authority_end(error: &PersistenceError) -> (ConfirmationState, Cow<'static, str>) {
    match error {
        PersistenceError::CommandRejected { code, .. }
            if matches!(
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
            ) =>
        {
            (
                ConfirmationState::Cancelled,
                Cow::Borrowed("confirmation_cancelled"),
            )
        }
        PersistenceError::CommandRejected { code, .. } => {
            (ConfirmationState::Failed, Cow::Owned(code.to_string()))
        }
        _ => (
            ConfirmationState::Failed,
            Cow::Borrowed("agent_control_unavailable"),
        ),
    }
}

fn authority_message(error: &PersistenceError) -> String {
    match error {
        PersistenceError::CommandRejected { message, .. } => message.clone(),
        _ => "The owner cannot control agents.".to_owned(),
    }
}
