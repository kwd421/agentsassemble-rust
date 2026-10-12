use crate::delegated_pin::{DelegatedRoomActionAuthority, attributed_action_event};
use crate::{DelegateRoomAuthority, ParticipantMuteMutation, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    DirectRoomAction, RoomChannel, RoomSettings, canonical_payload_hash, public_settings,
};
use chrono::Utc;
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

impl DelegatedRoomActionAuthority {
    pub(crate) async fn direct(
        tx: &mut Transaction<'_, Sqlite>,
        authority: &DelegateRoomAuthority,
        action: &str,
    ) -> Result<Self, PersistenceError> {
        let proof = Self::pin(tx, authority).await?;
        if !proof.allows(action) {
            return Err(rejected(
                "owner_not_permitted",
                "The owner is not permitted to perform this action.",
            ));
        }
        Ok(proof)
    }
    fn allows(&self, action: &str) -> bool {
        match action {
            "set_message_pin" => true,
            "create_text_channel" | "rename_text_channel" => self.owner.capabilities.room_manage,
            "set_participant_mute" => self.owner.capabilities.participant_mute,
            "start_agent" | "stop_agent" => self.owner.capabilities.agent_control,
            _ => false,
        }
    }
}

impl SqliteStore {
    /// Fresh action-specific discovery; calls still resolve authority transactionally.
    /// # Errors
    /// Propagates storage failures.
    pub async fn delegated_direct_actions(
        &self,
        authority: &DelegateRoomAuthority,
    ) -> Result<Vec<String>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let proof = match DelegatedRoomActionAuthority::pin(&mut tx, authority).await {
            Ok(proof) => Some(proof),
            Err(
                PersistenceError::CommandRejected { .. }
                | PersistenceError::RoomMissing
                | PersistenceError::ParticipantMissing,
            ) => None,
            Err(error) => return Err(error),
        };
        let actions = std::iter::once("set_message_pin")
            .chain(DirectRoomAction::NAMES.iter().copied())
            .filter(|action| proof.as_ref().is_some_and(|proof| proof.allows(action)))
            .map(str::to_owned)
            .collect();
        tx.commit().await?;
        Ok(actions)
    }

    /// Applies a channel delta or mute through the human mutation core.
    /// # Errors
    /// Rejects ended authority, conflicting retries, invalid targets and admission failures.
    pub async fn execute_delegated_atomic<A>(
        &self,
        authority: &DelegateRoomAuthority,
        request: &DirectRoomAction,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(ParticipantMuteMutation, Option<A>), PersistenceError> {
        let request_id = request.request_id();
        if !Uuid::parse_str(request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let proof =
            DelegatedRoomActionAuthority::direct(&mut tx, authority, request.name()).await?;
        if let DirectRoomAction::SetParticipantMute(r) = request
            && r.muted
            && r.participant_id == proof.owner.participant_id
        {
            return Err(rejected(
                "owner_not_permitted",
                "A delegate cannot mute its bound human owner.",
            ));
        }
        let payload = serde_json::to_value(request)?;
        let hash = canonical_payload_hash(
            &json!({"request":payload,"owner":proof.owner.participant_id,"delegate":proof.delegate.participant_id}),
        );
        if let Some(mut outcome) = crate::command_admission::inspect_non_lifecycle_command(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            request_id,
            request.name(),
            &hash,
        )
        .await?
        {
            outcome.result = agentsassemble_domain::public_value_for_principal(
                &outcome.result,
                &proof.delegate,
            )?;
            tx.commit().await?;
            return Ok((
                ParticipantMuteMutation {
                    outcome,
                    assignments: Vec::new(),
                    host_interrupt_effect: None,
                },
                None,
            ));
        }
        let (mut mutation, target, description) = match request {
            DirectRoomAction::StartAgent(_) | DirectRoomAction::StopAgent(_) => {
                return Err(rejected("bad_request", "Use the lifecycle mutation owner."));
            }
            DirectRoomAction::CreateTextChannel(_) | DirectRoomAction::RenameTextChannel(_) => {
                let (delta, target) = channel_delta(&mut tx, authority.room_id(), request).await?;
                let outcome =
                    crate::room_settings::update_settings_in(&mut tx, &proof.owner, &delta).await?;
                (
                    ParticipantMuteMutation {
                        outcome,
                        assignments: Vec::new(),
                        host_interrupt_effect: None,
                    },
                    target,
                    "updated a text channel",
                )
            }
            DirectRoomAction::SetParticipantMute(r) => (
                crate::participant_mute::mute_in(
                    &mut tx,
                    &proof.owner,
                    &json!({"participant_id":r.participant_id,"muted":r.muted}),
                )
                .await?,
                r.participant_id.clone(),
                if r.muted {
                    "muted a participant"
                } else {
                    "unmuted a participant"
                },
            ),
        };
        let bytes = crate::room_write_budget::command_size(request_id, request.name(), &payload)?;
        crate::room_write_budget::reserve_room_write_budget(&mut tx, authority.room_id(), bytes)
            .await?;
        let debit = admit(
            &proof.owner.principal_id,
            &proof.delegate.principal_id,
            bytes,
        )?;
        let event = attributed_action_event(
            &mut tx,
            proof.attribution(),
            request_id,
            request.name(),
            &target,
            description,
        )
        .await?;
        let canonical_id = mutation.outcome.event.id.clone();
        for canonical in mutation
            .outcome
            .events
            .iter_mut()
            .filter(|event| event.id == canonical_id)
        {
            for key in [
                "actor_participant_id",
                "on_behalf_of_participant_id",
                "action",
                "target_id",
                "request_id",
            ] {
                canonical.extra.insert(key.into(), event.extra[key].clone());
            }
            sqlx::query("UPDATE room_events SET event_json=? WHERE room_id=? AND seq=?")
                .bind(serde_json::to_string(canonical)?)
                .bind(authority.room_id())
                .bind(canonical.seq)
                .execute(&mut *tx)
                .await?;
        }
        crate::room_turns::support::insert_event(&mut tx, &event).await?;
        mutation.outcome.events.push(event.clone());
        mutation.outcome.event = event.clone();
        mutation.outcome.result = json!({"status":"succeeded","request_id":request_id,"action":request.name(),"target_id":target,"event_id":event.id,"event":event,"events":mutation.outcome.events});
        crate::command_admission::store_command_result(
            &mut tx,
            (authority.room_id(), &proof.delegate.principal_id),
            request_id,
            request.name(),
            &hash,
            &mutation.outcome.result,
        )
        .await?;
        mutation.outcome.result = agentsassemble_domain::public_value_for_principal(
            &mutation.outcome.result,
            &proof.delegate,
        )?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok((mutation, Some(debit)))
    }
}

async fn channel_delta(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    request: &DirectRoomAction,
) -> Result<(serde_json::Value, String), PersistenceError> {
    let encoded: String = sqlx::query_scalar("SELECT settings_json FROM rooms WHERE room_id=?")
        .bind(room_id)
        .fetch_one(&mut **tx)
        .await?;
    let mut current: RoomSettings = serde_json::from_str(&encoded)?;
    let revision = public_settings(&current)?.settings_revision;
    let (expected, target) = match request {
        DirectRoomAction::CreateTextChannel(r) => {
            let id = format!("c{}", &Uuid::new_v4().simple().to_string()[..12]);
            current.channels.push(RoomChannel {
                id: id.clone(),
                name: r.name.clone(),
                channel_type: "text".into(),
                position: current.channels.len() as u32,
                created_at: Utc::now().to_rfc3339(),
            });
            (revision, id)
        }
        DirectRoomAction::RenameTextChannel(r) => {
            let channel = current
                .channels
                .iter_mut()
                .find(|channel| channel.id == r.channel_id)
                .ok_or_else(|| {
                    rejected(
                        "channel_not_found",
                        "The text channel does not exist in this room.",
                    )
                })?;
            channel.name.clone_from(&r.name);
            (r.expected_settings_revision.clone(), r.channel_id.clone())
        }
        _ => return Err(rejected("bad_request", "A channel delta is required.")),
    };
    Ok((
        json!({"expected_revision":expected,"channels":current.channels}),
        target,
    ))
}
fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}
