use crate::participant_rows::save_participant_exact as save_participant;
use agentsassemble_domain::{
    AGENT_PROFILE_NAME_CHARACTER_LIMIT, DurableAgentSession, Participant, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_avatar_assets::{AvatarHolder, replace_agent_avatar},
    agent_lifecycle::{load_participant, load_session, save_session},
    agent_lifecycle_authority::{authorize_control, payload_agent_id_with_fields},
    agent_lifecycle_events::{append_profile_events, store_result},
    authority::active_room_for_principal,
    command_admission::admit_non_lifecycle_command,
    room_write_budget::command_size,
};

const ACTION: &str = "agent.profile.update";

impl SqliteStore {
    /// Updates Agent identity and its room participant projection in one durable command.
    ///
    /// # Errors
    /// Returns authorization, malformed identity, replay conflict or storage failures.
    pub async fn execute_agent_profile_update(
        &self,
        authorization: crate::RoomMutationAuthority<'_>,
        request_id: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let principal = &authorization.resolve(&mut transaction).await?;
        authorize_control(principal)?;
        let agent_id =
            payload_agent_id_with_fields(payload, &["display_name", "avatar_image_url"])?;
        if payload.get("display_name").is_none() && payload.get("avatar_image_url").is_none() {
            return Err(rejected(
                "bad_request",
                "At least one Agent identity field is required.",
            ));
        }
        let name = payload.get("display_name").map(profile_name).transpose()?;
        let avatar = payload
            .get("avatar_image_url")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| rejected("bad_request", "avatar_image_url must be a string."))
            })
            .transpose()?;
        let payload_hash = canonical_payload_hash(payload);
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            ACTION,
            &payload_hash,
            command_size(request_id, ACTION, payload)?,
        )
        .await?
        {
            authorization.record_success(&mut transaction).await?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        let target = load_profile_target(&mut transaction, &principal.room_id, &agent_id).await?;
        if let Some(avatar) = avatar {
            let current = match &target {
                ProfileTarget::Session(session, _) => session.public.avatar_image_url.clone(),
                ProfileTarget::Connector(participant) => participant.avatar_image_url.clone(),
            };
            replace_agent_avatar(
                &mut transaction,
                AvatarHolder::of(&target),
                &current,
                avatar,
            )
            .await?;
        }
        let (result, events) = match target {
            ProfileTarget::Session(session, participant) => {
                update_session(
                    &mut transaction,
                    principal,
                    *session,
                    participant,
                    name,
                    avatar,
                )
                .await?
            }
            ProfileTarget::Connector(participant) => {
                update_connector(&mut transaction, principal, participant, name, avatar).await?
            }
        };
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            ACTION,
            payload_hash,
            result,
            events,
        )
        .await?;
        authorization.record_success(&mut transaction).await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn update_session(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    mut session: DurableAgentSession,
    mut participant: Participant,
    name: Option<&str>,
    avatar: Option<&str>,
) -> Result<(Value, Vec<agentsassemble_domain::RoomEvent>), PersistenceError> {
    if let Some(avatar) = avatar {
        avatar.clone_into(&mut session.public.avatar_image_url);
    }
    if let Some(name) = name {
        name.clone_into(&mut session.public.display_name);
    }
    session.public.updated_at = Utc::now();
    participant
        .display_name
        .clone_from(&session.public.display_name);
    participant
        .avatar_image_url
        .clone_from(&session.public.avatar_image_url);
    participant.updated_at = session.public.updated_at;
    save_session(transaction, &session).await?;
    save_participant(
        transaction,
        &participant.room_id,
        &participant.participant_id,
        &participant,
    )
    .await?;
    let [participant_event, session_event] =
        append_profile_events(transaction, principal, &session.public).await?;
    let public_session = session_event.extra["agent_session"].clone();
    let events = vec![participant_event, session_event];
    let result = json!({
        "agent_session": public_session,
        "participant": participant,
        "events": events,
        "event": events.last(),
    });
    Ok((result, events))
}

/// An external AI has only its room participant, so one update event carries
/// the new name and picture.
async fn update_connector(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    mut participant: Participant,
    name: Option<&str>,
    avatar: Option<&str>,
) -> Result<(Value, Vec<agentsassemble_domain::RoomEvent>), PersistenceError> {
    if let Some(avatar) = avatar {
        avatar.clone_into(&mut participant.avatar_image_url);
    }
    if let Some(name) = name {
        name.clone_into(&mut participant.display_name);
    }
    participant.updated_at = Utc::now();
    save_participant(
        transaction,
        &participant.room_id,
        &participant.participant_id,
        &participant,
    )
    .await?;
    let event = connector_profile_event(transaction, principal, &participant).await?;
    let events = vec![event];
    let result = json!({
        "participant": participant,
        "events": events,
        "event": events.last(),
    });
    Ok((result, events))
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

/// An Agent Session, or an external AI, which has a room participant and no session.
pub(crate) enum ProfileTarget {
    Session(Box<DurableAgentSession>, Participant),
    Connector(Participant),
}

pub(crate) async fn load_profile_target(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    agent_id: &str,
) -> Result<ProfileTarget, PersistenceError> {
    if crate::agent_session_rows::load_optional_agent_session_row(transaction, room_id, agent_id)
        .await?
        .is_none()
    {
        // Every AI the app runs has a session; an external AI has none and
        // joined through a Room Connector invite. Anything else is refused, so
        // an app AI whose session row went missing fails instead of being edited
        // as an external AI.
        let participant = load_participant(transaction, room_id, agent_id).await?;
        let joined_by_connector: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM room_connector_invites WHERE room_id = ? AND participant_id = ?)",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_one(&mut **transaction)
        .await?;
        if participant.participant_type != "agent" || !joined_by_connector {
            return Err(rejected(
                "not_found",
                &format!("Agent session {agent_id} was not found."),
            ));
        }
        // An external AI that left or was removed keeps its row only for history.
        if !matches!(
            participant.status,
            agentsassemble_domain::ParticipantStatus::Joined
                | agentsassemble_domain::ParticipantStatus::Detached
        ) {
            return Err(rejected(
                "participant_inactive",
                "This AI is no longer in the room.",
            ));
        }
        return Ok(ProfileTarget::Connector(participant));
    }
    let session = load_session(transaction, room_id, agent_id).await?;
    let participant =
        load_participant(transaction, room_id, &session.public.participant_id).await?;
    if participant.participant_type != "agent" {
        return Err(rejected(
            "stored_agent_identity_invalid",
            "Agent profile requires an Agent participant.",
        ));
    }
    Ok(ProfileTarget::Session(Box::new(session), participant))
}

async fn connector_profile_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    participant: &Participant,
) -> Result<agentsassemble_domain::RoomEvent, PersistenceError> {
    let actor_type = if principal.client_kind == agentsassemble_domain::ClientKind::Browser {
        "human"
    } else {
        "agent"
    };
    let event = agentsassemble_domain::RoomEvent {
        v: 1,
        id: uuid::Uuid::new_v4().to_string(),
        seq: crate::room_event_sequence::next_sequence(transaction, &principal.room_id).await?,
        created_at: participant.updated_at,
        room_id: principal.room_id.clone(),
        event_type: "participant_updated".to_owned(),
        actor: agentsassemble_domain::Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: actor_type.to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some("agent".to_owned()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some(actor_type.to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: std::collections::BTreeMap::from([(
            "avatar_image_url".to_owned(),
            json!(participant.avatar_image_url),
        )]),
    };
    crate::room_turns::support::insert_event(transaction, &event).await?;
    Ok(event)
}

fn profile_name(value: &Value) -> Result<&str, PersistenceError> {
    let name = value
        .as_str()
        .ok_or_else(|| rejected("bad_request", "display_name must be a string."))?
        .trim();
    if name.is_empty()
        || name.chars().count() > AGENT_PROFILE_NAME_CHARACTER_LIMIT
        || name.chars().any(char::is_control)
    {
        return Err(rejected(
            "bad_request",
            &format!(
                "display_name must contain 1 to {AGENT_PROFILE_NAME_CHARACTER_LIMIT} characters without controls."
            ),
        ));
    }
    Ok(name)
}
