use agentsassemble_domain::{
    AuthenticatedPrincipal, MAX_MESSAGE_SEARCH_CURSOR_BYTES, MESSAGE_CONTEXT_RADIUS,
    MESSAGE_SEARCH_PAGE_SIZE, RoomEvent, casefold_message_search_text, clean_message_search_query,
    compact_casefolded_message_search_text, public_event_for_principal,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{SecondsFormat, Utc};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    human_session_authority::revalidate_human_session,
    message_search_index::{canonical_created_at_nanos, searchable_lobby_message},
    room_user_identity::current_local_room_principal,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyMessageSearchResult {
    pub event_id: String,
    pub seq: i64,
    pub created_at: String,
    pub author: String,
    pub content: String,
    pub attachment_filenames: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LobbyMessageSearchPage {
    pub results: Vec<LobbyMessageSearchResult>,
    pub next_cursor: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LobbyMessageContext {
    pub event_id: String,
    pub events: Vec<RoomEvent>,
}

impl SqliteStore {
    /// Searches canonical lobby history as the current local room manager.
    ///
    /// # Errors
    ///
    /// Rejects stale authority, invalid query/cursor input, or inconsistent stored projections.
    pub async fn search_local_lobby_messages(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        query: &str,
        cursor: &str,
    ) -> Result<LobbyMessageSearchPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal =
            current_local_room_principal(&mut transaction, room_id, user_id, participant_id)
                .await?;
        let page = search_in(&mut transaction, &principal, query, cursor).await?;
        transaction.commit().await?;
        Ok(page)
    }

    /// Searches canonical lobby history while a durable human session remains authorized.
    ///
    /// # Errors
    ///
    /// Rejects revoked history permission, invalid input, or inconsistent stored projections.
    pub async fn search_human_session_lobby_messages(
        &self,
        expected: &HumanSessionAuthorization,
        query: &str,
        cursor: &str,
    ) -> Result<LobbyMessageSearchPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) = revalidate_human_session(&mut transaction, expected, Utc::now()).await?;
        let principal = current.principal();
        require_history(principal)?;
        let page = search_in(&mut transaction, principal, query, cursor).await?;
        transaction.commit().await?;
        Ok(page)
    }

    /// Reads bounded canonical lobby context as the current local room manager.
    ///
    /// # Errors
    ///
    /// Rejects stale authority, unknown targets, or inconsistent stored projections.
    pub async fn local_lobby_message_context(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        event_id: &str,
    ) -> Result<LobbyMessageContext, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal =
            current_local_room_principal(&mut transaction, room_id, user_id, participant_id)
                .await?;
        let context = context_in(&mut transaction, &principal, event_id).await?;
        transaction.commit().await?;
        Ok(context)
    }

    /// Reads bounded canonical lobby context while a durable human session remains authorized.
    ///
    /// # Errors
    ///
    /// Rejects revoked history permission, unknown targets, or inconsistent stored projections.
    pub async fn human_session_lobby_message_context(
        &self,
        expected: &HumanSessionAuthorization,
        event_id: &str,
    ) -> Result<LobbyMessageContext, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) = revalidate_human_session(&mut transaction, expected, Utc::now()).await?;
        let principal = current.principal();
        require_history(principal)?;
        let context = context_in(&mut transaction, principal, event_id).await?;
        transaction.commit().await?;
        Ok(context)
    }
}

async fn search_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    query: &str,
    cursor: &str,
) -> Result<LobbyMessageSearchPage, PersistenceError> {
    require_history(principal)?;
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
            &principal.room_id,
            &compact,
            &phrase,
            cursor,
            limit,
        )
        .await?
    } else {
        search_short(transaction, &principal.room_id, &phrase, cursor, limit).await?
    };
    project_page(principal, rows)
}

async fn search_long(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    compact: &str,
    phrase: &str,
    cursor: Option<(i64, i64)>,
    limit: i64,
) -> Result<Vec<SqliteRow>, PersistenceError> {
    let (created_at, seq) = cursor.unwrap_or((i64::MAX, i64::MAX));
    Ok(sqlx::query(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND (instr(search.compact_text, ?) > 0 OR search.id IN (\
             SELECT rowid FROM room_message_search_phrase \
             WHERE room_message_search_phrase MATCH ?\
         )) \
         AND (search.created_at_nanos < ? OR (search.created_at_nanos = ? AND search.event_seq < ?)) \
         ORDER BY search.created_at_nanos DESC, search.event_seq DESC LIMIT ?",
    )
    .bind(room_id)
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
    phrase: &str,
    cursor: Option<(i64, i64)>,
    limit: i64,
) -> Result<Vec<SqliteRow>, PersistenceError> {
    let (created_at, seq) = cursor.unwrap_or((i64::MAX, i64::MAX));
    Ok(sqlx::query(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_phrase AS phrase \
         JOIN room_message_search_records AS search ON search.id = phrase.rowid \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE phrase.search_text MATCH ? AND search.room_id = ? \
         AND (search.created_at_nanos < ? OR (search.created_at_nanos = ? AND search.event_seq < ?)) \
         ORDER BY search.created_at_nanos DESC, search.event_seq DESC LIMIT ?",
    )
    .bind(phrase)
    .bind(room_id)
    .bind(created_at)
    .bind(created_at)
    .bind(seq)
    .bind(limit)
    .fetch_all(&mut **transaction)
    .await?)
}

fn project_page(
    principal: &AuthenticatedPrincipal,
    mut rows: Vec<SqliteRow>,
) -> Result<LobbyMessageSearchPage, PersistenceError> {
    let has_more = rows.len() > MESSAGE_SEARCH_PAGE_SIZE;
    rows.truncate(MESSAGE_SEARCH_PAGE_SIZE);
    let mut results = Vec::with_capacity(rows.len());
    let mut last_cursor = None;
    for row in rows {
        let (event, created_at_nanos) = checked_event(&row, &principal.room_id)?;
        let message = searchable_lobby_message(&event)?.ok_or_else(invalid_state)?;
        last_cursor = Some((created_at_nanos, event.seq));
        results.push(LobbyMessageSearchResult {
            event_id: event.id,
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
    Ok(LobbyMessageSearchPage {
        results,
        next_cursor,
    })
}

async fn context_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    event_id: &str,
) -> Result<LobbyMessageContext, PersistenceError> {
    require_history(principal)?;
    if event_id.is_empty() || event_id.len() > 128 || event_id.contains('\0') {
        return Err(rejected("bad_request", "event_id is invalid."));
    }
    let target = sqlx::query(
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND search.event_id = ?",
    )
    .bind(&principal.room_id)
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(message_missing)?;
    let (target_event, _) = checked_event(&target, &principal.room_id)?;
    let _ = searchable_lobby_message(&target_event)?.ok_or_else(invalid_state)?;
    let radius = i64::try_from(MESSAGE_CONTEXT_RADIUS).map_err(|_| invalid_state())?;
    let mut rows = context_side(
        transaction,
        &principal.room_id,
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
            let _ = searchable_lobby_message(&event)?.ok_or_else(invalid_state)?;
            Ok(public_event_for_principal(&event, principal))
        })
        .collect::<Result<Vec<_>, PersistenceError>>()?;
    Ok(LobbyMessageContext {
        event_id: target_event.id,
        events,
    })
}

async fn context_side(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    target_seq: i64,
    before: bool,
    limit: i64,
) -> Result<Vec<SqliteRow>, PersistenceError> {
    let sql = if before {
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND search.event_seq < ? \
         ORDER BY search.event_seq DESC LIMIT ?"
    } else {
        "SELECT search.event_id, search.event_seq, search.created_at_nanos, events.event_json \
         FROM room_message_search_records AS search \
         JOIN room_events AS events ON events.room_id = search.room_id AND events.seq = search.event_seq \
         WHERE search.room_id = ? AND search.event_seq > ? \
         ORDER BY search.event_seq ASC LIMIT ?"
    };
    Ok(sqlx::query(sql)
        .bind(room_id)
        .bind(target_seq)
        .bind(limit)
        .fetch_all(&mut **transaction)
        .await?)
}

fn checked_event(row: &SqliteRow, room_id: &str) -> Result<(RoomEvent, i64), PersistenceError> {
    let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
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
