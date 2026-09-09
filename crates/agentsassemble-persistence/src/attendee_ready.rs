use crate::{
    AgentTurnCommit, AttendeeConnectionAuthorization, PersistenceError, SqliteStore,
    attendee_invites::rejected,
};
use agentsassemble_domain::{AgentRuntimeStatus, AgentSession, DurableAgentSession};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

pub(crate) async fn retained_interrupt_supported_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_attendee_invites invite JOIN attendee_connections connection USING(session_fingerprint) WHERE invite.room_id=? AND invite.participant_id=? AND connection.retained_interrupt=1)")
        .bind(room_id).bind(participant_id).fetch_one(&mut **tx).await?)
}

pub(crate) async fn project_session_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &AgentSession,
) -> Result<AgentSession, PersistenceError> {
    let mut projection = session.clone();
    projection.external_retained_interrupt = if session.external_owned
        && session.process_ownership == "external"
    {
        Some(retained_interrupt_supported_in(tx, &session.room_id, &session.participant_id).await?)
    } else {
        None
    };
    Ok(projection)
}

/// The external client reports its own runtime and selected public profile; no host paths or keys.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeRuntimeReady {
    pub retained_interrupt: bool,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub provider_session_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub variant: String,
    pub execution_harness: String,
    pub permission_mode: String,
    pub max_output_tokens: u32,
}

impl AttendeeRuntimeReady {
    fn validate(&self) -> Result<(), PersistenceError> {
        for field in [
            &self.runtime_handle_id,
            &self.runtime_owner_id,
            &self.runtime_lease_token,
            &self.provider_session_id,
            &self.model,
            &self.execution_harness,
            &self.permission_mode,
        ] {
            if field.trim().is_empty() {
                return Err(invalid_ready());
            }
        }
        for field in [
            &self.runtime_handle_id,
            &self.runtime_owner_id,
            &self.runtime_lease_token,
            &self.provider_session_id,
            &self.model,
            &self.reasoning_effort,
            &self.service_tier,
            &self.variant,
            &self.execution_harness,
            &self.permission_mode,
        ] {
            if field.len() > 512 || field.chars().any(char::is_control) {
                return Err(invalid_ready());
            }
        }
        Ok(())
    }

    fn from_session(session: &DurableAgentSession, retained_interrupt: bool) -> Self {
        Self {
            retained_interrupt,
            runtime_handle_id: session.runtime_handle_id.clone(),
            runtime_owner_id: session.runtime_owner_id.clone(),
            runtime_lease_token: session.runtime_lease_token.clone(),
            provider_session_id: session.provider_session_id.clone(),
            model: session.public.model.clone(),
            reasoning_effort: session.public.reasoning_effort.clone(),
            service_tier: session.public.service_tier.clone(),
            variant: session.public.variant.clone(),
            execution_harness: session.public.execution_harness.clone(),
            permission_mode: session.public.permission_mode.clone(),
            max_output_tokens: session.public.max_output_tokens,
        }
    }

    fn apply(&self, session: &mut DurableAgentSession) {
        session
            .runtime_handle_id
            .clone_from(&self.runtime_handle_id);
        session.runtime_owner_id.clone_from(&self.runtime_owner_id);
        session
            .runtime_lease_token
            .clone_from(&self.runtime_lease_token);
        session
            .provider_session_id
            .clone_from(&self.provider_session_id);
        session.public.model.clone_from(&self.model);
        session
            .public
            .reasoning_effort
            .clone_from(&self.reasoning_effort);
        session.public.service_tier.clone_from(&self.service_tier);
        session.public.variant.clone_from(&self.variant);
        session
            .public
            .execution_harness
            .clone_from(&self.execution_harness);
        session
            .public
            .permission_mode
            .clone_from(&self.permission_mode);
        session.public.max_output_tokens = self.max_output_tokens;
    }
}

impl SqliteStore {
    /// Binds readiness to one exact external runtime, preserving active turn custody on reconnect.
    ///
    /// # Errors
    /// Rejects changed runtimes/profiles, replaced connections and invalid or revoked authority.
    pub async fn record_attendee_ready(
        &self,
        connection: &AttendeeConnectionAuthorization,
        report: &AttendeeRuntimeReady,
        now: DateTime<Utc>,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        report.validate()?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_connection::revalidate_in(&mut tx, connection, now).await?;
        let principal = connection.session.principal();
        let mut session = crate::agent_lifecycle::load_session(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        crate::agent_lifecycle::require_valid_turn_authority(&session)?;
        if !session.lifecycle_intent_action.is_none()
            || crate::room_runtime_cleanup::cleanup_exists(
                &mut tx,
                &principal.room_id,
                &principal.participant_id,
            )
            .await?
        {
            return Err(rejected(
                "operation_in_progress",
                "An external lifecycle operation is still pending.",
            ));
        }
        let first = session.runtime_handle_id.is_empty();
        let retained_interrupt: bool = sqlx::query_scalar(
            "SELECT retained_interrupt FROM attendee_connections WHERE session_fingerprint=?",
        )
        .bind(connection.session.fingerprint.as_slice())
        .fetch_one(&mut *tx)
        .await?;
        if !first && report != &AttendeeRuntimeReady::from_session(&session, retained_interrupt) {
            return Err(rejected(
                "runtime_owner_mismatch",
                "Reconnect must retain its exact external runtime and profile.",
            ));
        }
        let changed = sqlx::query("UPDATE attendee_connections SET state='ready', retained_interrupt=? WHERE session_fingerprint=? AND connection_id=? AND state='connected'")
            .bind(report.retained_interrupt).bind(connection.session.fingerprint.as_slice()).bind(connection.connection_id.to_string()).execute(&mut *tx).await?;
        if changed.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(AgentTurnCommit {
                events: Vec::new(),
                next_assignments: Vec::new(),
            });
        }
        report.apply(&mut session);
        if first {
            session.public.enabled = true;
        }
        session.public.provider_session_active = true;
        if session.public.runtime_status == AgentRuntimeStatus::Disconnected {
            session.public.runtime_status = AgentRuntimeStatus::Idle;
        }
        if session.public.last_error_code == "bridge_disconnected" {
            session.public.last_error_code.clear();
            session.public.last_error.clear();
        }
        session.public.updated_at = now;
        crate::agent_lifecycle::save_session(&mut tx, &session).await?;
        let event = crate::room_turns::support::session_state_event(&mut tx, &session).await?;
        let (room, settings) =
            crate::room_turns::support::load_active_room(&mut tx, &principal.room_id).await?;
        let mut commit = crate::room_turns::assign_pending_in(&mut tx, &room, &settings).await?;
        commit.events.insert(0, event);
        tx.commit().await?;
        Ok(commit)
    }
}

pub(crate) async fn is_available(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<bool, PersistenceError> {
    if !session.public.external_owned {
        return Ok(true);
    }
    let fingerprint: Option<Vec<u8>> = sqlx::query_scalar("SELECT invite.session_fingerprint FROM room_attendee_invites invite JOIN attendee_connections connection USING(session_fingerprint) WHERE invite.room_id=? AND invite.participant_id=? AND connection.state='ready'")
        .bind(&session.public.room_id).bind(&session.public.participant_id).fetch_optional(&mut **tx).await?;
    let Some(fingerprint) = fingerprint else {
        return Ok(false);
    };
    let fingerprint = fingerprint.try_into().map_err(|_| invalid_ready())?;
    match crate::attendee_session::authorize_in(tx, &fingerprint, Utc::now()).await {
        Ok(_) => Ok(true),
        Err(PersistenceError::ParticipantMissing | PersistenceError::RoomMissing) => Ok(false),
        Err(PersistenceError::CommandRejected { code, .. })
            if matches!(code.as_bytes(), b"session_revoked" | b"permission_denied") =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn invalid_ready() -> PersistenceError {
    rejected(
        "invalid_bridge_ready",
        "The external runtime report is missing or invalid.",
    )
}
