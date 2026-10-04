//! Immutable ordered routing identity belongs to the source event, not a mutable profile.
use agentsassemble_domain::{DurableAgentSession, RoomEvent};
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, room_turns::support::rejected};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS ordered_input_routes (room_id TEXT NOT NULL, event_seq INTEGER NOT NULL, selected_session_id TEXT NOT NULL CHECK(length(selected_session_id) > 0), directly_addressed INTEGER NOT NULL CHECK(directly_addressed IN (0, 1)), PRIMARY KEY(room_id, event_seq), FOREIGN KEY(room_id, event_seq) REFERENCES room_events(room_id, seq) ON DELETE CASCADE)";

pub(crate) async fn record_first(
    tx: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
    selected_session_id: &str,
    directly_addressed: bool,
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO ordered_input_routes(room_id, event_seq, selected_session_id, directly_addressed) VALUES (?, ?, ?, ?) ON CONFLICT(room_id, event_seq) DO NOTHING")
        .bind(&event.room_id).bind(event.seq).bind(selected_session_id).bind(directly_addressed)
        .execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn directly_addressed(
    tx: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<bool, PersistenceError> {
    sqlx::query_scalar(
        "SELECT directly_addressed FROM ordered_input_routes WHERE room_id = ? AND event_seq = ?",
    )
    .bind(&event.room_id)
    .bind(event.seq)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        rejected(
            "ordered_route_missing",
            "The ordered input has no durable routing identity.",
        )
    })
}

/// One-time upgrade of retained inputs using identities in canonical history at routing time.
/// Never interprets an old mention using today's names. Missing history fails the upgrade.
pub(crate) async fn upgrade_retained_inputs(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    let sessions: Vec<String> =
        sqlx::query_scalar("SELECT session_json FROM agent_sessions ORDER BY room_id, session_id")
            .fetch_all(&mut **tx)
            .await?;
    for json in sessions {
        let session: DurableAgentSession = serde_json::from_str(&json)?;
        for input in session
            .pending_inputs
            .iter()
            .chain(&session.inflight_inputs)
        {
            if input.delivery_kind
                != agentsassemble_domain::RoomInputDeliveryKind::OrderedObservation
            {
                continue;
            }
            let event = crate::room_turns::support::load_event(
                tx,
                &session.public.room_id,
                &input.event_id,
            )
            .await?
            .ok_or_else(crate::agent_lifecycle::invalid_turn_queue)?;
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ordered_input_routes WHERE room_id = ? AND event_seq = ?)")
                .bind(&event.room_id).bind(event.seq).fetch_one(&mut **tx).await?;
            if exists {
                continue;
            }
            let direct = historical_direct_target(tx, &event).await?;
            // For undirected legacy inputs, retain the earliest recorded holder (or still-pending
            // custody). Named inputs retain the exact historical target even after a decline.
            let first_holder: Option<String> = sqlx::query_scalar("SELECT json_extract(event_json, '$.session_id') FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.type') = 'turn_started' AND json_extract(event_json, '$.source_event_id') = ? ORDER BY seq LIMIT 1")
                .bind(&event.room_id).bind(&event.id).fetch_optional(&mut **tx).await?;
            let selected = direct
                .as_deref()
                .or(first_holder.as_deref())
                .unwrap_or(&session.public.session_id);
            record_first(tx, &event, selected, direct.is_some()).await?;
        }
    }
    Ok(())
}

async fn historical_direct_target(
    tx: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<Option<String>, PersistenceError> {
    let names: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT session_id, participant_id, display_name FROM (SELECT json_extract(event_json, '$.agent_session.session_id') AS session_id, json_extract(event_json, '$.agent_session.participant_id') AS participant_id, json_extract(event_json, '$.agent_session.display_name') AS display_name, row_number() OVER (PARTITION BY json_extract(event_json, '$.agent_session.session_id') ORDER BY seq DESC) AS latest FROM room_events WHERE room_id = ? AND seq < ? AND json_extract(event_json, '$.type') IN ('agent_session_created', 'agent_session_state')) WHERE latest = 1",
    ).bind(&event.room_id).bind(event.seq).fetch_all(&mut **tx).await?;
    if names.is_empty() {
        return Err(rejected(
            "ordered_route_history_missing",
            "Cannot recover ordered routing without historical Agent Session identities.",
        ));
    }
    let names = names
        .iter()
        .filter(|(id, participant, _)| {
            id != &event.actor.participant_id && participant != &event.actor.participant_id
        })
        .collect::<Vec<_>>();
    let structured = event
        .extra
        .get("target_agent_id")
        .and_then(serde_json::Value::as_str)
        .filter(|target| names.iter().any(|(id, _, _)| id == target))
        .map(str::to_owned);
    Ok(crate::room_turns::routing::last_direct_target_for_names(
        event.content.as_deref().unwrap_or_default(),
        names
            .iter()
            .map(|(id, _, name)| (id.as_str(), name.as_str())),
    )
    .or(structured))
}
