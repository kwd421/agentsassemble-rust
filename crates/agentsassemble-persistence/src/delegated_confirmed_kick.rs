//! Owner-confirmed human participant kick binds the target's admission identity.
//!
//! The AI supplies only the request identity and the target participant id; the
//! target's current identity is bound on the server. The request path stores the
//! immutable request without mutation. Approval revalidates owner and delegate
//! authority, re-binds the target and runs the same participant-removal core as the
//! owner's own kick of a human, with the AI named on behalf of the owner. The
//! removal's revoked live sessions are returned to the caller so the room actor
//! broadcasts the same session revocation the owner's own kick sends.
//!
//! Target resolution happens inside the request transaction *after* the exact-retry
//! lookup, so retrying an already executed request reads its receipt even though the
//! target is gone.
use crate::delegated_confirmation::{Binding, create_in, rejected};
use crate::delegated_confirmation_lifecycle::{item, load_in, receipt, refresh_in};
use crate::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    ConfirmedParticipantKick, ConfirmedRoomAction, DelegatedActionRequest, Participant,
    ParticipantStatus, RequestKickParticipant, canonical_payload_hash,
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

fn consequences_for(display_name: &str) -> String {
    format!(
        "Removes the person {display_name} from the room and ends their access: their live room session is closed and their invite or join link stops working. The AIs they own leave with them and stop; this cannot be undone."
    )
}

/// Loads the bound target: a human participant whose removal state still admits a
/// kick. An agent and an already-removed person are refused here, so a kick never
/// targets an agent directly or repeats a removal; the AIs the person owns leave with
/// them through the shared removal core.
/// # Errors
/// Refuses `target_changed` for a missing, non-human or already-removed target.
pub(crate) async fn load_kick_target(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Participant, PersistenceError> {
    let participant = crate::agent_lifecycle::load_participant(tx, room_id, participant_id)
        .await
        .map_err(|error| match error {
            PersistenceError::ParticipantMissing => rejected(
                "target_changed",
                "The target participant is no longer in this room.",
            ),
            other => other,
        })?;
    if participant.participant_type != "human"
        || matches!(
            participant.status,
            ParticipantStatus::Kicked | ParticipantStatus::Exported
        )
    {
        return Err(rejected(
            "target_changed",
            "The target is not a person who can be removed from this room.",
        ));
    }
    Ok(participant)
}

/// Runs the owner's own participant-removal core for one approved confirmation inside
/// the decision transaction, after re-binding the target's admission identity, and
/// records the AI's attributed system line. Returns the revoked live-session
/// fingerprints so the room actor can close them exactly as the owner's own kick does.
/// # Errors
/// Refuses `target_changed` for a changed target; propagates storage failures.
pub(crate) async fn kick_participant_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    proof: &crate::delegated_pin::DelegatedRoomActionAuthority,
    request: &DelegatedActionRequest,
    kick: &ConfirmedParticipantKick,
    first_seq: i64,
) -> Result<(Value, Vec<[u8; 32]>), PersistenceError> {
    crate::participant_removal::authorized_removal_target(
        &proof.owner,
        &json!({"participant_id": kick.participant_id}),
    )?;
    let current = load_kick_target(tx, &proof.owner.room_id, &kick.participant_id).await?;
    if current.created_at != kick.expected_created_at
        || current.updated_at != kick.expected_updated_at
    {
        return Err(rejected(
            "target_changed",
            "The target participant changed before approval.",
        ));
    }
    let commit = crate::participant_removal::removal_in(
        tx,
        &proof.owner,
        &kick.participant_id,
        ParticipantStatus::Kicked,
        "participant_kicked",
    )
    .await?;
    let revoked = commit.revoked_session_fingerprints;
    let mut events = commit.events;
    let mut system = crate::delegated_pin::attributed_action_event(
        tx,
        proof.attribution(),
        &kick.request_id,
        "kick_participant",
        &kick.participant_id,
        &format!("kicked {}", request.target_name),
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
    Ok((
        json!({
            "status": "succeeded",
            "request_id": kick.request_id,
            "action": "kick_participant",
            "target_id": kick.participant_id,
            "confirmation_id": request.confirmation_id,
            "event_id": system.id,
        }),
        revoked,
    ))
}

impl SqliteStore {
    /// Stores only an immutable kick confirmation, binding the target's current
    /// admission identity. Exact retries never debit again and never re-read the target.
    /// # Errors
    /// Rejects stale custody, conflicting retries, a missing/removed/non-human target
    /// and exhausted budgets.
    pub async fn request_delegated_participant_kick<A>(
        &self,
        authority: &DelegateRoomAuthority,
        params: &RequestKickParticipant,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(Value, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&params.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        if params.participant_id.is_empty() {
            return Err(rejected(
                "bad_request",
                "A target participant id is required.",
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
            let bound = match &stored.action {
                ConfirmedRoomAction::KickParticipant(kick) => kick,
                _ => {
                    return Err(rejected(
                        "request_conflict",
                        "This request ID identifies another action or execution.",
                    ));
                }
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
            "request_kick_participant",
        )
        .await?;
        crate::participant_removal::authorized_removal_target(
            &proof.owner,
            &json!({"participant_id": params.participant_id}),
        )?;
        let target = load_kick_target(&mut tx, authority.room_id(), &params.participant_id).await?;
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &params.request_id,
            "request_kick_participant",
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
        let action = ConfirmedRoomAction::KickParticipant(ConfirmedParticipantKick {
            request_id: params.request_id.clone(),
            participant_id: params.participant_id.clone(),
            params: params.clone(),
            expected_created_at: target.created_at,
            expected_updated_at: target.updated_at,
        });
        let result = create_in(
            &mut tx,
            authority,
            &proof,
            action,
            target.display_name.clone(),
            &consequences_for(&target.display_name),
            admit,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Loads the bound kick request, its owner reference and current receipt.
    /// # Errors
    /// Rejects missing confirmations, other actions and storage failures.
    pub async fn approved_kick_snapshot(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<(DelegatedActionRequest, Value, Value), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let request = item(&row)?;
        if !matches!(request.action, ConfirmedRoomAction::KickParticipant(_)) {
            return Err(rejected("bad_request", "A participant kick is required."));
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
