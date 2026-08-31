use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, DurableAgentSession, Participant, ParticipantStatus, RoomEvent,
    canonical_payload_hash,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    AgentTurnAssignment, CommandOutcome, PersistenceError, ProviderTurnInterruptCause,
    ProviderTurnInterruptEffect, SqliteStore,
    agent_lifecycle::save_session,
    authority::active_room_for_principal,
    command_admission::{admit_non_lifecycle_command, store_command_result},
    provider_turn_effect::prepare_interrupt_effect,
    provider_turn_execution::load_execution_in,
    room_event_sequence::next_sequence,
    room_turns::{
        assign_pending_in,
        support::{insert_event, load_active_room, load_participant},
    },
    room_write_budget::command_size,
    turn_authority::active_turn_authority,
};

const ACTION: &str = "participant.mute";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParticipantMute {
    participant_id: String,
    muted: bool,
}

struct PreparedParticipantMute {
    participant: Participant,
    event: RoomEvent,
    scheduling: crate::AgentTurnCommit,
    interrupt_effect: Option<ProviderTurnInterruptEffect>,
}

#[derive(Debug, Clone)]
pub struct ParticipantMuteMutation {
    pub outcome: CommandOutcome,
    pub assignments: Vec<AgentTurnAssignment>,
    pub interrupt_effect: Option<ProviderTurnInterruptEffect>,
}

impl SqliteStore {
    /// Atomically changes canonical room mute state and prepares exact Agent effects.
    ///
    /// # Errors
    ///
    /// Returns authorization, payload, membership, replay, or exact-turn failures.
    pub async fn execute_participant_mute(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<ParticipantMuteMutation, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if !principal.capabilities.participant_mute {
            return Err(rejected(
                "permission_denied",
                "This room session cannot mute participants.",
            ));
        }
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            ACTION,
            &payload_hash,
            command_size(request_id, ACTION, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(ParticipantMuteMutation {
                outcome,
                assignments: Vec::new(),
                interrupt_effect: None,
            });
        }
        let update = parse_update(payload)?;
        let prepared = prepare_participant_mute(&mut transaction, principal, &update).await?;
        let result = json!({
            "participant": prepared.participant,
            "event": prepared.event,
            "event_seq": prepared.event.seq,
        });
        store_command_result(
            &mut transaction,
            principal,
            request_id,
            ACTION,
            &payload_hash,
            &result,
        )
        .await?;
        transaction.commit().await?;
        let mut events = vec![prepared.event.clone()];
        events.extend(prepared.scheduling.events);
        Ok(ParticipantMuteMutation {
            outcome: CommandOutcome {
                result,
                event: prepared.event,
                events,
                deduplicated: false,
            },
            assignments: prepared.scheduling.next_assignments,
            interrupt_effect: prepared.interrupt_effect,
        })
    }
}

async fn prepare_participant_mute(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    update: &ParticipantMute,
) -> Result<PreparedParticipantMute, PersistenceError> {
    let mut participant = load_participant(transaction, &principal.room_id, &update.participant_id)
        .await
        .map_err(|error| match error {
            PersistenceError::ParticipantMissing => rejected(
                "participant_not_found",
                "The participant does not exist in this room.",
            ),
            other => other,
        })?;
    require_target_membership(&participant, &principal.room_id, &update.participant_id)?;
    let mut agent_session = match participant.participant_type.as_str() {
        "human" => None,
        "agent" => Some(
            load_agent_session_for_participant(
                transaction,
                &principal.room_id,
                &participant.participant_id,
            )
            .await?,
        ),
        _ => {
            return Err(rejected(
                "stored_participant_invalid",
                "Stored participant type is not canonical.",
            ));
        }
    };
    participant.muted = update.muted;
    participant.updated_at = Utc::now();
    let event = muted_event(transaction, principal, &participant).await?;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(serde_json::to_string(&participant)?)
    .bind(&principal.room_id)
    .bind(&participant.participant_id)
    .execute(&mut **transaction)
    .await?;
    insert_event(transaction, &event).await?;
    let mut interrupt_effect = None;
    let mut scheduling = crate::AgentTurnCommit {
        events: Vec::new(),
        next_assignments: Vec::new(),
    };
    if let Some(session) = &mut agent_session {
        if update.muted && active_turn_authority(session).map_err(|_| stored_turn_invalid())? {
            let execution = load_execution_in(
                transaction,
                &principal.room_id,
                &session.public.session_id,
                session.turn_generation,
            )
            .await?;
            interrupt_effect = Some(
                prepare_interrupt_effect(
                    transaction,
                    &execution,
                    ProviderTurnInterruptCause::ParticipantMuted,
                )
                .await?,
            );
        } else if !update.muted {
            session.schedule_requested = true;
            session.public.updated_at = Utc::now();
            save_session(transaction, session).await?;
            let (room, settings) = load_active_room(transaction, &principal.room_id).await?;
            scheduling = assign_pending_in(transaction, &room, &settings).await?;
        }
    }
    Ok(PreparedParticipantMute {
        participant,
        event,
        scheduling,
        interrupt_effect,
    })
}

fn parse_update(payload: &Value) -> Result<ParticipantMute, PersistenceError> {
    let update = serde_json::from_value::<ParticipantMute>(payload.clone()).map_err(|_| {
        rejected(
            "invalid_participant_mute",
            "participant.mute requires exactly participant_id and muted.",
        )
    })?;
    if update.participant_id.is_empty()
        || update.participant_id.len() > 128
        || update.participant_id.trim() != update.participant_id
        || update.participant_id.chars().any(char::is_control)
    {
        return Err(rejected(
            "invalid_participant_mute",
            "participant_id is invalid.",
        ));
    }
    Ok(update)
}

fn require_target_membership(
    participant: &Participant,
    room_id: &str,
    participant_id: &str,
) -> Result<(), PersistenceError> {
    if participant.room_id != room_id || participant.participant_id != participant_id {
        return Err(rejected(
            "stored_participant_invalid",
            "Stored participant identity does not match its room key.",
        ));
    }
    if participant.status != ParticipantStatus::Joined {
        return Err(rejected(
            "participant_not_active",
            "Only a joined room participant can be muted.",
        ));
    }
    Ok(())
}

async fn load_agent_session_for_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<DurableAgentSession, PersistenceError> {
    let rows = sqlx::query(
        "SELECT session_json FROM agent_sessions WHERE room_id = ? ORDER BY session_id LIMIT 65",
    )
    .bind(room_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() > 64 {
        return Err(rejected(
            "agent_session_capacity",
            "This room exceeds its Agent Session capacity.",
        ));
    }
    let mut matching = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str::<DurableAgentSession>(
                row.get::<String, _>("session_json").as_str(),
            )
            .map_err(PersistenceError::from)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|session| session.public.participant_id == participant_id);
    let session = matching.next().ok_or_else(|| {
        rejected(
            "agent_session_missing",
            "The canonical Agent participant has no Agent Session authority.",
        )
    })?;
    if matching.next().is_some() {
        return Err(rejected(
            "agent_session_ambiguous",
            "More than one Agent Session owns the same room participant.",
        ));
    }
    Ok(session)
}

async fn muted_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
) -> Result<RoomEvent, PersistenceError> {
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &principal.room_id).await?,
        created_at: Utc::now(),
        room_id: principal.room_id.clone(),
        event_type: "participant_muted".to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some(participant.participant_type.clone()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::from([("muted".to_owned(), json!(participant.muted))]),
    })
}

fn stored_turn_invalid() -> PersistenceError {
    rejected(
        "stored_turn_authority_invalid",
        "Stored Agent Session turn authority is inconsistent.",
    )
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
