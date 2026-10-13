//! Action-bound lifecycle custody. This cannot authorize any other room command.
use agentsassemble_domain::{AuthenticatedPrincipal, DirectRoomAction, canonical_payload_hash};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::delegated_pin::DelegatedRoomActionAuthority;
use crate::{DelegateRoomAuthority, PersistenceError, RoomMutationAuthority, SqliteStore};

pub struct DelegatedAgentControl {
    pub(crate) authority: DelegateRoomAuthority,
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) owner_id: String,
    pub(crate) request_id: String,
    pub(crate) action: &'static str,
    pub(crate) payload: Value,
    pub(crate) confirmation_id: Option<String>,
}

impl DelegatedAgentControl {
    #[must_use]
    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }
    #[must_use]
    pub fn payload(&self) -> &Value {
        &self.payload
    }
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    #[must_use]
    pub fn operation_id(&self) -> String {
        crate::agent_lifecycle_authority::lifecycle_operation_id(
            &self.principal,
            &self.request_id,
            self.action,
        )
    }
    #[must_use]
    pub const fn action(&self) -> &'static str {
        self.action
    }
}

/// Only the lifecycle owners accept this proof; general room mutations do not.
#[derive(Clone, Copy)]
pub enum AgentControlAuthority<'a> {
    Human(RoomMutationAuthority<'a>),
    Delegated(&'a DelegatedAgentControl),
}
impl<'a> From<RoomMutationAuthority<'a>> for AgentControlAuthority<'a> {
    fn from(authority: RoomMutationAuthority<'a>) -> Self {
        Self::Human(authority)
    }
}

impl AgentControlAuthority<'_> {
    pub(crate) async fn resolve(
        self,
        tx: &mut Transaction<'_, Sqlite>,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        match self {
            Self::Human(authority) => {
                // The private envelope can originate only at the delegated admission owner.
                if payload.get("_delegation").is_some() {
                    return Err(invalid());
                }
                let principal = authority.resolve(tx).await?.into_owned();
                crate::agent_lifecycle_authority::authorize_control(&principal)?;
                Ok(principal)
            }
            Self::Delegated(control) => {
                if request_id != control.request_id
                    || action != control.action
                    || payload != &control.payload
                {
                    return Err(invalid());
                }
                if control.confirmation_id.is_some() {
                    return crate::delegated_confirmed_agent::authorize_in(tx, control).await;
                }
                let name = direct_name(action)?;
                let proof =
                    DelegatedRoomActionAuthority::direct(tx, &control.authority, name).await?;
                if proof.owner.participant_id != control.owner_id
                    || proof.delegate.principal_id != control.principal.principal_id
                {
                    return Err(invalid());
                }
                crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
                require_target(tx, &proof.delegate.room_id, action, payload).await?;
                Ok(proof.delegate)
            }
        }
    }

    pub(crate) async fn record_success(
        self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), PersistenceError> {
        if let Self::Human(authority) = self {
            authority.record_success(tx).await?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Attribution {
    pub(crate) actor_participant_id: String,
    pub(crate) on_behalf_of_participant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) confirmation_id: Option<String>,
}

impl SqliteStore {
    /// Admits one action-bound lifecycle request, retaining its exact two identities.
    /// The lifecycle owner revalidates this proof inside reservation and pre-effect writes.
    /// # Errors
    /// Returns current authority, permission, replay, target or budget failures.
    pub async fn admit_delegated_agent_control<A>(
        &self,
        authority: &DelegateRoomAuthority,
        request: &DirectRoomAction,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(DelegatedAgentControl, Option<A>), PersistenceError> {
        let (action, agent_id) = match request {
            DirectRoomAction::StartAgent(r) => ("agent.start", &r.agent_id),
            DirectRoomAction::StopAgent(r) => ("agent.stop", &r.agent_id),
            _ => return Err(invalid()),
        };
        if !uuid::Uuid::parse_str(request.request_id()).is_ok_and(|id| !id.is_nil()) {
            return Err(invalid());
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let proof =
            DelegatedRoomActionAuthority::direct(&mut tx, authority, request.name()).await?;
        crate::agent_lifecycle_authority::authorize_control(&proof.owner)?;
        crate::delegated_confirmation_lifecycle::reject_confirmation_identity(
            &mut tx,
            authority,
            request.request_id(),
        )
        .await?;
        let payload = json!({"agent_id":agent_id,"_delegation":Attribution {
            actor_participant_id: proof.delegate.participant_id.clone(),
            on_behalf_of_participant_id: proof.owner.participant_id.clone(),
            confirmation_id: None,
        }});
        let identity = crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            request.request_id(),
            action,
            &canonical_payload_hash(&payload),
        )
        .await?;
        require_target(&mut tx, authority.room_id(), action, &payload).await?;
        let debit = if matches!(
            identity,
            None | Some(crate::command_admission::ExistingRequestIdentity::RejectedLifecycle)
        ) {
            Some(admit(
                &proof.owner.principal_id,
                &proof.delegate.principal_id,
                crate::room_write_budget::command_size(request.request_id(), action, &payload)?,
            )?)
        } else {
            None
        };
        tx.commit().await?;
        Ok((
            DelegatedAgentControl {
                authority: authority.clone(),
                principal: proof.delegate,
                owner_id: proof.owner.participant_id,
                request_id: request.request_id().into(),
                action,
                payload,
                confirmation_id: None,
            },
            debit,
        ))
    }
}

async fn require_target(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    action: &str,
    payload: &Value,
) -> Result<(), PersistenceError> {
    let id = control_agent_id(payload)?;
    let session = crate::agent_lifecycle::load_session(tx, room_id, &id).await?;
    if action == "agent.start" && session.public.permission_mode != "meeting_read_only" {
        return Err(PersistenceError::CommandRejected {
            code: "owner_confirmation_required".into(),
            message: "Starting this agent needs the owner's confirmation because it permits workspace write or full access.".into(),
        });
    }
    Ok(())
}

pub(crate) fn control_agent_id(payload: &Value) -> Result<String, PersistenceError> {
    crate::agent_lifecycle_authority::payload_agent_id_with_fields(payload, &["_delegation"])
}

// Recovery records an already authorized effect; it never grants fresh execution authority.
pub(crate) fn valid_recorded_actor(
    principal: &AuthenticatedPrincipal,
    action: &str,
    payload: &Value,
) -> bool {
    let Some(metadata) = payload.get("_delegation") else {
        return principal.capabilities.agent_control;
    };
    let Ok(attribution) = serde_json::from_value::<Attribution>(metadata.clone()) else {
        return false;
    };
    direct_name(action).is_ok()
        && principal.client_kind != agentsassemble_domain::ClientKind::Browser
        && attribution.actor_participant_id == principal.participant_id
        && !attribution.on_behalf_of_participant_id.is_empty()
        && attribution.on_behalf_of_participant_id != principal.participant_id
}

fn direct_name(action: &str) -> Result<&'static str, PersistenceError> {
    match action {
        "agent.start" => Ok("start_agent"),
        "agent.stop" => Ok("stop_agent"),
        _ => Err(invalid()),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn store_control_result(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: &str,
    payload: &Value,
    mut result: Value,
    mut events: Vec<agentsassemble_domain::RoomEvent>,
) -> Result<crate::CommandOutcome, PersistenceError> {
    if let Some(metadata) = payload.get("_delegation") {
        let attribution: Attribution = serde_json::from_value(metadata.clone())?;
        let name = direct_name(action)?;
        if attribution.actor_participant_id != principal.participant_id {
            return Err(invalid());
        }
        let owner = crate::agent_lifecycle::load_participant(
            tx,
            &principal.room_id,
            &attribution.on_behalf_of_participant_id,
        )
        .await?;
        let mut event = crate::delegated_pin::attributed_action_event(
            tx,
            crate::delegated_pin::ActionAttribution {
                room_id: &principal.room_id,
                actor_id: &principal.participant_id,
                actor_name: &principal.display_name,
                owner_id: &owner.participant_id,
                owner_name: &owner.display_name,
            },
            request_id,
            name,
            &control_agent_id(payload)?,
            if action == "agent.start" {
                "started an agent"
            } else {
                "stopped an agent"
            },
        )
        .await?;
        if let Some(id) = &attribution.confirmation_id {
            event.extra.insert("confirmation_id".into(), json!(id));
        }
        for canonical in &mut events {
            canonical.actor.clone_from(&event.actor);
            canonical.actor_id.clone_from(&event.actor_id);
            canonical.actor_type.clone_from(&event.actor_type);
            for key in [
                "actor_participant_id",
                "on_behalf_of_participant_id",
                "action",
                "target_id",
                "request_id",
            ] {
                canonical.extra.insert(key.into(), event.extra[key].clone());
            }
            if let Some(id) = &attribution.confirmation_id {
                canonical.extra.insert("confirmation_id".into(), json!(id));
            }
            sqlx::query("UPDATE room_events SET event_json=? WHERE room_id=? AND seq=?")
                .bind(serde_json::to_string(canonical)?)
                .bind(&principal.room_id)
                .bind(canonical.seq)
                .execute(&mut **tx)
                .await?;
        }
        crate::room_turns::support::insert_event(tx, &event).await?;
        events.push(event.clone());
        result = json!({"status":"succeeded", "request_id":request_id, "action":name,
            "target_id":control_agent_id(payload)?, "event_id":event.id, "event":event, "events":events});
    }
    if let Some(id) = payload
        .get("_delegation")
        .and_then(|a| a.get("confirmation_id"))
    {
        result["confirmation_id"] = id.clone();
    }
    crate::delegated_confirmed_agent::complete_in(
        tx,
        &principal.room_id,
        payload,
        agentsassemble_domain::ConfirmationState::Succeeded,
        &agentsassemble_domain::public_value_for_principal(&result, principal)?,
    )
    .await?;
    crate::agent_lifecycle_events::store_result(
        tx,
        principal,
        request_id,
        action,
        canonical_payload_hash(payload),
        result,
        events,
    )
    .await
}

fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "bad_request".into(),
        message: "Invalid delegated lifecycle request.".into(),
    }
}
