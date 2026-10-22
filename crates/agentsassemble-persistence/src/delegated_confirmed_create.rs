//! Owner-confirmed stopped-agent creation binds the catalog-selected draft.
//!
//! The AI supplies only the owner's own `agent.create` choice fields; identity,
//! acknowledgement, start intent and session custody cannot be supplied. The
//! request path stores the normalized parameters plus every server-resolved
//! default (including the deterministic agent identity) without mutation. The
//! approval driver re-runs the owner's catalog selection and executes the same
//! creation core as the owner's own stopped create, so catalog drift refuses
//! with `target_changed` instead of creating a stale session.
use std::borrow::Cow;

use crate::delegated_confirmation::{Binding, create_in, rejected};
use crate::delegated_confirmation_lifecycle::{
    finish_in, item, load_in, receipt, retry_in, validate_in,
};
use crate::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    AgentSessionDraft, AuthenticatedPrincipal, ConfirmationDecision, ConfirmationState,
    ConfirmedAgentCreate, ConfirmedRoomAction, DecideDelegatedAction, DelegatedActionRequest,
    RequestCreateAgent, canonical_payload_hash,
};
use serde_json::{Map, Value, json};
use sqlx::Row;
use uuid::Uuid;

/// Builds the exact owner-flow selection payload for a delegated create request.
///
/// The AI schema cannot carry acknowledgement, start intent or session identity;
/// the acknowledgement placeholder validates the candidate only and confers no
/// authority. The human acknowledgement that matters is enforced in the approval
/// decision. Creation is always stopped.
#[must_use]
pub fn create_selection_payload(params: &RequestCreateAgent) -> Value {
    let mut payload = Map::new();
    payload.insert("provider_id".into(), json!(params.provider_id.clone()));
    payload.insert(
        "catalog_revision".into(),
        json!(params.catalog_revision.clone()),
    );
    payload.insert("display_name".into(), json!(params.display_name.clone()));
    payload.insert("workspace".into(), json!(params.workspace.clone()));
    for (key, value) in [
        ("model", params.model.as_ref()),
        ("reasoning_effort", params.reasoning_effort.as_ref()),
        ("service_tier", params.service_tier.as_ref()),
        ("variant", params.variant.as_ref()),
        ("permission_mode", params.permission_mode.as_ref()),
        ("persona_card_id", params.persona_card_id.as_ref()),
        ("provider_endpoint", params.provider_endpoint.as_ref()),
    ] {
        if let Some(value) = value {
            payload.insert(key.into(), json!(value));
        }
    }
    if let Some(limit) = params.max_output_tokens {
        payload.insert("max_output_tokens".into(), json!(limit));
    }
    payload.insert("full_access_acknowledged".into(), json!(true));
    payload.insert("start_now".into(), json!(false));
    Value::Object(payload)
}

/// Builds the deterministic `agent.create` admission identity for an approved drive.
///
/// The acknowledgement records the verified decision outcome for the bound draft;
/// `_delegation` carries only attribution, never authority.
#[must_use]
pub fn create_execution_payload(
    params: &RequestCreateAgent,
    draft: &AgentSessionDraft,
    request: &DelegatedActionRequest,
) -> Value {
    let mut payload = create_selection_payload(params);
    payload["full_access_acknowledged"] = json!(draft.permission_mode == "full_access");
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
            "Creates a stopped agent owned by you with full access. Nothing runs yet: starting it later is a separate step that needs your explicit acknowledgement and permits host execution."
        }
        "workspace_write" => {
            "Creates a stopped agent owned by you with workspace write. Nothing runs yet: starting it later needs your separate confirmation and may spend provider credit."
        }
        _ => {
            "Creates a stopped conversation-only agent owned by you. It cannot touch the workspace until reconfigured, and starting it later is a separate step."
        }
    }
}

/// What one approved-create drive produced. `new_events` need publication only
/// when this call executed; otherwise the receipt is the recorded terminal state.
pub struct ApprovedCreateDrive {
    pub receipt: Value,
    pub state: ConfirmationState,
    pub new_events: Vec<agentsassemble_domain::RoomEvent>,
    pub event_id: Option<String>,
}

impl SqliteStore {
    /// Resolves the current owner principal for candidate catalog selection.
    /// Read-only; the request transaction re-resolves authoritatively.
    /// # Errors
    /// Rejects ended custody, role/scope changes and owners without agent control.
    pub async fn delegated_create_owner(
        &self,
        authority: &DelegateRoomAuthority,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        // Current authority, so a retry from a later turn can still read its stored receipt.
        let proof = crate::delegated_pin::DelegatedRoomActionAuthority::current(
            &mut tx,
            authority,
            "request_create_agent",
        )
        .await?;
        crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
        tx.commit().await?;
        Ok(proof.owner)
    }

    /// Stores only an immutable creation confirmation. Exact retries never debit again.
    /// # Errors
    /// Rejects stale custody, conflicting retries and exhausted budgets.
    pub async fn request_delegated_agent_create<A>(
        &self,
        authority: &DelegateRoomAuthority,
        params: &RequestCreateAgent,
        draft: &AgentSessionDraft,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(Value, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&params.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        if draft.agent_id.is_empty() || draft.display_name.is_empty() {
            return Err(rejected(
                "bad_request",
                "A catalog-selected agent draft is required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let action = ConfirmedRoomAction::CreateAgent(ConfirmedAgentCreate {
            request_id: params.request_id.clone(),
            params: params.clone(),
            draft: draft.clone(),
        });
        if let Some(result) = retry_in(&mut tx, authority, &params.request_id, &action).await? {
            tx.commit().await?;
            return Ok((result, None));
        }
        let proof = crate::delegated_pin::DelegatedRoomActionAuthority::direct(
            &mut tx,
            authority,
            "request_create_agent",
        )
        .await?;
        crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &params.request_id,
            "request_create_agent",
            &canonical_payload_hash(&serde_json::to_value(&action)?),
        )
        .await
        .map_err(|error| match error {
            PersistenceError::CommandConflict => rejected(
                "request_conflict",
                "This request ID already identifies another action.",
            ),
            other => other,
        })?;
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

    /// Loads the bound create request, its owner reference and current receipt.
    /// # Errors
    /// Rejects missing confirmations and storage failures.
    pub async fn approved_create_snapshot(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<(DelegatedActionRequest, Value, Value), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if !matches!(request.action, ConfirmedRoomAction::CreateAgent(_)) {
            return Err(rejected("bad_request", "An agent creation is required."));
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

    /// Records a drive-time selection failure as the terminal receipt.
    /// # Errors
    /// Propagates storage failures; a non-executing request returns its receipt.
    pub async fn fail_approved_agent_create(
        &self,
        room_id: &str,
        confirmation_id: &str,
        code: &str,
        message: &str,
    ) -> Result<ApprovedCreateDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if request.state != ConfirmationState::Executing {
            let receipt = receipt(&row)?;
            tx.commit().await?;
            return Ok(ApprovedCreateDrive {
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
        Ok(ApprovedCreateDrive {
            receipt,
            state: ConfirmationState::Failed,
            new_events: Vec::new(),
            event_id: None,
        })
    }

    /// Executes one approved stopped-agent creation through the owner's creation
    /// core with delegate attribution. Idempotent: exact retries return the
    /// recorded receipt without executing again.
    /// # Errors
    /// Returns the recorded terminal receipt for rejections; storage failures
    /// roll back without changing state so a later retry can re-drive.
    #[allow(clippy::too_many_lines)]
    pub async fn drive_approved_agent_create(
        &self,
        room_id: &str,
        confirmation_id: &str,
        draft: &AgentSessionDraft,
        payload: &Value,
        owner_reference: &Value,
    ) -> Result<ApprovedCreateDrive, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        let ConfirmedRoomAction::CreateAgent(stored) = &request.action else {
            return Err(rejected("bad_request", "An agent creation is required."));
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
            return Ok(ApprovedCreateDrive {
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
                return Ok(ApprovedCreateDrive {
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
            return Ok(ApprovedCreateDrive {
                receipt,
                state,
                new_events: Vec::new(),
                event_id: None,
            });
        }
        let owner = proof.owner.clone();
        let hash = canonical_payload_hash(payload);
        let size =
            crate::room_write_budget::command_size(&stored.request_id, "agent.create", payload)?;
        if let Some(outcome) = crate::command_admission::admit_non_lifecycle_command(
            &mut tx,
            room_id,
            &owner.principal_id,
            &stored.request_id,
            "agent.create",
            &hash,
            size,
        )
        .await?
        {
            // A committed creation already owns this identity; the confirmation
            // receipt is the recorded terminal state, never a second agent.
            let _ = outcome;
            let current = load_in(&mut tx, room_id, confirmation_id).await?;
            let receipt = receipt(&current)?;
            tx.commit().await?;
            return Ok(ApprovedCreateDrive {
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
            let owner_binding: crate::delegation_owner::OwnerBinding =
                serde_json::from_value(owner_reference.clone())?;
            // The sequence floor is claimed before any insert so attribution covers
            // exactly this transaction's events; no other writer enters the range.
            let first_seq = crate::room_event_sequence::next_sequence(&mut tx, room_id).await?;
            let records = crate::agent_creation_records::create_or_reuse_agent_records(
                &mut tx, &owner, draft, None, false,
            )
            .await?;
            if !records.committed_events.is_empty() {
                owner_binding
                    .save(
                        &mut tx,
                        room_id,
                        "participant",
                        &records.session.public.session_id,
                    )
                    .await?;
            }
            let mut events = records.committed_events;
            let mut system = crate::delegated_pin::attributed_action_event(
                &mut tx,
                proof.attribution(),
                &stored.request_id,
                "create_agent",
                &draft.agent_id,
                &format!("created the agent {}", draft.display_name),
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
            let mut result = records.result;
            result["events"] = serde_json::to_value(&events)?;
            result["event"] = serde_json::to_value(events.last())?;
            result["event_seq"] = events
                .last()
                .map_or(Value::Null, |event| Value::from(event.seq));
            let _stored = crate::agent_lifecycle_events::store_result(
                &mut tx,
                &owner,
                &stored.request_id,
                "agent.create",
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
                let receipt = json!({"status":"succeeded","request_id":stored.request_id,"action":"create_agent","target_id":draft.agent_id,"confirmation_id":request.confirmation_id,"event_id":event_id});
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
                Ok(ApprovedCreateDrive {
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
                Ok(ApprovedCreateDrive {
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
