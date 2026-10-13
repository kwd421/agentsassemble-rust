//! Owner-confirmed agent removal binds the target agent's creation identity.
//!
//! The AI supplies only the request identity and the target agent id; the target's
//! current creation identity is bound on the server. The request path stores the
//! immutable request without mutation. Approval revalidates owner and delegate
//! authority, re-binds the target and runs the same participant-removal core as the
//! owner's own kick of an agent, with the AI named on behalf of the owner. The
//! durable runtime-cleanup reservation is reconciled by the existing cleanup owner.
//!
//! Target resolution happens inside the request transaction *after* the exact-retry
//! lookup, so retrying an already executed request reads its receipt even though the
//! target is gone.
use crate::delegated_confirmation::{Binding, create_in, rejected};
use crate::delegated_confirmation_lifecycle::{item, load_in, receipt, refresh_in};
use crate::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    ConfirmedAgentRemove, ConfirmedRoomAction, DelegatedActionRequest, DurableAgentSession,
    ParticipantStatus, RequestRemoveAgent, canonical_payload_hash,
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

fn consequences_for(display_name: &str) -> String {
    format!(
        "Removes the agent {display_name} from the room. Its session ends, its access is revoked and a running runtime is stopped; this cannot be undone."
    )
}

/// Loads the bound target: an agent participant whose removal state still admits a
/// kick. A human participant and an already-removed agent are refused here so the
/// agent action can never reach a human or repeat a removal.
/// # Errors
/// Refuses `target_changed` for a missing, non-agent or already-removed target.
pub(crate) async fn load_removal_target(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    agent_id: &str,
) -> Result<DurableAgentSession, PersistenceError> {
    let session = crate::agent_lifecycle::load_session(tx, room_id, agent_id)
        .await
        .map_err(|error| match error {
            PersistenceError::CommandRejected { code, .. } if code == "not_found" => {
                rejected("target_changed", "The target agent no longer exists.")
            }
            other => other,
        })?;
    let participant = crate::agent_lifecycle::load_participant(tx, room_id, agent_id)
        .await
        .map_err(|error| match error {
            PersistenceError::ParticipantMissing => rejected(
                "target_changed",
                "The target agent is no longer a participant.",
            ),
            other => other,
        })?;
    if participant.participant_type != "agent"
        || matches!(
            participant.status,
            ParticipantStatus::Kicked | ParticipantStatus::Exported
        )
    {
        return Err(rejected(
            "target_changed",
            "The target is not an agent that can be removed from this room.",
        ));
    }
    Ok(session)
}

/// Runs the owner's own agent-removal core for one approved confirmation inside the
/// decision transaction, after re-binding the target's creation identity, and records
/// the AI's attributed system line. Runtime cleanup keeps its durable reservation.
/// # Errors
/// Refuses `target_changed` for a changed target; propagates storage failures.
pub(crate) async fn remove_agent_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    proof: &crate::delegated_pin::DelegatedRoomActionAuthority,
    request: &DelegatedActionRequest,
    remove: &ConfirmedAgentRemove,
    first_seq: i64,
) -> Result<Value, PersistenceError> {
    crate::participant_removal::authorized_removal_target(
        &proof.owner,
        &json!({"participant_id": remove.agent_id}),
    )?;
    let current = load_removal_target(tx, &proof.owner.room_id, &remove.agent_id).await?;
    if current.public.created_at != remove.expected_created_at {
        return Err(rejected(
            "target_changed",
            "The target agent changed before approval.",
        ));
    }
    let commit = crate::participant_removal::removal_in(
        tx,
        &proof.owner,
        &remove.agent_id,
        ParticipantStatus::Kicked,
        "participant_kicked",
    )
    .await?;
    let mut events = commit.events;
    let mut system = crate::delegated_pin::attributed_action_event(
        tx,
        proof.attribution(),
        &remove.request_id,
        "remove_agent",
        &remove.agent_id,
        &format!("removed the agent {}", request.target_name),
    )
    .await?;
    system
        .extra
        .insert("confirmation_id".into(), json!(request.confirmation_id));
    crate::room_turns::support::insert_event(tx, &system).await?;
    events.push(system.clone());
    crate::delegated_event_attribution::attribute_since(
        tx,
        &proof.delegate,
        &proof.owner.participant_id,
        Some(&request.confirmation_id),
        first_seq,
        &mut events,
    )
    .await?;
    Ok(json!({
        "status": "succeeded",
        "request_id": remove.request_id,
        "action": "remove_agent",
        "target_id": remove.agent_id,
        "confirmation_id": request.confirmation_id,
        "event_id": system.id,
    }))
}

impl SqliteStore {
    /// Stores only an immutable removal confirmation, binding the target's current
    /// creation identity. Exact retries never debit again and never re-read the target.
    /// # Errors
    /// Rejects stale custody, conflicting retries, a missing/removed target and
    /// exhausted budgets.
    pub async fn request_delegated_agent_removal<A>(
        &self,
        authority: &DelegateRoomAuthority,
        params: &RequestRemoveAgent,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(Value, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&params.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        if params.agent_id.is_empty() {
            return Err(rejected("bad_request", "A target agent id is required."));
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
            let bound = match &stored.action {
                ConfirmedRoomAction::RemoveAgent(remove) => remove,
                _ => {
                    return Err(rejected(
                        "request_conflict",
                        "This request ID identifies another action or execution.",
                    ));
                }
            };
            if !binding.authority.same_binding(authority) || bound.params != *params {
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
            "request_remove_agent",
        )
        .await?;
        crate::participant_removal::authorized_removal_target(
            &proof.owner,
            &json!({"participant_id": params.agent_id}),
        )?;
        let session = load_removal_target(&mut tx, authority.room_id(), &params.agent_id).await?;
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &params.request_id,
            "request_remove_agent",
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
        let action = ConfirmedRoomAction::RemoveAgent(ConfirmedAgentRemove {
            request_id: params.request_id.clone(),
            agent_id: params.agent_id.clone(),
            params: params.clone(),
            expected_created_at: session.public.created_at,
        });
        let result = create_in(
            &mut tx,
            authority,
            &proof,
            action,
            session.public.display_name.clone(),
            &consequences_for(&session.public.display_name),
            admit,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Loads the bound removal request, its owner reference and current receipt.
    /// # Errors
    /// Rejects missing confirmations and storage failures.
    pub async fn approved_removal_snapshot(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<(DelegatedActionRequest, Value, Value), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if !matches!(request.action, ConfirmedRoomAction::RemoveAgent(_)) {
            return Err(rejected("bad_request", "An agent removal is required."));
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
}
