use agentsassemble_domain::{
    AuthenticatedPrincipal, InviteScope, MAX_MESSAGE_SEARCH_CURSOR_BYTES, MESSAGE_CONTEXT_RADIUS,
    MESSAGE_SEARCH_PAGE_SIZE, RoomEvent, RoomMessageContext, RoomMessageSearchPage,
    RoomMessageSearchResult, casefold_message_search_text, clean_message_search_query,
    compact_casefolded_message_search_text, is_message_event_id, public_event_for_principal,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::SecondsFormat;
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use crate::{
    PersistenceError, RoomSessionAuthorization, SqliteStore,
    agent_lifecycle::load_session,
    message_search_index::{canonical_created_at_nanos, searchable_room_message},
    room_channels::require_text_channel,
    room_turns::support::{load_participant, provider_room_principal},
    room_user_identity::resolve_local_room_manager,
    turn_authority::require_provider_room_tool_authority,
};

const MESSAGE_CHANNEL_SQL: &str = "CASE json_extract(events.event_json, '$.type') WHEN 'channel_message_final' THEN json_extract(events.event_json, '$.channel_id') ELSE 'lobby' END";

#[derive(Debug, Clone, Copy)]
pub struct ProviderMessageSearchAuthority<'a> {
    pub room_id: &'a str,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub input_up_to_seq: i64,
    pub turn_generation: u64,
    pub execution_id: &'a str,
}

impl SqliteStore {
    /// Searches current room text channels as the current local room manager.
    ///
    /// # Errors
    ///
    /// Rejects stale authority, invalid query/cursor input, or inconsistent stored projections.
    pub async fn search_local_messages(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        channel_id: &str,
        query: &str,
        cursor: &str,
    ) -> Result<RoomMessageSearchPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (_, principal) =
            resolve_local_room_manager(&mut transaction, room_id, user_id, participant_id).await?;
        let page = search_in(&mut transaction, &principal, channel_id, query, cursor).await?;
        transaction.commit().await?;
        Ok(page)
    }

    /// Searches current room text channels while a room session remains authorized.
    ///
    /// # Errors
    ///
    /// Rejects revoked history permission, invalid input, or inconsistent stored projections.
    pub async fn search_room_session_messages(
        &self,
        expected: &RoomSessionAuthorization,
        channel_id: &str,
        query: &str,
        cursor: &str,
    ) -> Result<RoomMessageSearchPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = expected
            .mutation_authority()
            .resolve(&mut transaction)
            .await?;
        let page = search_in(&mut transaction, &principal, channel_id, query, cursor).await?;
        transaction.commit().await?;
        Ok(page)
    }

    /// Reads one current text channel context as the current local room manager.
    ///
    /// # Errors
    ///
    /// Rejects stale authority, unknown targets, or inconsistent stored projections.
    pub async fn local_message_context(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        channel_id: &str,
        event_id: &str,
    ) -> Result<RoomMessageContext, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (_, principal) =
            resolve_local_room_manager(&mut transaction, room_id, user_id, participant_id).await?;
        let context = context_in(&mut transaction, &principal, channel_id, event_id).await?;
        transaction.commit().await?;
        Ok(context)
    }

    /// Reads one current text channel context while a room session remains authorized.
    ///
    /// # Errors
    ///
    /// Rejects revoked history permission, unknown targets, or inconsistent stored projections.
    pub async fn room_session_message_context(
        &self,
        expected: &RoomSessionAuthorization,
        channel_id: &str,
        event_id: &str,
    ) -> Result<RoomMessageContext, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = expected
            .mutation_authority()
            .resolve(&mut transaction)
            .await?;
        let context = context_in(&mut transaction, &principal, channel_id, event_id).await?;
        transaction.commit().await?;
        Ok(context)
    }

    /// Searches canonical lobby history for one exact active provider turn.
    ///
    /// # Errors
    ///
    /// Rejects stale turn authority, a revoked room participant, or invalid search input.
    pub async fn search_provider_lobby_messages(
        &self,
        authority: ProviderMessageSearchAuthority<'_>,
        query: &str,
        cursor: &str,
    ) -> Result<RoomMessageSearchPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = provider_search_principal(&mut transaction, authority).await?;
        let page =
            search_authorized_in(&mut transaction, &principal.room_id, "lobby", query, cursor)
                .await?;
        transaction.commit().await?;
        Ok(page)
    }

    /// Reads bounded canonical lobby context for one exact active provider turn.
    ///
    /// # Errors
    ///
    /// Rejects stale turn authority, a revoked room participant, or an unknown target.
    pub async fn provider_lobby_message_context(
        &self,
        authority: ProviderMessageSearchAuthority<'_>,
        event_id: &str,
    ) -> Result<RoomMessageContext, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = provider_search_principal(&mut transaction, authority).await?;
        let context =
            context_authorized_in(&mut transaction, &principal, "lobby", event_id).await?;
        transaction.commit().await?;
        Ok(context)
    }
}

async fn search_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    channel_id: &str,
    query: &str,
    cursor: &str,
) -> Result<RoomMessageSearchPage, PersistenceError> {
    require_history(principal)?;
    search_authorized_in(transaction, &principal.room_id, channel_id, query, cursor).await
}

async fn search_authorized_in(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &str,
    query: &str,
    cursor: &str,
) -> Result<RoomMessageSearchPage, PersistenceError> {
    let channel_id = require_search_channel(transaction, room_id, channel_id, true).await?;
    let query = clean_message_search_query(query);
    if query.is_empty() {
        return Err(rejected("bad_request", "q is required."));
    }
    let cursor = decode_cursor(cursor)?;
    let folded = casefold_message_search_text(&query);
    let compact = compact_casefolded_message_search_text(&folded);
    let phrase = fts_phrase(&folded);
    let limit = i64::try_from(MESSAGE_SEARCH_PAGE_SIZE + 1).map_err(|_| invalid_state())?;
    let rows = if compact.chars().count() >= 3 {
        search_long(
            transaction,
            room_id,
            channel_id,
            &compact,
            &phrase,
            cursor,
            limit,
        )
        .await?
    } else {
        search_short(transaction, room_id, channel_id, &phrase, cursor, limit).await?
    };
    project_page(room_id, rows)
}

async fn search_long(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &str,
    compact: &str,
    phrase: &str,
    cursor: Option<(i64, i64)>,
    limit: i64,
) -> Result<Vec<SqliteRow>, PersistenceError> {
    let (created_at, seq) = cursor.unwrap_or((i64::MAX, i64::MAX));
    let sql = format!(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND (? = 'all' OR {MESSAGE_CHANNEL_SQL} = ?) \
         AND (instr(search.compact_text, ?) > 0 OR search.id IN (\
             SELECT rowid FROM room_message_search_phrase WHERE room_message_search_phrase MATCH ?\
         )) \
         AND (search.created_at_nanos < ? OR (search.created_at_nanos = ? AND search.event_seq < ?)) \
         ORDER BY search.created_at_nanos DESC, search.event_seq DESC LIMIT ?"
    );
    Ok(sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(room_id)
        .bind(channel_id)
        .bind(channel_id)
        .bind(compact)
        .bind(phrase)
        .bind(created_at)
        .bind(created_at)
        .bind(seq)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await?)
}

async fn search_short(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &str,
    phrase: &str,
    cursor: Option<(i64, i64)>,
    limit: i64,
) -> Result<Vec<SqliteRow>, PersistenceError> {
    let (created_at, seq) = cursor.unwrap_or((i64::MAX, i64::MAX));
    let sql = format!(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_phrase AS phrase \
         JOIN room_message_search_records AS search ON search.id = phrase.rowid \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE phrase.search_text MATCH ? AND search.room_id = ? AND (? = 'all' OR {MESSAGE_CHANNEL_SQL} = ?) \
         AND (search.created_at_nanos < ? OR (search.created_at_nanos = ? AND search.event_seq < ?)) \
         ORDER BY search.created_at_nanos DESC, search.event_seq DESC LIMIT ?"
    );
    Ok(sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(phrase)
        .bind(room_id)
        .bind(channel_id)
        .bind(channel_id)
        .bind(created_at)
        .bind(created_at)
        .bind(seq)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await?)
}

fn project_page(
    room_id: &str,
    mut rows: Vec<SqliteRow>,
) -> Result<RoomMessageSearchPage, PersistenceError> {
    let has_more = rows.len() > MESSAGE_SEARCH_PAGE_SIZE;
    rows.truncate(MESSAGE_SEARCH_PAGE_SIZE);
    let mut results = Vec::with_capacity(rows.len());
    let mut last_cursor = None;
    for row in rows {
        let (event, created_at_nanos) = checked_event(&row, room_id)?;
        let message = searchable_room_message(&event)?.ok_or_else(invalid_state)?;
        last_cursor = Some((created_at_nanos, event.seq));
        results.push(RoomMessageSearchResult {
            channel_id: message.channel_id,
            event_id: event.id,
            participant_id: event.actor.participant_id,
            seq: event.seq,
            created_at: event
                .created_at
                .to_rfc3339_opts(SecondsFormat::AutoSi, true),
            author: message.author,
            content: message.content,
            attachment_filenames: message.attachment_filenames,
        });
    }
    let next_cursor = if has_more {
        encode_cursor(last_cursor.ok_or_else(invalid_state)?)?
    } else {
        String::new()
    };
    Ok(RoomMessageSearchPage {
        results,
        next_cursor,
    })
}

async fn context_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    channel_id: &str,
    event_id: &str,
) -> Result<RoomMessageContext, PersistenceError> {
    require_history(principal)?;
    context_authorized_in(transaction, principal, channel_id, event_id).await
}

async fn context_authorized_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    channel_id: &str,
    event_id: &str,
) -> Result<RoomMessageContext, PersistenceError> {
    if !is_message_event_id(event_id) {
        return Err(rejected("bad_request", "event_id is invalid."));
    }
    let channel_id =
        require_search_channel(transaction, &principal.room_id, channel_id, false).await?;
    let target_sql = format!(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND search.event_id = ? AND {MESSAGE_CHANNEL_SQL} = ?"
    );
    let target = sqlx::query(sqlx::AssertSqlSafe(target_sql))
        .bind(&principal.room_id)
        .bind(event_id)
        .bind(channel_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(message_missing)?;
    let (target_event, _) = checked_event(&target, &principal.room_id)?;
    let _ = searchable_room_message(&target_event)?.ok_or_else(invalid_state)?;
    let radius = i64::try_from(MESSAGE_CONTEXT_RADIUS).map_err(|_| invalid_state())?;
    let mut rows = context_side(
        transaction,
        &principal.room_id,
        channel_id,
        target_event.seq,
        true,
        radius,
    )
    .await?;
    rows.reverse();
    rows.push(target);
    rows.extend(
        context_side(
            transaction,
            &principal.room_id,
            channel_id,
            target_event.seq,
            false,
            radius,
        )
        .await?,
    );
    let events = rows
        .into_iter()
        .map(|row| {
            let (event, _) = checked_event(&row, &principal.room_id)?;
            let _ = searchable_room_message(&event)?.ok_or_else(invalid_state)?;
            Ok(public_event_for_principal(&event, principal))
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    Ok(RoomMessageContext {
        channel_id: channel_id.to_owned(),
        event_id: target_event.id,
        events,
    })
}

async fn provider_search_principal(
    transaction: &mut Transaction<'_, Sqlite>,
    authority: ProviderMessageSearchAuthority<'_>,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    let session = load_session(transaction, authority.room_id, authority.session_id).await?;
    require_provider_room_tool_authority(
        transaction,
        &session,
        authority.turn_id,
        authority.input_up_to_seq,
        authority.turn_generation,
        authority.execution_id,
    )
    .await?;
    let participant = load_participant(
        transaction,
        authority.room_id,
        &session.public.participant_id,
    )
    .await?;
    provider_room_principal(&session, &participant, InviteScope::ReadOnly)
}

async fn context_side(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &str,
    target_seq: i64,
    before: bool,
    limit: i64,
) -> Result<Vec<SqliteRow>, PersistenceError> {
    let (comparison, direction) = if before { ("<", "DESC") } else { (">", "ASC") };
    let sql = format!(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND {MESSAGE_CHANNEL_SQL} = ? AND search.event_seq {comparison} ? \
         ORDER BY search.event_seq {direction} LIMIT ?"
    );
    Ok(sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(room_id)
        .bind(channel_id)
        .bind(target_seq)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await?)
}

async fn require_search_channel<'a>(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &'a str,
    allow_all: bool,
) -> Result<&'a str, PersistenceError> {
    let channel_id = if channel_id.is_empty() {
        "lobby"
    } else {
        channel_id
    };
    match channel_id {
        "lobby" => {}
        "all" if allow_all => {}
        "all" => {
            return Err(rejected(
                "bad_request",
                "A concrete channel_id is required.",
            ));
        }
        _ => require_text_channel(transaction, room_id, channel_id).await?,
    }
    Ok(channel_id)
}

fn checked_event(row: &SqliteRow, room_id: &str) -> Result<(RoomEvent, i64), PersistenceError> {
    let event: RoomEvent = serde_json::from_str(row.get::<&str, _>("event_json"))?;
    let created_at_nanos = row.get::<i64, _>("created_at_nanos");
    if event.room_id != room_id
        || event.id != row.get::<String, _>("event_id")
        || event.seq != row.get::<i64, _>("event_seq")
        || canonical_created_at_nanos(&event)? != created_at_nanos
    {
        return Err(invalid_state());
    }
    Ok((event, created_at_nanos))
}

fn fts_phrase(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn encode_cursor(cursor: (i64, i64)) -> Result<String, PersistenceError> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
}

fn decode_cursor(cursor: &str) -> Result<Option<(i64, i64)>, PersistenceError> {
    if cursor.is_empty() {
        return Ok(None);
    }
    if cursor.len() > MAX_MESSAGE_SEARCH_CURSOR_BYTES {
        return Err(invalid_cursor());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid_cursor())?;
    let decoded: (i64, i64) = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if decoded.0 <= 0 || decoded.1 <= 0 || encode_cursor(decoded)? != cursor {
        return Err(invalid_cursor());
    }
    Ok(Some(decoded))
}

fn require_history(principal: &AuthenticatedPrincipal) -> Result<(), PersistenceError> {
    if principal.capabilities.room_history {
        Ok(())
    } else {
        Err(rejected(
            "permission_denied",
            "This room session cannot read message history.",
        ))
    }
}

fn invalid_cursor() -> PersistenceError {
    rejected("bad_request", "cursor is invalid.")
}

fn message_missing() -> PersistenceError {
    rejected("message_missing", "The message was not found.")
}

fn invalid_state() -> PersistenceError {
    rejected(
        "invalid_state",
        "Stored message search projection is inconsistent.",
    )
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "message_search_tests.rs"]
mod tests;
