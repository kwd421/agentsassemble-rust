//! The action-bound proof stays private; no operator principal reaches an AI.
use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, InviteScope, RoomEvent, SetMessagePin, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{CommandOutcome, PersistenceError, SqliteStore};

/// Authenticated AI custody supplied by the transport owner, never tool arguments.
#[derive(Clone)]
pub enum DelegateRoomAuthority {
    Provider {
        room_id: String,
        session_id: String,
        turn_id: String,
        input_up_to_seq: i64,
        turn_generation: u64,
        execution_id: String,
    },
    Attendee {
        session: crate::AttendeeSessionAuthorization,
        connection_id: Uuid,
        turn_generation: u64,
        execution_id: String,
    },
    Connector(crate::ConnectorSessionAuthorization),
}

impl DelegateRoomAuthority {
    #[must_use]
    pub fn room_id(&self) -> &str {
        match self {
            Self::Provider { room_id, .. } => room_id,
            Self::Attendee { session, .. } => &session.principal().room_id,
            Self::Connector(session) => &session.principal().room_id,
        }
    }

    fn delegate_id(&self) -> &str {
        match self {
            Self::Provider { session_id, .. } => session_id,
            Self::Attendee { session, .. } => &session.principal().participant_id,
            Self::Connector(session) => &session.principal().participant_id,
        }
    }

    async fn resolve(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        match self {
            Self::Provider {
                room_id,
                session_id,
                turn_id,
                input_up_to_seq,
                turn_generation,
                execution_id,
            } => {
                let session = crate::agent_lifecycle::load_session(tx, room_id, session_id).await?;
                crate::turn_authority::require_provider_room_tool_authority(
                    tx,
                    &session,
                    turn_id,
                    *input_up_to_seq,
                    *turn_generation,
                    execution_id,
                )
                .await?;
                let participant =
                    crate::authority::load_active_participant(tx, room_id, session_id).await?;
                crate::room_turns::support::provider_room_principal(
                    &session,
                    &participant,
                    InviteScope::ReadWrite,
                )
            }
            Self::Attendee {
                session,
                connection_id,
                turn_generation,
                execution_id,
            } => {
                let connection = crate::AttendeeConnectionAuthorization {
                    session: session.clone(),
                    connection_id: *connection_id,
                };
                crate::attendee::connection::revalidate_in(tx, &connection, Utc::now()).await?;
                crate::attendee::tool_authority::load_turn_in(
                    tx,
                    &connection,
                    *turn_generation,
                    execution_id,
                )
                .await?;
                Ok(session.principal().clone())
            }
            Self::Connector(session) => {
                Ok(
                    crate::connector_session::revalidate_in(tx, session, Utc::now())
                        .await?
                        .principal()
                        .clone(),
                )
            }
        }
    }
}

pub(crate) struct DelegatedRoomActionAuthority {
    pub(crate) owner: AuthenticatedPrincipal,
    pub(crate) delegate: AuthenticatedPrincipal,
}
impl DelegatedRoomActionAuthority {
    pub(crate) async fn pin(
        tx: &mut Transaction<'_, Sqlite>,
        authority: &DelegateRoomAuthority,
    ) -> Result<Self, PersistenceError> {
        let owner = crate::delegation_owner::resolve_pin_owner(
            tx,
            authority.room_id(),
            authority.delegate_id(),
        )
        .await?;
        let delegate = authority.resolve(tx).await?;
        if delegate.invite_scope != InviteScope::ReadWrite {
            return Err(rejected(
                "delegate_authority_expired",
                "The delegate has read-only authority.",
            ));
        }
        Ok(Self { owner, delegate })
    }
}

impl SqliteStore {
    /// Revalidates both current authorities for discovery without conferring execution rights.
    /// # Errors
    /// Propagates storage failures; ended authority makes the tool unavailable.
    pub async fn delegated_pin_available(
        &self,
        authority: &DelegateRoomAuthority,
    ) -> Result<bool, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let proof = DelegatedRoomActionAuthority::pin(&mut tx, authority).await;
        tx.commit().await?;
        match proof {
            Ok(_) => Ok(true),
            Err(
                PersistenceError::CommandRejected { .. }
                | PersistenceError::RoomMissing
                | PersistenceError::ParticipantMissing,
            ) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Commits a pin, exact receipt and attributed system line under both live authorities.
    /// The admission callback debits the existing process budgets while the transaction
    /// owns the current identities. Its receipt is returned only after commit.
    /// # Errors
    /// Rejects stale custody, role/scope/mute changes, conflicting retries or invalid targets.
    pub async fn set_delegated_message_pin<A>(
        &self,
        authority: &DelegateRoomAuthority,
        request: &SetMessagePin,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(CommandOutcome, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&request.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let payload = serde_json::to_value(request)?;
        let hash = canonical_payload_hash(&payload);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let proof = DelegatedRoomActionAuthority::pin(&mut tx, authority).await?;
        let delegate = &proof.delegate;
        if let Some(outcome) = crate::command_admission::inspect_non_lifecycle_command(
            &mut tx,
            &delegate.room_id,
            &delegate.principal_id,
            &request.request_id,
            "set_message_pin",
            &hash,
        )
        .await?
        {
            tx.commit().await?;
            return Ok((outcome, None));
        }
        crate::message_pins::set_pin(
            &mut tx,
            &proof.owner.room_id,
            &request.channel_id,
            &request.message_id,
            request.pinned,
            Utc::now(),
        )
        .await?;
        let bytes = crate::room_write_budget::command_size(
            &request.request_id,
            "set_message_pin",
            &payload,
        )?;
        crate::room_write_budget::reserve_room_write_budget(&mut tx, &delegate.room_id, bytes)
            .await?;
        let admission = admit(&proof.owner.principal_id, &delegate.principal_id, bytes)?;
        let mut event = attributed_action_event(
            &mut tx,
            &proof,
            &request.request_id,
            "set_message_pin",
            &request.message_id,
            if request.pinned {
                "pinned a message"
            } else {
                "unpinned a message"
            },
        )
        .await?;
        event.extra.insert("pinned".into(), json!(request.pinned));
        event
            .extra
            .insert("target_channel_id".into(), json!(request.channel_id));
        crate::room_turns::support::insert_event(&mut tx, &event).await?;
        let result = json!({"status":"succeeded", "request_id":request.request_id,
            "action":"set_message_pin", "target_id":request.message_id, "event_id":event.id, "event":event});
        crate::command_admission::store_command_result(
            &mut tx,
            (&delegate.room_id, &delegate.principal_id),
            &request.request_id,
            "set_message_pin",
            &hash,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok((
            CommandOutcome {
                result,
                event: event.clone(),
                events: vec![event],
                deduplicated: false,
            },
            Some(admission),
        ))
    }
}

pub(crate) async fn attributed_action_event(
    tx: &mut Transaction<'_, Sqlite>,
    proof: &DelegatedRoomActionAuthority,
    request_id: &str,
    action: &str,
    target_id: &str,
    description: &str,
) -> Result<RoomEvent, PersistenceError> {
    let delegate = &proof.delegate;
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: crate::room_event_sequence::next_sequence(tx, &delegate.room_id).await?,
        created_at: Utc::now(),
        room_id: delegate.room_id.clone(),
        event_type: "message_final".into(),
        actor: Actor {
            participant_id: delegate.participant_id.clone(),
            participant_type: "agent".into(),
        },
        participant_id: Some(delegate.participant_id.clone()),
        participant_type: Some("agent".into()),
        actor_id: Some(delegate.participant_id.clone()),
        actor_type: Some("agent".into()),
        display_name: Some(delegate.display_name.clone()),
        content: Some(format!(
            "{} {} on behalf of {}.",
            delegate.display_name, description, proof.owner.display_name
        )),
        message_kind: Some("system".into()),
        extra: [
            (
                "actor_participant_id".into(),
                json!(delegate.participant_id),
            ),
            (
                "on_behalf_of_participant_id".into(),
                json!(proof.owner.participant_id),
            ),
            ("action".into(), json!(action)),
            ("target_id".into(), json!(target_id)),
            ("request_id".into(), json!(request_id)),
        ]
        .into(),
    })
}

fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "delegated_pin_tests.rs"]
mod tests;
