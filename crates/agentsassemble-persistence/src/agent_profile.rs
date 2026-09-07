use std::collections::BTreeMap;

use agentsassemble_domain::{
    AuthenticatedPrincipal, DurableAgentSession, Participant, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_avatar_assets::replace_agent_avatar,
    agent_lifecycle::{load_participant, load_session, save_participant, save_session},
    agent_lifecycle_authority::{authorize_control, payload_agent_id_with_fields},
    agent_lifecycle_events::{append_session_event, append_state_event, store_result},
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
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
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
        let mut transaction = self.pool.begin().await?;
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
            transaction.commit().await?;
            return Ok(outcome);
        }
        let (mut session, mut participant) =
            load_profile_target(&mut transaction, &principal.room_id, &agent_id).await?;
        if let Some(avatar) = avatar {
            replace_agent_avatar(&mut transaction, &session.public, avatar).await?;
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
        save_session(&mut transaction, &session).await?;
        save_participant(&mut transaction, &participant).await?;
        let participant_event = append_session_event(
            &mut transaction,
            principal,
            &session.public,
            "participant_updated",
            BTreeMap::from([(
                "avatar_image_url".to_owned(),
                json!(session.public.avatar_image_url),
            )]),
        )
        .await?;
        let session_event =
            append_state_event(&mut transaction, principal, &session.public).await?;
        let events = vec![participant_event, session_event];
        let result = json!({
            "agent_session": session.public,
            "participant": participant,
            "events": events,
            "event": events.last(),
        });
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
        transaction.commit().await?;
        Ok(outcome)
    }
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

pub(crate) async fn load_profile_target(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    agent_id: &str,
) -> Result<(DurableAgentSession, Participant), PersistenceError> {
    let session = load_session(transaction, room_id, agent_id).await?;
    let participant =
        load_participant(transaction, room_id, &session.public.participant_id).await?;
    if participant.participant_type != "agent" {
        return Err(rejected(
            "stored_agent_identity_invalid",
            "Agent profile requires an Agent participant.",
        ));
    }
    Ok((session, participant))
}

fn profile_name(value: &Value) -> Result<&str, PersistenceError> {
    let name = value
        .as_str()
        .ok_or_else(|| rejected("bad_request", "display_name must be a string."))?
        .trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(rejected(
            "bad_request",
            "display_name must contain 1 to 80 characters without controls.",
        ));
    }
    Ok(name)
}
