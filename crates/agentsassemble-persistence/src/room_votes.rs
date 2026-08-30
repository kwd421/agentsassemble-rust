use agentsassemble_domain::{
    AuthenticatedPrincipal, Participant, RoomEvent, VoteCast, VoteCommand, prepare_vote_event,
    resolve_vote_choice, vote_deadline_at,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError,
    message_attachments::{bind_message_attachments, prepare_message_attachment_bindings},
    room_turns::support::insert_event,
};

struct StoredVote {
    poll: RoomEvent,
    options: Vec<String>,
    deadline_at: Option<DateTime<Utc>>,
    tallies: Vec<u64>,
    total_votes: u64,
    manual_close_seq: Option<i64>,
}

struct VoteDefinition {
    options: Vec<String>,
    deadline_at: Option<DateTime<Utc>>,
}

pub(crate) async fn apply_vote_command(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    command: VoteCommand,
    sequence: i64,
    now: DateTime<Utc>,
) -> Result<RoomEvent, PersistenceError> {
    match command {
        VoteCommand::Create(create) => {
            let attachment_ids = create.attachment_ids.clone();
            let attachments = prepare_message_attachment_bindings(
                transaction,
                principal,
                &attachment_ids,
                now.timestamp(),
            )
            .await?;
            let command = VoteCommand::Create(create);
            let mut event = prepare_vote_event(principal, participant, &command, sequence, now)
                .map_err(|error| rejection(&error))?;
            if !attachments.is_empty() {
                event
                    .extra
                    .insert("attachments".to_owned(), serde_json::to_value(attachments)?);
            }
            insert_event(transaction, &event).await?;
            insert_vote_state(transaction, &event).await?;
            bind_message_attachments(
                transaction,
                principal,
                &attachment_ids,
                sequence,
                now.timestamp(),
            )
            .await?;
            Ok(event)
        }
        VoteCommand::Cast(cast) => {
            let mut stored = load_vote(transaction, &principal.room_id, &cast.vote_id).await?;
            require_open(&stored, now)?;
            let choice = resolve_vote_choice(&cast.choice, &stored.options)
                .ok_or_else(invalid_vote_choice)?;
            let choice_index = stored
                .options
                .iter()
                .position(|option| option == &choice)
                .ok_or_else(invalid_vote_state)?;
            let canonical = VoteCommand::Cast(VoteCast {
                vote_id: cast.vote_id,
                choice,
            });
            let event = prepare_vote_event(principal, participant, &canonical, sequence, now)
                .map_err(|error| rejection(&error))?;
            insert_event(transaction, &event).await?;
            replace_ballot(transaction, participant, &mut stored, choice_index).await?;
            save_vote_state(transaction, &stored).await?;
            Ok(event)
        }
        VoteCommand::Withdraw(reference) => {
            let mut stored = load_vote(transaction, &principal.room_id, &reference.vote_id).await?;
            require_open(&stored, now)?;
            let command = VoteCommand::Withdraw(reference);
            let event = prepare_vote_event(principal, participant, &command, sequence, now)
                .map_err(|error| rejection(&error))?;
            insert_event(transaction, &event).await?;
            remove_ballot(transaction, participant, &mut stored).await?;
            save_vote_state(transaction, &stored).await?;
            Ok(event)
        }
        VoteCommand::Close(reference) => {
            let mut stored = load_vote(transaction, &principal.room_id, &reference.vote_id).await?;
            require_open(&stored, now)?;
            if !principal.is_operator
                && stored.poll.actor.participant_id != participant.participant_id
            {
                return Err(rejected(
                    "permission_denied",
                    "Only the vote creator or room operator can close this vote.",
                ));
            }
            let command = VoteCommand::Close(reference);
            let event = prepare_vote_event(principal, participant, &command, sequence, now)
                .map_err(|error| rejection(&error))?;
            insert_event(transaction, &event).await?;
            stored.manual_close_seq = Some(sequence);
            save_vote_state(transaction, &stored).await?;
            Ok(event)
        }
    }
}

async fn insert_vote_state(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    let definition = poll_definition(event)?;
    let tallies = vec![0_u64; definition.options.len()];
    sqlx::query(
        "INSERT INTO room_vote_states(room_id, vote_id, poll_seq, tallies_json, total_votes) VALUES (?, ?, ?, ?, 0)",
    )
    .bind(&event.room_id)
    .bind(&event.id)
    .bind(event.seq)
    .bind(serde_json::to_string(&tallies)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_vote(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    vote_id: &str,
) -> Result<StoredVote, PersistenceError> {
    let row = sqlx::query(
        "SELECT state.poll_seq, state.tallies_json, state.total_votes, state.manual_close_seq, event.event_json FROM room_vote_states AS state JOIN room_events AS event ON event.room_id = state.room_id AND event.seq = state.poll_seq WHERE state.room_id = ? AND state.vote_id = ?",
    )
    .bind(room_id)
    .bind(vote_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(vote_missing)?;
    let poll: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
    if poll.id != vote_id || poll.room_id != room_id || poll.seq != row.get::<i64, _>("poll_seq") {
        return Err(invalid_vote_state());
    }
    let definition = poll_definition(&poll)?;
    let tallies: Vec<u64> = serde_json::from_str(row.get::<String, _>("tallies_json").as_str())?;
    let total_votes =
        u64::try_from(row.get::<i64, _>("total_votes")).map_err(|_| invalid_vote_state())?;
    if tallies.len() != definition.options.len()
        || tallies
            .iter()
            .try_fold(0_u64, |total, value| total.checked_add(*value))
            != Some(total_votes)
    {
        return Err(invalid_vote_state());
    }
    let manual_close_seq = row.get::<Option<i64>, _>("manual_close_seq");
    if let Some(close_seq) = manual_close_seq {
        validate_close_event(transaction, room_id, vote_id, poll.seq, close_seq).await?;
    }
    Ok(StoredVote {
        poll,
        options: definition.options,
        deadline_at: definition.deadline_at,
        tallies,
        total_votes,
        manual_close_seq,
    })
}

fn poll_definition(event: &RoomEvent) -> Result<VoteDefinition, PersistenceError> {
    if event.event_type != "message_final" || event.message_kind.as_deref() != Some("vote") {
        return Err(invalid_vote_state());
    }
    let question = event.extra.get("vote_question").and_then(Value::as_str);
    let options = event.extra.get("vote_options").and_then(Value::as_array);
    let duration = event
        .extra
        .get("vote_duration_seconds")
        .and_then(Value::as_u64);
    let deadline = event.extra.get("vote_deadline_at").and_then(Value::as_str);
    let (Some(question), Some(options), Some(duration), Some(deadline)) =
        (question, options, duration, deadline)
    else {
        return Err(invalid_vote_state());
    };
    let payload = json!({
        "kind": "vote",
        "vote_question": question,
        "vote_options": options,
        "vote_duration_seconds": duration,
    });
    let VoteCommand::Create(definition) =
        VoteCommand::from_payload(&payload).map_err(|_| invalid_vote_state())?
    else {
        return Err(invalid_vote_state());
    };
    let raw_options = options
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(invalid_vote_state)?;
    let duration = u32::try_from(duration).map_err(|_| invalid_vote_state())?;
    let deadline_at = vote_deadline_at(event.created_at, duration);
    let expected_deadline = deadline_at
        .map(|value| value.to_rfc3339())
        .unwrap_or_default();
    if definition.question != question
        || definition.options != raw_options
        || definition.duration_seconds != duration
        || deadline != expected_deadline
    {
        return Err(invalid_vote_state());
    }
    Ok(VoteDefinition {
        options: definition.options,
        deadline_at,
    })
}

async fn validate_close_event(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    vote_id: &str,
    poll_seq: i64,
    close_seq: i64,
) -> Result<(), PersistenceError> {
    if close_seq <= poll_seq {
        return Err(invalid_vote_state());
    }
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq = ?",
    )
    .bind(room_id)
    .bind(close_seq)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(invalid_vote_state)?;
    let event: RoomEvent = serde_json::from_str(&encoded)?;
    if event.room_id != room_id
        || event.seq != close_seq
        || event.event_type != "message_final"
        || event.message_kind.as_deref() != Some("vote_close")
        || event.extra.get("vote_id").and_then(Value::as_str) != Some(vote_id)
    {
        return Err(invalid_vote_state());
    }
    Ok(())
}

fn require_open(stored: &StoredVote, now: DateTime<Utc>) -> Result<(), PersistenceError> {
    if stored.deadline_at.is_some_and(|deadline| now >= deadline) {
        return Err(rejected("vote_expired", "This vote has ended."));
    }
    if stored.manual_close_seq.is_some() {
        return Err(rejected("vote_closed", "This vote has ended."));
    }
    Ok(())
}

async fn replace_ballot(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
    stored: &mut StoredVote,
    choice_index: usize,
) -> Result<(), PersistenceError> {
    let previous = load_ballot(transaction, participant, stored).await?;
    if previous == Some(choice_index) {
        return Ok(());
    }
    if let Some(previous) = previous {
        stored.tallies[previous] = stored.tallies[previous]
            .checked_sub(1)
            .ok_or_else(invalid_vote_state)?;
    } else {
        stored.total_votes = stored
            .total_votes
            .checked_add(1)
            .ok_or_else(invalid_vote_state)?;
    }
    stored.tallies[choice_index] = stored.tallies[choice_index]
        .checked_add(1)
        .ok_or_else(invalid_vote_state)?;
    sqlx::query(
        "INSERT INTO room_vote_ballots(room_id, vote_id, participant_id, choice_index) VALUES (?, ?, ?, ?) ON CONFLICT(room_id, vote_id, participant_id) DO UPDATE SET choice_index = excluded.choice_index",
    )
    .bind(&stored.poll.room_id)
    .bind(&stored.poll.id)
    .bind(&participant.participant_id)
    .bind(i64::try_from(choice_index).map_err(|_| invalid_vote_state())?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn remove_ballot(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
    stored: &mut StoredVote,
) -> Result<(), PersistenceError> {
    let Some(previous) = load_ballot(transaction, participant, stored).await? else {
        return Ok(());
    };
    stored.tallies[previous] = stored.tallies[previous]
        .checked_sub(1)
        .ok_or_else(invalid_vote_state)?;
    stored.total_votes = stored
        .total_votes
        .checked_sub(1)
        .ok_or_else(invalid_vote_state)?;
    sqlx::query(
        "DELETE FROM room_vote_ballots WHERE room_id = ? AND vote_id = ? AND participant_id = ?",
    )
    .bind(&stored.poll.room_id)
    .bind(&stored.poll.id)
    .bind(&participant.participant_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_ballot(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
    stored: &StoredVote,
) -> Result<Option<usize>, PersistenceError> {
    let value = sqlx::query_scalar::<_, i64>(
        "SELECT choice_index FROM room_vote_ballots WHERE room_id = ? AND vote_id = ? AND participant_id = ?",
    )
    .bind(&stored.poll.room_id)
    .bind(&stored.poll.id)
    .bind(&participant.participant_id)
    .fetch_optional(&mut **transaction)
    .await?;
    value
        .map(|value| {
            usize::try_from(value)
                .ok()
                .filter(|index| *index < stored.options.len())
                .ok_or_else(invalid_vote_state)
        })
        .transpose()
}

async fn save_vote_state(
    transaction: &mut Transaction<'_, Sqlite>,
    stored: &StoredVote,
) -> Result<(), PersistenceError> {
    let result = sqlx::query(
        "UPDATE room_vote_states SET tallies_json = ?, total_votes = ?, manual_close_seq = ? WHERE room_id = ? AND vote_id = ? AND poll_seq = ?",
    )
    .bind(serde_json::to_string(&stored.tallies)?)
    .bind(i64::try_from(stored.total_votes).map_err(|_| invalid_vote_state())?)
    .bind(stored.manual_close_seq)
    .bind(&stored.poll.room_id)
    .bind(&stored.poll.id)
    .bind(stored.poll.seq)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(invalid_vote_state());
    }
    Ok(())
}

fn vote_missing() -> PersistenceError {
    rejected("vote_not_found", "This vote does not exist in the room.")
}

fn invalid_vote_choice() -> PersistenceError {
    rejected("invalid_vote_choice", "The vote choice is invalid.")
}

fn invalid_vote_state() -> PersistenceError {
    rejected("invalid_state", "Stored vote authority is invalid.")
}

fn rejection(error: &agentsassemble_domain::CommandRejection) -> PersistenceError {
    rejected(error.code, &error.message)
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
