//! Existing-agent start binds the sealed configuration and uses the lifecycle owner's gate.
use crate::delegated_confirmation::{Binding, create_in, rejected};
use crate::delegated_confirmation_lifecycle::{item, load_in, receipt, refresh_in};
use crate::{DelegateRoomAuthority, DelegatedAgentControl, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    AgentStartConfiguration, ConfirmationState, ConfirmedAgentStart, ConfirmedRoomAction,
    DelegatedActionRequest, DurableAgentSession, StartAgent, canonical_payload_hash,
};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

fn normalise(session: &DurableAgentSession, request: &StartAgent) -> ConfirmedAgentStart {
    let s = &session.public;
    ConfirmedAgentStart {
        request_id: request.request_id.clone(),
        agent_id: request.agent_id.clone(),
        expected_configuration_revision: session.runtime_profile_key.clone(),
        expected_created_at: s.created_at,
        configuration: AgentStartConfiguration {
            provider_kind: s.provider_kind.clone(),
            runtime_kind: s.runtime_kind.clone(),
            connection_kind: s.connection_kind.clone(),
            model: s.model.clone(),
            reasoning_effort: s.reasoning_effort.clone(),
            service_tier: s.service_tier.clone(),
            variant: s.variant.clone(),
            execution_harness: s.execution_harness.clone(),
            permission_mode: s.permission_mode.clone(),
            max_output_tokens: s.max_output_tokens,
            catalog_revision: s.catalog_revision.clone(),
            persona_card_id: s.persona_card_id.to_string(),
            transport: s.transport.clone(),
            working_folder: session.workspace.clone(),
        },
    }
}

pub(crate) async fn require_target_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    request: &DelegatedActionRequest,
) -> Result<(), PersistenceError> {
    let ConfirmedRoomAction::StartAgent(start) = &request.action else {
        return Err(rejected("bad_request", "An agent start is required."));
    };
    let session = crate::agent_lifecycle::load_session(tx, room_id, &start.agent_id)
        .await
        .map_err(|e| match e {
            PersistenceError::CommandRejected { code, .. } if code == "not_found" => {
                rejected("target_changed", "The target agent no longer exists.")
            }
            other => other,
        })?;
    if normalise(
        &session,
        &StartAgent {
            request_id: start.request_id.clone(),
            agent_id: start.agent_id.clone(),
        },
    ) != *start
        || session.public.display_name != request.target_name
    {
        return Err(rejected(
            "target_changed",
            "The target agent configuration or identity changed.",
        ));
    }
    Ok(())
}

impl SqliteStore {
    /// Routes execution-capable starts to confirmation, without reserving or launching.
    /// Conversation-only targets continue through the direct lifecycle owner, which rechecks mode.
    /// # Errors
    /// Rejects stale bindings, conflicting identities, invalid targets and admission exhaustion.
    pub async fn request_delegated_agent_start<A>(
        &self,
        authority: &DelegateRoomAuthority,
        request: &StartAgent,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<Option<(Value, Option<A>)>, PersistenceError> {
        if !uuid::Uuid::parse_str(&request.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row)=sqlx::query("SELECT * FROM delegated_action_requests WHERE room_id=? AND delegate_id=? AND request_id=?").bind(authority.room_id()).bind(authority.delegate_id()).bind(&request.request_id).fetch_optional(&mut *tx).await? {
            let binding:Binding=serde_json::from_str(row.get("binding_json"))?;
            let stored=item(&row)?;
            if !binding.authority.same_binding(authority) || !matches!(&stored.action,ConfirmedRoomAction::StartAgent(s) if s.request_id==request.request_id && s.agent_id==request.agent_id) { return Err(rejected("request_conflict","This request ID identifies another action or execution.")); }
            refresh_in(&mut tx,authority.room_id(),&row).await?;
            let row=load_in(&mut tx,authority.room_id(),&stored.confirmation_id).await?;
            let result=receipt(&row)?;
            tx.commit().await?;
            return Ok(Some((result,None)));
        }
        let proof = crate::delegated_pin::DelegatedRoomActionAuthority::direct(
            &mut tx,
            authority,
            "start_agent",
        )
        .await?;
        // A request ID already owned by the direct lifecycle namespace replays there;
        // a changed payload under it conflicts before any target load.
        let direct_payload = json!({"agent_id":request.agent_id,"_delegation":crate::delegated_agent_control::Attribution {
            actor_participant_id: proof.delegate.participant_id.clone(),
            on_behalf_of_participant_id: proof.owner.participant_id.clone(),
            confirmation_id: None,
        }});
        if crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &request.request_id,
            "agent.start",
            &canonical_payload_hash(&direct_payload),
        )
        .await?
        .is_some()
        {
            tx.commit().await?;
            return Ok(None);
        }
        crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
        let session =
            crate::agent_lifecycle::load_session(&mut tx, authority.room_id(), &request.agent_id)
                .await?;
        if session.public.permission_mode == "meeting_read_only" {
            tx.commit().await?;
            return Ok(None);
        }
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &request.request_id,
            "start_agent",
            &canonical_payload_hash(&serde_json::to_value(request)?),
        )
        .await
        .map_err(|e| match e {
            PersistenceError::CommandConflict => rejected(
                "request_conflict",
                "This request ID already identifies another action.",
            ),
            other => other,
        })?;
        let action = ConfirmedRoomAction::StartAgent(normalise(&session, request));
        let result=create_in(&mut tx,authority,&proof,action,session.public.display_name.clone(),"Starts this configured agent, may spend provider credit and permits workspace write or host execution under its existing permission level.",admit).await?;
        tx.commit().await?;
        Ok(Some(result))
    }

    /// Loads only an already approved, reserved start for the room actor.
    /// The lifecycle owner revalidates original authorities and configuration before effects.
    /// # Errors
    /// Rejects absent/unapproved actions and propagates storage failures.
    pub async fn confirmed_agent_start(
        &self,
        room_id: &str,
        confirmation_id: &str,
    ) -> Result<DelegatedAgentControl, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let row = load_in(&mut tx, room_id, confirmation_id).await?;
        let control = control_in(&mut tx, room_id, &row).await?;
        tx.commit().await?;
        Ok(control)
    }
}

// Construction confers no owner rights. AgentControlAuthority::resolve owns authorization.
pub(crate) async fn control_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    row: &SqliteRow,
) -> Result<DelegatedAgentControl, PersistenceError> {
    let request = item(row)?;
    let ConfirmedRoomAction::StartAgent(action) = &request.action else {
        return Err(rejected("bad_request", "An agent start is required."));
    };
    let decision: agentsassemble_domain::DecideDelegatedAction = serde_json::from_str(
        row.get::<Option<&str>, _>("decision_json")
            .ok_or_else(|| rejected("permission_denied", "The owner has not approved."))?,
    )?;
    if request.state != ConfirmationState::Executing
        || decision.decision != agentsassemble_domain::ConfirmationDecision::Approve
    {
        return Err(rejected(
            "permission_denied",
            "The owner has not approved this execution.",
        ));
    }
    let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
    let principal = match &binding.authority {
        DelegateRoomAuthority::Connector(session) => session.principal().clone(),
        DelegateRoomAuthority::Attendee { session, .. } => session.principal().clone(),
        DelegateRoomAuthority::Provider { session_id, .. } => {
            let session = crate::agent_lifecycle::load_session(tx, room_id, session_id).await?;
            let participant =
                crate::authority::load_active_participant(tx, room_id, session_id).await?;
            crate::room_turns::support::provider_room_principal(
                &session,
                &participant,
                agentsassemble_domain::InviteScope::ReadWrite,
            )?
        }
    };
    let payload = json!({"agent_id":action.agent_id,"_delegation":crate::delegated_agent_control::Attribution {actor_participant_id:request.delegate_participant_id.clone(),on_behalf_of_participant_id:request.owner_participant_id.clone(),confirmation_id:Some(request.confirmation_id.clone())}});
    Ok(DelegatedAgentControl {
        authority: binding.authority,
        principal,
        owner_id: request.owner_participant_id,
        request_id: action.request_id.clone(),
        action: "agent.start",
        payload,
        confirmation_id: Some(request.confirmation_id),
    })
}

pub(crate) async fn authorize_in(
    tx: &mut Transaction<'_, Sqlite>,
    control: &DelegatedAgentControl,
) -> Result<agentsassemble_domain::AuthenticatedPrincipal, PersistenceError> {
    let id = control
        .confirmation_id
        .as_deref()
        .ok_or_else(|| rejected("permission_denied", "Confirmation is required."))?;
    let row = load_in(tx, control.authority.room_id(), id).await?;
    let request = item(&row)?;
    if request.state != ConfirmationState::Executing
        || control_in(tx, control.authority.room_id(), &row)
            .await?
            .payload
            != control.payload
    {
        return Err(rejected(
            "request_conflict",
            "This execution is not the approved action.",
        ));
    }
    let proof =
        crate::delegated_confirmation_lifecycle::validate_in(tx, control.authority.room_id(), &row)
            .await?;
    crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
    require_target_in(tx, control.authority.room_id(), &request).await?;
    Ok(proof.delegate)
}

pub(crate) async fn complete_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    payload: &Value,
    state: ConfirmationState,
    result: &Value,
) -> Result<(), PersistenceError> {
    if let Some(id) = payload
        .get("_delegation")
        .and_then(|a| a.get("confirmation_id"))
        .and_then(Value::as_str)
    {
        let row = load_in(tx, room_id, id).await?;
        if item(&row)?.state == ConfirmationState::Executing {
            crate::delegated_confirmation_lifecycle::finish_in(tx, room_id, &row, state, result)
                .await?;
        }
    }
    Ok(())
}
