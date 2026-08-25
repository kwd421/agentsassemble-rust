use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, DurableAgentSession, ParticipantStatus, RoomEvent,
    RoomRandomRequest, RoomRandomResult, RoomSettings, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle::load_session,
    authority::active_room_for_principal,
    command_admission::admit_non_lifecycle_command,
    room_turns::support::{insert_event, load_active_room, load_participant, next_sequence},
    room_write_budget::{command_size, reserve_room_write_budget},
    turn_authority::active_turn_authority,
};

const MAX_RANDOM_RESULTS_PER_TURN: i64 = 32;

#[derive(Debug, Clone)]
pub struct RoomRandomCommit {
    pub event: RoomEvent,
    pub result: RoomRandomResult,
}

#[derive(Debug)]
pub struct ProviderRoomRandomCommit<'a> {
    pub room_id: &'a str,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub input_up_to_seq: i64,
    pub result_id: &'a str,
    pub request: &'a RoomRandomRequest,
    pub result: &'a RoomRandomResult,
}

impl SqliteStore {
    /// Commits one human room-random command through normal command replay authority.
    ///
    /// # Errors
    ///
    /// Returns replay, permission, room-mode, participant, validation, or storage failures.
    pub async fn execute_room_random_command(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
        result: &RoomRandomResult,
    ) -> Result<CommandOutcome, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        let _ = active_room_for_principal(&mut transaction, principal).await?;
        if !principal.capabilities.room_random {
            return Err(rejected(
                "permission_denied",
                "This room session cannot use room randomness.",
            ));
        }
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
            command_size(request_id, action, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let request = RoomRandomRequest::parse(action, payload)
            .map_err(|error| rejected("invalid_room_random_request", error.message))?;
        require_tabletop(&mut transaction, &principal.room_id).await?;
        let participant = load_participant(
            &mut transaction,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        require_participant(participant.status, participant.muted)?;
        validate_result(&request, result)?;
        let event = random_event(
            &mut transaction,
            &principal.room_id,
            &principal.participant_id,
            &participant.display_name,
            "",
            &format!("result-{}", Uuid::new_v4().simple()),
            result,
        )
        .await?;
        let response = json!({"event": event, "event_seq": event.seq});
        insert_event(&mut transaction, &event).await?;
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(request_id)
        .bind(action)
        .bind(payload_hash)
        .bind(serde_json::to_string(&response)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CommandOutcome {
            result: response,
            event: event.clone(),
            events: vec![event],
            deduplicated: false,
        })
    }

    /// Commits one provider `RoomPortal` random result under its active durable turn.
    ///
    /// # Errors
    ///
    /// Returns when the room/session/turn/input tuple, mode, budget, or result is invalid.
    pub async fn commit_provider_room_random(
        &self,
        commit: ProviderRoomRandomCommit<'_>,
    ) -> Result<RoomRandomCommit, PersistenceError> {
        let ProviderRoomRandomCommit {
            room_id,
            session_id,
            turn_id,
            input_up_to_seq,
            result_id,
            request,
            result,
        } = commit;
        if !valid_result_id(result_id) {
            return Err(rejected(
                "invalid_room_result",
                "Room tool result id is invalid.",
            ));
        }
        validate_result(request, result)?;
        let mut transaction = self.pool.begin().await?;
        let (_, settings) = load_active_room(&mut transaction, room_id).await?;
        if settings.tool_mode != "tabletop" {
            return Err(rejected(
                "room_random_unavailable",
                "Room randomness is available only in tabletop mode.",
            ));
        }
        let session = load_session(&mut transaction, room_id, session_id).await?;
        require_provider_authority(&session, turn_id, input_up_to_seq)?;
        let participant =
            load_participant(&mut transaction, room_id, &session.public.participant_id).await?;
        require_participant(participant.status, participant.muted)?;
        let committed = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_turn_tool_results WHERE room_id = ? AND session_id = ? AND turn_id = ?",
        )
        .bind(room_id)
        .bind(session_id)
        .bind(turn_id)
        .fetch_one(&mut *transaction)
        .await?;
        if committed >= MAX_RANDOM_RESULTS_PER_TURN {
            return Err(rejected(
                "room_random_budget_exhausted",
                "This Agent Session turn reached its room-random result limit.",
            ));
        }
        let payload = request.canonical_payload();
        reserve_room_write_budget(
            &mut transaction,
            room_id,
            command_size(result_id, request.room_action(), &payload)?,
        )
        .await?;
        let event = random_event(
            &mut transaction,
            room_id,
            &session.public.participant_id,
            &session.public.display_name,
            turn_id,
            result_id,
            result,
        )
        .await?;
        insert_event(&mut transaction, &event).await?;
        sqlx::query(
            "INSERT INTO room_turn_tool_results(room_id, session_id, turn_id, result_id, event_seq) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(room_id)
        .bind(session_id)
        .bind(turn_id)
        .bind(result_id)
        .bind(event.seq)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(RoomRandomCommit {
            event,
            result: result.clone(),
        })
    }
}

async fn require_tabletop(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<(), PersistenceError> {
    let encoded =
        sqlx::query_scalar::<_, String>("SELECT settings_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_one(&mut **transaction)
            .await?;
    let settings: RoomSettings = serde_json::from_str(&encoded)?;
    if settings.tool_mode == "tabletop" {
        Ok(())
    } else {
        Err(rejected(
            "room_random_unavailable",
            "Room randomness is available only in tabletop mode.",
        ))
    }
}

fn require_participant(status: ParticipantStatus, muted: bool) -> Result<(), PersistenceError> {
    if status != ParticipantStatus::Joined {
        return Err(rejected(
            "session_revoked",
            "This participant is no longer in the room.",
        ));
    }
    if muted {
        return Err(rejected(
            "participant_muted",
            "Muted participants cannot publish room results.",
        ));
    }
    Ok(())
}

fn require_provider_authority(
    session: &DurableAgentSession,
    turn_id: &str,
    input_up_to_seq: i64,
) -> Result<(), PersistenceError> {
    if active_turn_authority(session) != Ok(true)
        || session.public.active_turn_id != turn_id
        || session.input_up_to_seq != input_up_to_seq
        || session.public.status != "attached"
        || session.public.runtime_status != "busy"
        || !session.public.enabled
        || !session.public.provider_session_active
        || session.public.process_ownership != "server"
        || session.runtime_handle_id.is_empty()
        || session.runtime_owner_id.is_empty()
        || session.runtime_lease_token.is_empty()
    {
        return Err(rejected(
            "stale_provider_turn",
            "Room tool result no longer matches the active provider turn.",
        ));
    }
    Ok(())
}

fn validate_result(
    request: &RoomRandomRequest,
    result: &RoomRandomResult,
) -> Result<(), PersistenceError> {
    let valid = match (request, result) {
        (
            RoomRandomRequest::Roll {
                notation,
                count,
                sides,
                modifier,
                ..
            },
            RoomRandomResult::RollDice {
                notation: result_notation,
                rolls,
                modifier: result_modifier,
                total,
            },
        ) => {
            result_notation == notation
                && rolls.len() == *count as usize
                && rolls.iter().all(|roll| (1..=*sides).contains(roll))
                && result_modifier == modifier
                && *total
                    == rolls.iter().map(|roll| i64::from(*roll)).sum::<i64>() + i64::from(*modifier)
        }
        (
            RoomRandomRequest::Choose { options, .. },
            RoomRandomResult::ChooseRandom {
                choice,
                index,
                option_count,
                options: result_options,
            },
        ) => {
            result_options == options
                && *option_count == options.len()
                && options
                    .get(*index)
                    .is_some_and(|selected| selected == choice)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(rejected(
            "invalid_room_result",
            "Room randomness result does not match its validated request.",
        ))
    }
}

async fn random_event(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    source_participant_id: &str,
    source_display_name: &str,
    source_turn_id: &str,
    result_id: &str,
    result: &RoomRandomResult,
) -> Result<RoomEvent, PersistenceError> {
    let (display_name, content, kind, operation, details) = match result {
        RoomRandomResult::RollDice {
            notation,
            rolls,
            modifier,
            total,
        } => (
            "주사위 결과",
            format!(
                "{source_display_name} · {notation} → {total} (굴림: {})",
                rolls
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "dice_roll",
            "roll_dice",
            json!({
                "notation": notation,
                "rolls": rolls,
                "modifier": modifier,
                "total": total,
            }),
        ),
        RoomRandomResult::ChooseRandom {
            choice,
            index,
            option_count,
            ..
        } => (
            "무작위 선택 결과",
            format!(
                "{source_display_name} · 무작위 선택 → 「{choice}」 ({}/{option_count})",
                index + 1
            ),
            "random_choice",
            "choose_random",
            json!({
                "choice": choice,
                "index": index,
                "option_count": option_count,
            }),
        ),
    };
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, room_id).await?,
        created_at: Utc::now(),
        room_id: room_id.to_owned(),
        event_type: "message_final".to_owned(),
        actor: Actor {
            participant_id: "room-system".to_owned(),
            participant_type: "system".to_owned(),
        },
        participant_id: Some("room-system".to_owned()),
        participant_type: Some("system".to_owned()),
        actor_id: Some("room-system".to_owned()),
        actor_type: Some("system".to_owned()),
        display_name: Some(display_name.to_owned()),
        content: Some(content),
        message_kind: Some("system".to_owned()),
        extra: BTreeMap::from([
            ("message_source".to_owned(), json!("room_tool_result")),
            ("room_result_id".to_owned(), json!(result_id)),
            ("room_result_kind".to_owned(), json!(kind)),
            ("operation".to_owned(), json!(operation)),
            ("source_turn_id".to_owned(), json!(source_turn_id)),
            (
                "source_participant_id".to_owned(),
                json!(source_participant_id),
            ),
            ("details".to_owned(), details),
        ]),
    })
}

fn valid_result_id(value: &str) -> bool {
    value.len() == 39
        && value.starts_with("result-")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
