use crate::{
    AttendeeSessionAuthorization, PersistenceError, SqliteStore, attendee_invites::rejected,
};
use agentsassemble_domain::{AgentRuntimeStatus, RoomEvent};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

/// A single network connection, issued by persistence under exact attendee provenance.
#[derive(Clone)]
pub struct AttendeeConnectionAuthorization {
    pub(crate) session: AttendeeSessionAuthorization,
    pub(crate) connection_id: Uuid,
}

impl AttendeeConnectionAuthorization {
    #[must_use]
    pub const fn session(&self) -> &AttendeeSessionAuthorization {
        &self.session
    }
    #[must_use]
    pub const fn connection_id(&self) -> Uuid {
        self.connection_id
    }
}

pub struct AttendeeConnectionClaim {
    pub authorization: AttendeeConnectionAuthorization,
    pub events: Vec<RoomEvent>,
}

impl SqliteStore {
    /// Replaces network custody without granting readiness or changing provider execution identity.
    ///
    /// # Errors
    /// Rejects expired/revoked admission, invalid connection IDs and persistence failures.
    pub async fn claim_attendee_connection(
        &self,
        expected: &AttendeeSessionAuthorization,
        connection_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<AttendeeConnectionClaim, PersistenceError> {
        if connection_id.is_nil() {
            return Err(rejected("bad_request", "A connection UUID is required."));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = crate::attendee_session::revalidate_in(&mut tx, expected, now).await?;
        // A connection identity belongs to one TCP/WebSocket lifetime and is never reused.
        sqlx::query("INSERT INTO attendee_connections(session_fingerprint,connection_id,state) VALUES(?,?,'connected') ON CONFLICT(session_fingerprint) DO UPDATE SET connection_id=excluded.connection_id,state='connected'")
            .bind(current.fingerprint.as_slice()).bind(connection_id.to_string()).execute(&mut *tx).await?;
        let events = mark_network_unavailable(&mut tx, &current, now)
            .await?
            .into_iter()
            .collect();
        tx.commit().await?;
        Ok(AttendeeConnectionClaim {
            authorization: AttendeeConnectionAuthorization {
                session: current,
                connection_id,
            },
            events,
        })
    }

    /// Revalidates both membership and the current network generation.
    ///
    /// # Errors
    /// Rejects replaced/disconnected sockets and revoked admission.
    pub async fn revalidate_attendee_connection(
        &self,
        expected: &AttendeeConnectionAuthorization,
        now: DateTime<Utc>,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        revalidate_in(&mut tx, expected, now).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Ends only this exact network connection; it makes no provider-stop claim.
    ///
    /// # Errors
    /// Propagates storage errors. A replaced socket's close is an explicit no-op.
    pub async fn disconnect_attendee_connection(
        &self,
        expected: &AttendeeConnectionAuthorization,
        now: DateTime<Utc>,
    ) -> Result<Option<RoomEvent>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let updated = sqlx::query("UPDATE attendee_connections SET state='disconnected' WHERE session_fingerprint=? AND connection_id=? AND state!='disconnected'")
            .bind(expected.session.fingerprint.as_slice()).bind(expected.connection_id.to_string()).execute(&mut *tx).await?;
        // Closing must remain possible after membership revocation, but cannot act on a replacement.
        let event = if updated.rows_affected() == 1 {
            mark_network_unavailable(&mut tx, &expected.session, now).await?
        } else {
            None
        };
        tx.commit().await?;
        Ok(event)
    }

    /// Invalidates old network lifetimes once at startup, before request admission.
    ///
    /// # Errors
    /// Propagates storage/state failures; never treats remote custody as locally stopped.
    pub async fn disconnect_attendees_before_admission(&self) -> Result<(), PersistenceError> {
        loop {
            let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let rows = sqlx::query("SELECT connection.session_fingerprint, invite.room_id, invite.participant_id FROM attendee_connections connection JOIN room_attendee_invites invite USING(session_fingerprint) WHERE connection.state!='disconnected' LIMIT 64")
                .fetch_all(&mut *tx).await?;
            if rows.is_empty() {
                tx.commit().await?;
                return Ok(());
            }
            for row in rows {
                let fingerprint: Vec<u8> = row.get("session_fingerprint");
                sqlx::query("UPDATE attendee_connections SET state='disconnected' WHERE session_fingerprint=?")
                    .bind(&fingerprint).execute(&mut *tx).await?;
                mark_session_network_unavailable(
                    &mut tx,
                    row.get("room_id"),
                    row.get("participant_id"),
                    Utc::now(),
                )
                .await?;
            }
            tx.commit().await?;
        }
    }
}

pub(crate) async fn revalidate_in(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &AttendeeConnectionAuthorization,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    crate::attendee_session::revalidate_in(tx, &expected.session, now).await?;
    let live: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attendee_connections WHERE session_fingerprint=? AND connection_id=? AND state!='disconnected')")
        .bind(expected.session.fingerprint.as_slice()).bind(expected.connection_id.to_string()).fetch_one(&mut **tx).await?;
    if !live {
        return Err(rejected(
            "bridge_connection_replaced",
            "This attendee connection is no longer current.",
        ));
    }
    Ok(())
}

async fn mark_network_unavailable(
    tx: &mut Transaction<'_, Sqlite>,
    session: &AttendeeSessionAuthorization,
    now: DateTime<Utc>,
) -> Result<Option<RoomEvent>, PersistenceError> {
    mark_session_network_unavailable(
        tx,
        &session.principal.room_id,
        &session.principal.participant_id,
        now,
    )
    .await
}

async fn mark_session_network_unavailable(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<RoomEvent>, PersistenceError> {
    let mut session = crate::agent_lifecycle::load_session(tx, room_id, participant_id).await?;
    crate::agent_lifecycle::require_valid_turn_authority(&session)?;
    if session.runtime_handle_id.is_empty()
        || session.public.last_error_code == "bridge_disconnected"
    {
        return Ok(None);
    }
    if session.public.runtime_status == AgentRuntimeStatus::Idle {
        session.public.runtime_status = AgentRuntimeStatus::Disconnected;
    }
    // Busy/Stopping and its exact turn remain intact: losing the socket proves no quiescence.
    "bridge_disconnected".clone_into(&mut session.public.last_error_code);
    "The external provider connection is unavailable; runtime state is unconfirmed."
        .clone_into(&mut session.public.last_error);
    session.public.updated_at = now;
    crate::agent_lifecycle::save_session(tx, &session).await?;
    Ok(Some(
        crate::room_turns::support::session_state_event(tx, &session).await?,
    ))
}
