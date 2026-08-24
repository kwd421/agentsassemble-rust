use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, RoomAppearance, RoomEvent, RoomSettings, RoomSettingsPatch,
    canonical_payload_hash, public_settings,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, SqliteStore, authority::active_room_for_principal,
    command_admission::admit_non_lifecycle_command,
};

trait AssetReferenceTransition {
    fn validate_and_plan(
        &self,
        current: &RoomAppearance,
        next: &RoomAppearance,
        patch: &RoomSettingsPatch,
    ) -> Result<(), PersistenceError>;
}

struct StageANoAssetReferences;

impl AssetReferenceTransition for StageANoAssetReferences {
    fn validate_and_plan(
        &self,
        current: &RoomAppearance,
        next: &RoomAppearance,
        _patch: &RoomSettingsPatch,
    ) -> Result<(), PersistenceError> {
        if current.banner_image_url != next.banner_image_url
            || current.icon_image_url != next.icon_image_url
        {
            return Err(rejected(
                "room_setting_unsupported",
                "Room appearance images are unavailable until their asset owner exists.",
            ));
        }
        Ok(())
    }
}

impl SqliteStore {
    /// Commits one canonical room-global settings command.
    ///
    /// # Errors
    ///
    /// Returns authorization, revision, validation, availability, replay, or storage failures.
    pub async fn execute_room_settings_update(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        let mut room = active_room_for_principal(&mut transaction, principal).await?;
        if !principal.capabilities.room_manage {
            return Err(rejected(
                "permission_denied",
                "This room session cannot manage room settings.",
            ));
        }
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            "room.settings.update",
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let encoded =
            sqlx::query_scalar::<_, String>("SELECT settings_json FROM rooms WHERE room_id = ?")
                .bind(&principal.room_id)
                .fetch_one(&mut *transaction)
                .await?;
        let current: RoomSettings = serde_json::from_str(&encoded)?;
        let (expected_revision, next, patch) = current
            .stage_a_update(payload)
            .map_err(|error| rejected(error.code, error.message))?;
        let current_public = public_settings(&current)?;
        if expected_revision != current_public.settings_revision {
            return Err(rejected(
                "settings_conflict",
                "Room settings changed before this update was applied.",
            ));
        }
        StageANoAssetReferences.validate_and_plan(&current.appearance, &next.appearance, &patch)?;
        let now = Utc::now();
        room.label.clone_from(&next.label);
        room.updated_at = now;
        let public = public_settings(&next)?;
        let event = RoomEvent {
            v: 1,
            id: Uuid::new_v4().to_string(),
            seq: next_sequence(&mut transaction, &principal.room_id).await?,
            created_at: now,
            room_id: principal.room_id.clone(),
            event_type: "room_settings_updated".to_owned(),
            actor: Actor {
                participant_id: principal.participant_id.clone(),
                participant_type: "human".to_owned(),
            },
            participant_id: Some(principal.participant_id.clone()),
            participant_type: Some("human".to_owned()),
            actor_id: Some(principal.participant_id.clone()),
            actor_type: Some("human".to_owned()),
            display_name: Some(principal.display_name.clone()),
            content: None,
            message_kind: None,
            extra: BTreeMap::from([("room_settings".to_owned(), json!(public))]),
        };
        let result = json!({"room_settings": public, "event": event});
        sqlx::query("UPDATE rooms SET room_json = ?, settings_json = ? WHERE room_id = ?")
            .bind(serde_json::to_string(&room)?)
            .bind(serde_json::to_string(&next)?)
            .bind(&principal.room_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
            .bind(&principal.room_id)
            .bind(event.seq)
            .bind(serde_json::to_string(&event)?)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES (?, ?, ?, 'room.settings.update', ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(request_id)
        .bind(payload_hash)
        .bind(serde_json::to_string(&result)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CommandOutcome {
            result,
            event: event.clone(),
            events: vec![event],
            deduplicated: false,
        })
    }
}

async fn next_sequence(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
) -> Result<i64, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?)
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
