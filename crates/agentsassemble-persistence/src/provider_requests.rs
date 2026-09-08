//! Persistent provider-request authority, shared by managed and admitted external runtimes.
use std::collections::BTreeMap;

use agentsassemble_domain::{DurableAgentSession, ProviderRequest, RoomEvent};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, SqliteStore,
    agent_lifecycle::load_session,
    attendee_connection::authorize_current_in,
    authority::load_active_membership,
    room_turns::support::{internal_event, load_event},
    room_write_budget::reserve_room_write_budget,
    turn_authority::require_provider_room_tool_authority,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenProviderRequest {
    pub turn_generation: u64,
    pub execution_id: String,
    pub request: ProviderRequest,
}

pub struct ProviderRequestCommit {
    pub session_id: String,
    pub event: RoomEvent,
    pub expires_at: DateTime<Utc>,
    pub deduplicated: bool,
}

pub(crate) async fn snapshot_pending_in(
    tx: &mut Transaction<'_, Sqlite>,
    principal: Option<&agentsassemble_domain::AuthenticatedPrincipal>,
) -> Result<Vec<agentsassemble_domain::PendingProviderRequest>, PersistenceError> {
    let Some(principal) = principal
        .filter(|principal| principal.client_kind == agentsassemble_domain::ClientKind::Browser)
    else {
        return Ok(Vec::new());
    };
    let rows = sqlx::query("SELECT session_id, request_json, expires_at, state FROM provider_requests WHERE room_id=? AND owner_id=? AND state IN ('open','resolving') ORDER BY open_event_id")
        .bind(&principal.room_id).bind(&principal.participant_id).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            let request: ProviderRequest = serde_json::from_str(row.get("request_json"))?;
            if !request.is_valid() {
                return Err(rejected(
                    "invalid_state",
                    "Stored provider request is invalid.",
                ));
            }
            Ok(agentsassemble_domain::PendingProviderRequest {
                session_id: row.get("session_id"),
                request,
                expires_at: DateTime::from_timestamp_millis(row.get("expires_at")).ok_or_else(
                    || {
                        rejected(
                            "invalid_state",
                            "Stored provider request deadline is invalid.",
                        )
                    },
                )?,
                state: match row.get::<&str, _>("state") {
                    "open" => agentsassemble_domain::PendingProviderRequestState::Open,
                    "resolving" => agentsassemble_domain::PendingProviderRequestState::Resolving,
                    _ => {
                        return Err(rejected(
                            "invalid_state",
                            "Stored pending request state is invalid.",
                        ));
                    }
                },
            })
        })
        .collect()
}

impl SqliteStore {
    /// Opens a request under the managed runtime's exact active turn authority.
    ///
    /// # Errors
    /// Rejects invalid requests, stale executions, inactive participants and pending requests.
    pub async fn open_managed_provider_request(
        &self,
        room_id: &str,
        session_id: &str,
        request: &OpenProviderRequest,
        now: DateTime<Utc>,
    ) -> Result<ProviderRequestCommit, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let session = load_session(&mut tx, room_id, session_id).await?;
        require_provider_room_tool_authority(
            &mut tx,
            &session,
            &session.public.active_turn_id,
            session.input_up_to_seq,
            request.turn_generation,
            &request.execution_id,
        )
        .await?;
        let result = open_in(&mut tx, &session, request, None, now).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Opens a request under the current admitted connection and exact external turn.
    ///
    /// # Errors
    /// Also rejects replaced connections, revoked admissions and retained interrupts.
    pub async fn open_attendee_provider_request(
        &self,
        fingerprint: &[u8; 32],
        connection_id: Uuid,
        request: &OpenProviderRequest,
        now: DateTime<Utc>,
    ) -> Result<ProviderRequestCommit, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let connection = authorize_current_in(&mut tx, fingerprint, connection_id, now).await?;
        let session = crate::attendee_tool_authority::load_turn_in(
            &mut tx,
            &connection,
            request.turn_generation,
            &request.execution_id,
        )
        .await?;
        let result = open_in(&mut tx, &session, request, Some(connection_id), now).await?;
        tx.commit().await?;
        Ok(result)
    }
}

// Both entry points establish custody and execution in this transaction first.
async fn open_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    input: &OpenProviderRequest,
    connection_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<ProviderRequestCommit, PersistenceError> {
    if !input.request.is_valid() {
        return Err(rejected(
            "invalid_provider_request",
            "Provider request is invalid.",
        ));
    }
    let room_id = &session.public.room_id;
    let session_id = &session.public.session_id;
    let (_, participant) =
        load_active_membership(tx, room_id, &session.public.participant_id).await?;
    if participant.muted || participant.owner_id.is_empty() {
        return Err(rejected(
            "permission_denied",
            "Provider request has no active owner authority.",
        ));
    }
    let (_, owner) = load_active_membership(tx, room_id, &participant.owner_id).await?;
    if owner.participant_type != "human" {
        return Err(rejected(
            "permission_denied",
            "Provider request requires a human owner.",
        ));
    }
    let request_id = input.request.provider_request_id.to_string();
    if let Some(existing) = replay_in(
        tx,
        session,
        input,
        &participant.owner_id,
        connection_id,
        now,
    )
    .await?
    {
        return Ok(existing);
    }
    let pending: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM provider_requests WHERE room_id=? AND session_id=? AND state IN ('open','resolving'))",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_one(&mut **tx)
    .await?;
    if pending != 0 {
        return Err(rejected(
            "provider_request_pending",
            "This session already has a pending provider request.",
        ));
    }
    let encoded = serde_json::to_string(&input.request)?;
    reserve_room_write_budget(tx, room_id, encoded.len()).await?;
    let expires_at = now
        .checked_add_signed(TimeDelta::seconds(i64::from(input.request.timeout_seconds)))
        .and_then(|deadline| DateTime::from_timestamp_millis(deadline.timestamp_millis()))
        .ok_or_else(|| {
            rejected(
                "invalid_provider_request",
                "Provider request deadline is invalid.",
            )
        })?;
    let event = internal_event(
        tx,
        session,
        "provider_request_opened",
        true,
        None,
        BTreeMap::from([
            ("visibility".to_owned(), json!("owner")),
            ("owner_id".to_owned(), json!(participant.owner_id)),
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("provider_request".to_owned(), json!(input.request)),
            ("expires_at".to_owned(), json!(expires_at)),
        ]),
    )
    .await?;
    sqlx::query(
        "INSERT INTO provider_requests(room_id, request_id, session_id, turn_generation, execution_id, owner_id, request_json, expires_at, state, open_event_id, connection_id) VALUES (?,?,?,?,?,?,?,?,'open',?,?)",
    )
    .bind(room_id)
    .bind(&request_id)
    .bind(session_id)
    .bind(i64::try_from(input.turn_generation).map_err(|_| PersistenceError::CommandConflict)?)
    .bind(&input.execution_id)
    .bind(&participant.owner_id)
    .bind(encoded)
    .bind(expires_at.timestamp_millis())
    .bind(&event.id)
    .bind(connection_id.map(|id| id.to_string()))
    .execute(&mut **tx)
    .await?;
    Ok(ProviderRequestCommit {
        session_id: session_id.to_owned(),
        event,
        expires_at,
        deduplicated: false,
    })
}

pub(crate) fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

async fn replay_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    input: &OpenProviderRequest,
    owner_id: &str,
    connection_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<Option<ProviderRequestCommit>, PersistenceError> {
    let room_id = &session.public.room_id;
    let session_id = &session.public.session_id;
    let request_id = input.request.provider_request_id.to_string();
    let Some(row) = sqlx::query(
        "SELECT session_id, turn_generation, execution_id, owner_id, request_json, expires_at, state, open_event_id, connection_id FROM provider_requests WHERE room_id=? AND request_id=?",
    )
    .bind(room_id)
    .bind(&request_id)
    .fetch_optional(&mut **tx)
    .await?
    else { return Ok(None); };
    let stored: ProviderRequest = serde_json::from_str(row.get("request_json"))?;
    if row.get::<&str, _>("session_id") != session_id
        || row.get::<i64, _>("turn_generation")
            != i64::try_from(input.turn_generation)
                .map_err(|_| PersistenceError::CommandConflict)?
        || row.get::<&str, _>("execution_id") != input.execution_id
        || row.get::<&str, _>("owner_id") != owner_id
        || row.get::<Option<String>, _>("connection_id") != connection_id.map(|id| id.to_string())
        || stored != input.request
    {
        return Err(PersistenceError::CommandConflict);
    }
    let expires_at = DateTime::from_timestamp_millis(row.get("expires_at")).ok_or_else(|| {
        rejected(
            "invalid_state",
            "Stored provider request deadline is invalid.",
        )
    })?;
    if expires_at <= now || row.get::<&str, _>("state") != "open" {
        return Err(rejected(
            "provider_request_closed",
            "Provider request is no longer open.",
        ));
    }
    let event = load_event(tx, room_id, row.get("open_event_id"))
        .await?
        .ok_or_else(|| rejected("invalid_state", "Provider request event is missing."))?;
    Ok(Some(ProviderRequestCommit {
        session_id: session_id.to_owned(),
        event,
        expires_at,
        deduplicated: true,
    }))
}
