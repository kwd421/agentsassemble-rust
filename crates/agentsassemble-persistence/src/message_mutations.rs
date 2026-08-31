use agentsassemble_domain::{
    AuthenticatedPrincipal, MessageDelete, MessageEdit, MutableMessageKind, RoomEvent,
    authorize_message_delete, authorize_message_edit, canonical_payload_hash, has_visible_text,
    prepare_deleted_message, prepare_message_deleted_event, prepare_message_updated_event,
    prepare_updated_message, redact_deleted_vote_transition,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    CommandOutcome, HumanSessionAuthorization, PersistenceError, SqliteStore,
    authority::load_active_participant,
    command_admission::{admit_non_lifecycle_command, store_command_result},
    human_session_authority::revalidate_human_session,
    message_attachments::{delete_bound_message_attachments, message_attachments_from_event},
    message_pins::remove_lobby_message_pin,
    message_search_index::{remove_lobby_message_index, replace_lobby_message_index},
    room_turns::remove_pending_input_reference,
    room_turns::support::{
        insert_event, load_event, load_participant, next_sequence, replace_event,
    },
    room_votes::delete_vote_projection,
    room_write_budget::command_size,
};

impl SqliteStore {
    /// Commits one current local-human lobby edit or deletion atomically.
    ///
    /// # Errors
    ///
    /// Returns authorization, target-state, idempotency, or storage failures without partial state.
    pub async fn execute_message_mutation(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let _ = load_active_participant(
            &mut transaction,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        let outcome =
            execute_message_mutation_in(&mut transaction, principal, request_id, action, payload)
                .await?;
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Commits one admitted-human lobby edit or deletion with session revalidation in-transaction.
    ///
    /// # Errors
    ///
    /// Returns stale-session, authorization, target-state, idempotency, or storage failures.
    pub async fn execute_human_session_message_mutation(
        &self,
        authorization: &HumanSessionAuthorization,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        let outcome = execute_message_mutation_in(
            &mut transaction,
            current.principal(),
            request_id,
            action,
            payload,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn execute_message_mutation_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: &str,
    payload: &Value,
) -> Result<CommandOutcome, PersistenceError> {
    let payload_hash = canonical_payload_hash(payload);
    if let Some(outcome) = admit_non_lifecycle_command(
        transaction,
        &principal.room_id,
        &principal.principal_id,
        request_id,
        action,
        &payload_hash,
        command_size(request_id, action, payload)?,
    )
    .await?
    {
        return Ok(outcome);
    }
    let (target_id, edit) = match action {
        "message.edit" => {
            let edit = MessageEdit::from_payload(payload).map_err(rejection)?;
            (edit.event_id.clone(), Some(edit))
        }
        "message.delete" => {
            let deletion = MessageDelete::from_payload(payload).map_err(rejection)?;
            (deletion.event_id, None)
        }
        _ => {
            return Err(rejected(
                "unsupported_action",
                format!("Unsupported room command: {action}"),
            ));
        }
    };
    let target = load_event(transaction, &principal.room_id, &target_id)
        .await?
        .ok_or_else(|| rejected("message_not_found", "Message was not found."))?;
    let now = Utc::now();
    let sequence = next_sequence(transaction, &principal.room_id).await?;
    let (_updated, mutation, result) = if let Some(edit) = edit {
        edit_message(transaction, principal, &target, edit, sequence, now).await?
    } else {
        delete_message(transaction, principal, &target, sequence, now).await?
    };
    insert_event(transaction, &mutation).await?;
    store_command_result(
        transaction,
        principal,
        request_id,
        action,
        &payload_hash,
        &result,
    )
    .await?;
    Ok(CommandOutcome {
        result,
        event: mutation.clone(),
        events: vec![mutation],
        deduplicated: false,
    })
}

async fn edit_message(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    target: &RoomEvent,
    edit: MessageEdit,
    sequence: i64,
    now: DateTime<Utc>,
) -> Result<(RoomEvent, RoomEvent, Value), PersistenceError> {
    authorize_message_edit(principal, target).map_err(rejection)?;
    if !has_visible_text(&edit.content) && message_attachments_from_event(target)?.is_empty() {
        return Err(rejected(
            "empty",
            "Message content or an attachment is required.",
        ));
    }
    let updated = prepare_updated_message(target, edit.content.clone(), now);
    replace_event(transaction, &updated).await?;
    replace_lobby_message_index(transaction, &updated).await?;
    let mutation = prepare_message_updated_event(principal, target, edit.content, sequence, now);
    let result = json!({"message": updated, "event": mutation, "event_seq": sequence});
    Ok((updated, mutation, result))
}

async fn delete_message(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    target: &RoomEvent,
    sequence: i64,
    now: DateTime<Utc>,
) -> Result<(RoomEvent, RoomEvent, Value), PersistenceError> {
    let author = load_participant(
        transaction,
        &principal.room_id,
        &target.actor.participant_id,
    )
    .await?;
    let kind = authorize_message_delete(principal, target, &author).map_err(rejection)?;
    let attachment_ids = delete_bound_message_attachments(transaction, target).await?;
    remove_lobby_message_pin(transaction, &principal.room_id, &target.id).await?;
    remove_lobby_message_index(transaction, target).await?;
    if kind == MutableMessageKind::Vote {
        redact_vote_transitions(transaction, target, now).await?;
        delete_vote_projection(transaction, &principal.room_id, &target.id, target.seq).await?;
    }
    remove_pending_input_reference(transaction, &principal.room_id, &target.id).await?;
    let updated = prepare_deleted_message(target, kind, now);
    replace_event(transaction, &updated).await?;
    let mutation = prepare_message_deleted_event(principal, target, sequence, now);
    let result = json!({
        "message": updated,
        "event": mutation,
        "event_seq": sequence,
        "target_event_id": target.id,
        "attachment_ids": attachment_ids,
    });
    Ok((updated, mutation, result))
}

async fn redact_vote_transitions(
    transaction: &mut Transaction<'_, Sqlite>,
    poll: &RoomEvent,
    deleted_at: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let rows = sqlx::query(
        "SELECT seq, event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.vote_id') = ? ORDER BY seq",
    )
    .bind(&poll.room_id)
    .bind(&poll.id)
    .fetch_all(&mut **transaction)
    .await?;
    for row in rows {
        let sequence = row.get::<i64, _>("seq");
        let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
        if event.room_id != poll.room_id
            || event.seq != sequence
            || event.event_type != "message_final"
            || !matches!(
                event.message_kind.as_deref(),
                Some("vote_cast" | "vote_withdraw" | "vote_close")
            )
            || event.extra.get("vote_id").and_then(Value::as_str) != Some(poll.id.as_str())
            || event.extra.get("message_deleted") == Some(&Value::Bool(true))
        {
            return Err(rejected(
                "invalid_state",
                "Stored vote transition is inconsistent.",
            ));
        }
        replace_event(
            transaction,
            &redact_deleted_vote_transition(&event, deleted_at),
        )
        .await?;
    }
    Ok(())
}

fn rejection(error: agentsassemble_domain::CommandRejection) -> PersistenceError {
    rejected(error.code, error.message)
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "message_mutation_tests.rs"]
mod tests;
