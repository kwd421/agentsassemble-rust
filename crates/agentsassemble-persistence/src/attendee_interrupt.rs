use agentsassemble_domain::canonical_payload_hash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    AttendeeConnectionAuthorization, CommandOutcome, PersistenceError, ProviderTurnEffectPhase,
    ProviderTurnReconciliationCandidate, ProviderTurnStartAuthority, RoomCommandMutation,
    SqliteStore,
    attendee_invites::rejected,
    command_admission::{admit_non_lifecycle_command, store_command_result},
    provider_turn_effect::{canonical_now, load_effect_in, transition_to_waiting_in},
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeInterruptDelivery {
    pub effect_id: String,
    pub dispatch_nonce: String,
    pub cause: String,
    pub authority: ProviderTurnStartAuthority,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeInterruptReport {
    pub request_id: Uuid,
    pub interrupted: AttendeeInterruptDelivery,
    pub runtime: AttendeeInterruptedRuntime,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttendeeInterruptedRuntime {
    Retained,
    Gone,
}

impl SqliteStore {
    /// Delivers one canonical interrupt to its current external connection without host I/O.
    ///
    /// # Errors
    /// Rejects replaced or revoked connections, corrupt effect state and pending stop custody.
    pub async fn deliver_attendee_interrupt(
        &self,
        connection: &AttendeeConnectionAuthorization,
        now: DateTime<Utc>,
    ) -> Result<Option<AttendeeInterruptDelivery>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_connection::revalidate_in(&mut tx, connection, now).await?;
        let Some(mut candidate) = load_candidate(&mut tx, connection).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        let Some(effect) = &candidate.effect else {
            tx.commit().await?;
            return Ok(None);
        };
        if effect.phase == ProviderTurnEffectPhase::Prepared {
            // Delivery may have crossed the socket before a disconnect. Only this external owner
            // can resolve that uncertainty; no host lease or fabricated issued proof is acquired.
            let changed = sqlx::query("UPDATE provider_turn_effects SET phase='dispatching', dispatch_nonce=?, updated_at=? WHERE room_id=? AND effect_id=? AND phase='prepared'")
                .bind(Uuid::new_v4().to_string()).bind(canonical_now()).bind(&effect.room_id).bind(&effect.effect_id)
                .execute(&mut *tx).await?;
            if changed.rows_affected() != 1 {
                return Err(stale_interrupt());
            }
            candidate.effect = Some(
                load_effect_in(
                    &mut tx,
                    &effect.room_id,
                    &effect.session_id,
                    effect.turn_generation,
                )
                .await?,
            );
        }
        let delivery = project(&candidate)?;
        tx.commit().await?;
        Ok(Some(delivery))
    }

    /// Commits positive exact quiescence and its retry receipt through canonical turn owners.
    ///
    /// # Errors
    /// Rejects stale connections/effects, changed retries and unconfirmed runtime outcomes.
    pub async fn record_attendee_interrupt(
        &self,
        authority: &crate::AttendeeCleanupAuthorization,
        connection_id: Uuid,
        report: &AttendeeInterruptReport,
        now: DateTime<Utc>,
    ) -> Result<RoomCommandMutation, PersistenceError> {
        if report.request_id.is_nil() {
            return Err(stale_interrupt());
        }
        let payload = serde_json::to_value(report)?;
        let hash = canonical_payload_hash(&payload);
        let request = report.request_id.to_string();
        let action = "bridge.interrupt.report";
        let key = &authority.key;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_cleanup::revalidate_in(&mut tx, authority).await?;
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut tx,
            &key.room_id,
            &key.session_id,
            &request,
            action,
            &hash,
            crate::room_write_budget::command_size(&request, action, &payload)?,
        )
        .await?
        {
            tx.commit().await?;
            return Ok(RoomCommandMutation {
                outcome,
                assignments: Vec::new(),
            });
        }
        // An exact receipt remains recoverable after runtime-gone detached membership.
        // A new proof still requires the current normal session and connection generation.
        let connection = crate::attendee_connection::authorize_current_in(
            &mut tx,
            &authority.fingerprint,
            connection_id,
            now,
        )
        .await?;
        let candidate = load_candidate(&mut tx, &connection)
            .await?
            .ok_or_else(stale_interrupt)?;
        if project(&candidate)? != report.interrupted {
            return Err(stale_interrupt());
        }
        let effect = candidate.effect.as_ref().ok_or_else(stale_interrupt)?;
        let commit = match report.runtime {
            AttendeeInterruptedRuntime::Retained => {
                let waiting = transition_to_waiting_in(&mut tx, effect, None).await?;
                crate::provider_turn_effect_finalize::finalize_retained_in(&mut tx, &waiting)
                    .await?
            }
            AttendeeInterruptedRuntime::Gone => {
                crate::provider_turn_reconciliation::finalize_runtime_gone_in(
                    &mut tx,
                    &candidate,
                    crate::provider_turn_reconciliation::FloorProgression::Assign,
                )
                .await?
            }
        };
        let event = commit.events.last().cloned().ok_or_else(stale_interrupt)?;
        let result = json!({"event":event,"events":commit.events});
        store_command_result(
            &mut tx,
            (&key.room_id, &key.session_id),
            &request,
            action,
            &hash,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(RoomCommandMutation {
            outcome: CommandOutcome {
                result,
                event,
                events: commit.events,
                deduplicated: false,
            },
            assignments: commit.next_assignments,
        })
    }
}

async fn load_candidate(
    tx: &mut Transaction<'_, Sqlite>,
    connection: &AttendeeConnectionAuthorization,
) -> Result<Option<ProviderTurnReconciliationCandidate>, PersistenceError> {
    let principal = connection.session.principal();
    let candidate = crate::provider_turn_reconciliation::load_active_candidate_in(
        tx,
        &principal.room_id,
        &principal.participant_id,
    )
    .await?;
    if candidate
        .as_ref()
        .is_some_and(|candidate| !candidate.session.lifecycle_intent_action.is_none())
    {
        return Err(stale_interrupt());
    }
    Ok(candidate)
}

fn project(
    candidate: &ProviderTurnReconciliationCandidate,
) -> Result<AttendeeInterruptDelivery, PersistenceError> {
    let effect = candidate.effect.as_ref().ok_or_else(stale_interrupt)?;
    if effect.phase != ProviderTurnEffectPhase::Dispatching || effect.dispatch_nonce.is_empty() {
        return Err(stale_interrupt());
    }
    Ok(AttendeeInterruptDelivery {
        effect_id: effect.effect_id.clone(),
        dispatch_nonce: effect.dispatch_nonce.clone(),
        cause: effect.cause.as_str().to_owned(),
        authority: candidate.execution.clone().into(),
    })
}

fn stale_interrupt() -> PersistenceError {
    rejected(
        "external_interrupt_changed",
        "The exact external interrupt authority is unavailable or changed.",
    )
}
