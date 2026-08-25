use agentsassemble_domain::{
    AgentSessionDraft, AuthenticatedPrincipal, ParticipantStatus, RoomEvent, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::Value;

use crate::{
    AgentLaunchFailureCommit, AgentRuntimeStarted, CommandOutcome, PersistenceError, SqliteStore,
    agent_creation_records::create_or_reuse_agent_records,
    agent_launch_events::{append_launch_events, launch_result},
    agent_lifecycle::{
        apply_runtime_started, load_participant, load_session, save_participant, save_session,
    },
    agent_lifecycle_authority::{
        authorize_control, lifecycle_operation_id, require_intent, validate_runtime_started,
    },
    agent_lifecycle_effect_authority::authorize_start_effect,
    agent_lifecycle_events::store_result,
    agent_lifecycle_reservations::{
        LifecycleReservation, StoredLifecycleReservation, finish_lifecycle_command,
        load_lifecycle_reservation, stored_rejection,
    },
    authority::active_room_for_principal,
    command_admission::existing_command,
    filesystem_authority::revalidate_runtime_authority,
};

const CREATE: &str = "agent.create";

#[derive(Debug, Clone)]
pub struct AgentCreateStartEffect {
    pub operation_id: String,
    pub session: agentsassemble_domain::DurableAgentSession,
    pub committed_events: Vec<RoomEvent>,
    pub newly_committed_events: Vec<RoomEvent>,
    prepared_result_json: String,
}

#[derive(Debug, Clone)]
pub enum AgentCreateStartPlan {
    Outcome(Box<CommandOutcome>),
    Select,
    Start(Box<AgentCreateStartEffect>),
}

#[derive(Debug, Clone)]
pub struct AgentCreateStartCommit {
    pub outcome: CommandOutcome,
    pub committed_events: Vec<RoomEvent>,
    pub newly_committed_events: Vec<RoomEvent>,
}

impl SqliteStore {
    /// Resolves replay or a committed create/start intent before mutable catalog discovery.
    ///
    /// # Errors
    ///
    /// Returns authorization, request-conflict, or inconsistent durable-authority failures.
    pub async fn inspect_agent_create_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentCreateStartPlan, PersistenceError> {
        authorize_control(principal)?;
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            CREATE,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentCreateStartPlan::Outcome(Box::new(outcome)));
        }
        let effect = resume_create_start(
            &mut transaction,
            principal,
            request_id,
            &payload_hash,
            self.runtime_generation(),
        )
        .await?;
        transaction.commit().await?;
        Ok(effect.map_or(AgentCreateStartPlan::Select, |effect| {
            AgentCreateStartPlan::Start(Box::new(effect))
        }))
    }

    /// Atomically commits creation records and the exact optional-start intent.
    ///
    /// # Errors
    ///
    /// Returns selection-authority, request-conflict, collision, or persistence failures.
    pub async fn prepare_agent_create_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        draft: &AgentSessionDraft,
    ) -> Result<AgentCreateStartPlan, PersistenceError> {
        authorize_control(principal)?;
        let payload_hash = canonical_payload_hash(payload);
        let operation_id = lifecycle_operation_id(principal, request_id, CREATE);
        revalidate_runtime_authority(draft).await?;
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            CREATE,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentCreateStartPlan::Outcome(Box::new(outcome)));
        }
        if let Some(effect) = resume_create_start(
            &mut transaction,
            principal,
            request_id,
            &payload_hash,
            self.runtime_generation(),
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentCreateStartPlan::Start(Box::new(effect)));
        }
        let records = create_or_reuse_agent_records(
            &mut transaction,
            principal,
            draft,
            Some(&operation_id),
            true,
        )
        .await?;
        let prepared_result_json = serde_json::to_string(&records.result)?;
        self.claim_lifecycle_command(
            &mut transaction,
            &LifecycleReservation::creation(
                principal,
                request_id,
                &payload_hash,
                &records.session.public.session_id,
                &operation_id,
                &prepared_result_json,
            ),
            payload,
            true,
        )
        .await?;
        let newly_committed_events = records.committed_events.clone();
        let effect = AgentCreateStartEffect {
            operation_id,
            session: records.session,
            committed_events: records.committed_events,
            newly_committed_events,
            prepared_result_json,
        };
        transaction.commit().await?;
        Ok(AgentCreateStartPlan::Start(Box::new(effect)))
    }

    /// Persists a create/start runtime identity and effect-inflight phase before provider I/O.
    ///
    /// # Errors
    ///
    /// Returns an exact reservation, authority, state, or persistence failure.
    pub async fn authorize_agent_create_start_effect(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        runtime_authority: (&str, &str, &str),
    ) -> Result<AgentCreateStartEffect, PersistenceError> {
        let (runtime_handle_id, runtime_owner_id, runtime_lease_token) = runtime_authority;
        let payload_hash = canonical_payload_hash(payload);
        let expected_operation_id = lifecycle_operation_id(principal, request_id, CREATE);
        if operation_id != expected_operation_id {
            return Err(rejected(
                "stale_start_authorization",
                "Provider start authorization does not match its create request.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let stored =
            required_create_reservation(&mut transaction, principal, request_id, &payload_hash)
                .await?;
        if stored.operation_id != operation_id
            || stored.supervisor_generation != self.runtime_generation()
        {
            return Err(rejected(
                "stale_lifecycle_reservation",
                "Create/start reservation is not owned by this runtime.",
            ));
        }
        let prepared_result: Value = serde_json::from_str(&stored.prepared_result_json)?;
        validate_prepared_result(&prepared_result, &stored.session_id)?;
        let mut session =
            load_session(&mut transaction, &principal.room_id, &stored.session_id).await?;
        authorize_start_effect(
            &mut session,
            operation_id,
            runtime_handle_id,
            runtime_owner_id,
            runtime_lease_token,
        )?;
        save_session(&mut transaction, &session).await?;
        transaction.commit().await?;
        Ok(AgentCreateStartEffect {
            operation_id: operation_id.to_owned(),
            session,
            committed_events: prepared_events(&prepared_result)?,
            newly_committed_events: Vec::new(),
            prepared_result_json: stored.prepared_result_json,
        })
    }

    /// Commits the provider launch and the original create result with its nested start result.
    ///
    /// # Errors
    ///
    /// Returns stale-effect, request-conflict, authority, or persistence failures.
    pub async fn complete_agent_create_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        started: &AgentRuntimeStarted,
    ) -> Result<AgentCreateStartCommit, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let expected_operation_id = lifecycle_operation_id(principal, request_id, CREATE);
        if operation_id != expected_operation_id {
            return Err(rejected(
                "stale_start_confirmation",
                "Provider start confirmation does not match its create request.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            CREATE,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentCreateStartCommit {
                outcome,
                committed_events: Vec::new(),
                newly_committed_events: Vec::new(),
            });
        }
        let stored =
            required_create_reservation(&mut transaction, principal, request_id, &payload_hash)
                .await?;
        if stored.operation_id != expected_operation_id {
            return Err(PersistenceError::CommandConflict);
        }
        let mut prepared_result: Value = serde_json::from_str(&stored.prepared_result_json)?;
        validate_prepared_result(&prepared_result, &stored.session_id)?;
        let mut session =
            load_session(&mut transaction, &principal.room_id, &stored.session_id).await?;
        validate_runtime_started(&session, started)?;
        require_intent(
            &session,
            "start",
            &expected_operation_id,
            "effect_inflight",
            "stale_start_confirmation",
        )?;
        finish_lifecycle_command(
            &mut transaction,
            &LifecycleReservation::creation(
                principal,
                request_id,
                &payload_hash,
                &stored.session_id,
                &expected_operation_id,
                &stored.prepared_result_json,
            ),
        )
        .await?;
        apply_runtime_started(&mut session, started);
        save_session(&mut transaction, &session).await?;
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, &stored.session_id).await?;
        let joined = participant.status != ParticipantStatus::Joined;
        participant.status = ParticipantStatus::Joined;
        participant.updated_at = Utc::now();
        save_participant(&mut transaction, &participant).await?;
        let launch_events =
            append_launch_events(&mut transaction, principal, &session, &participant, joined)
                .await?;
        let newly_committed_events = launch_events.clone();
        let start_result = launch_result(&session, started.runtime_reused, &launch_events);
        let mut committed_events = prepared_events(&prepared_result)?;
        committed_events.extend(launch_events);
        prepared_result["start"] = start_result;
        prepared_result["events"] = serde_json::to_value(&committed_events)?;
        prepared_result["event"] = serde_json::to_value(committed_events.last())?;
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            CREATE,
            payload_hash,
            prepared_result,
            committed_events.clone(),
        )
        .await?;
        transaction.commit().await?;
        Ok(AgentCreateStartCommit {
            outcome,
            committed_events,
            newly_committed_events,
        })
    }

    /// Records a confirmed-safe launch failure while retaining the created Agent Session.
    ///
    /// # Errors
    ///
    /// Returns stale-effect, authority, or persistence failures.
    #[allow(clippy::too_many_arguments)]
    pub async fn fail_agent_create_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        effect: &AgentCreateStartEffect,
        error_code: &'static str,
        message: &str,
    ) -> Result<AgentLaunchFailureCommit, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        self.fail_agent_launch_command(
            principal,
            request_id,
            &payload_hash,
            &effect.session.public.session_id,
            &effect.operation_id,
            error_code,
            message,
            CREATE,
            "creation_committed",
            &effect.prepared_result_json,
        )
        .await
    }

    /// Commits a create/start rejection before any provider effect was authorized.
    ///
    /// # Errors
    ///
    /// Returns stale-effect, authority, or persistence failures.
    pub async fn fail_agent_create_start_before_effect(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        effect: &AgentCreateStartEffect,
        error_code: &'static str,
        message: &str,
    ) -> Result<AgentLaunchFailureCommit, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        self.fail_agent_launch_before_effect_command(
            principal,
            request_id,
            &payload_hash,
            &effect.session.public.session_id,
            &effect.operation_id,
            error_code,
            message,
            CREATE,
            "creation_committed",
            &effect.prepared_result_json,
        )
        .await
    }
}

async fn resume_create_start(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: &str,
    runtime_generation: &str,
) -> Result<Option<AgentCreateStartEffect>, PersistenceError> {
    let Some(stored) = load_lifecycle_reservation(
        transaction,
        &principal.room_id,
        &principal.principal_id,
        request_id,
    )
    .await?
    else {
        return Ok(None);
    };
    validate_create_reservation(&stored, payload_hash)?;
    if stored.status == "pending" && stored.supervisor_generation != runtime_generation {
        return Err(PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            message: "The original provider effect belongs to a previous server runtime and awaits server-owned reconciliation.".to_owned(),
        });
    }
    let prepared_result: Value = serde_json::from_str(&stored.prepared_result_json)?;
    validate_prepared_result(&prepared_result, &stored.session_id)?;
    let session = load_session(transaction, &principal.room_id, &stored.session_id).await?;
    if session.lifecycle_intent_action != "start"
        || session.lifecycle_intent_id != stored.operation_id
    {
        return Err(rejected(
            "operation_in_progress",
            "The stored create/start operation does not own this Agent Session.",
        ));
    }
    match session.lifecycle_intent_status.as_str() {
        "prepared" => {}
        "effect_inflight" | "unconfirmed" => {
            return Err(PersistenceError::CommandUnresolved {
                code: "runtime_effect_unconfirmed",
                message: "The original provider start effect remains unresolved. Wait for authoritative runtime observation before retrying it.".to_owned(),
            });
        }
        _ => {
            return Err(rejected(
                "invalid_state",
                "Stored provider create/start intent is invalid.",
            ));
        }
    }
    Ok(Some(AgentCreateStartEffect {
        operation_id: stored.operation_id,
        session,
        committed_events: prepared_events(&prepared_result)?,
        newly_committed_events: Vec::new(),
        prepared_result_json: stored.prepared_result_json,
    }))
}

async fn required_create_reservation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: &str,
) -> Result<StoredLifecycleReservation, PersistenceError> {
    let stored = load_lifecycle_reservation(
        transaction,
        &principal.room_id,
        &principal.principal_id,
        request_id,
    )
    .await?
    .ok_or_else(|| {
        rejected(
            "stale_lifecycle_reservation",
            "Create/start reservation is missing.",
        )
    })?;
    validate_create_reservation(&stored, payload_hash)?;
    Ok(stored)
}

fn validate_create_reservation(
    stored: &StoredLifecycleReservation,
    payload_hash: &str,
) -> Result<(), PersistenceError> {
    if stored.action != CREATE || stored.payload_hash != payload_hash {
        return Err(PersistenceError::CommandConflict);
    }
    if stored.phase != "creation_committed" {
        return Err(rejected(
            "stale_lifecycle_reservation",
            "Create/start reservation phase is inconsistent.",
        ));
    }
    match stored.status.as_str() {
        "pending" => Ok(()),
        "rejected" => Err(stored_rejection(
            stored.failure_code.clone(),
            stored.failure_message.clone(),
        )),
        _ => Err(rejected(
            "stale_lifecycle_reservation",
            "Create/start reservation status is invalid.",
        )),
    }
}

fn validate_prepared_result(result: &Value, session_id: &str) -> Result<(), PersistenceError> {
    let valid = result.get("status").and_then(Value::as_str) == Some("created")
        && result
            .pointer("/agent_session/session_id")
            .and_then(Value::as_str)
            == Some(session_id)
        && result
            .pointer("/participant/participant_id")
            .and_then(Value::as_str)
            == Some(session_id);
    if valid {
        Ok(())
    } else {
        Err(rejected(
            "stale_lifecycle_reservation",
            "Prepared create result is inconsistent with its Agent Session.",
        ))
    }
}

fn prepared_events(result: &Value) -> Result<Vec<RoomEvent>, PersistenceError> {
    if let Some(events) = result.get("events") {
        return Ok(serde_json::from_value(events.clone())?);
    }
    result.get("event").map_or_else(
        || Ok(Vec::new()),
        |event| Ok(vec![serde_json::from_value(event.clone())?]),
    )
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "agent_create_start_tests.rs"]
mod tests;
