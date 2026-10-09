//! Sequenced room handoff in the same durable person-removal job.
use agentsassemble_domain::{ParticipantStatus, RoomEvent};
use sqlx::Row;

use crate::{MemberRemovalKey, PersistenceError, SqliteStore};

pub struct MemberRemovalRoomPage {
    pub room_id: String,
    pub event_seq: i64,
}

impl SqliteStore {
    /// Returns only committed work and its current phase; never new removal authority.
    /// # Errors
    /// Missing work and invalid schema are explicit errors.
    pub async fn member_removal_phase(
        &self,
        key: &MemberRemovalKey,
    ) -> Result<String, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        let phase = sqlx::query_scalar("SELECT phase FROM central_member_removals WHERE issuer=? AND person_id=? AND schema_revision=87")
            .bind(key.issuer()).bind(key.person_id()).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(phase)
    }

    /// Emits each room's irreversible removal/anonymous author once, after stored snapshots.
    /// # Errors
    /// A pending handoff survives interruption; invalid phase/JSON remains an error.
    pub async fn advance_member_removal_room(
        &self,
        key: &MemberRemovalKey,
    ) -> Result<Option<MemberRemovalRoomPage>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        let job = sqlx::query("SELECT phase,cursor,participant_id FROM central_member_removals WHERE issuer=? AND person_id=?")
            .bind(key.issuer()).bind(key.person_id()).fetch_one(&mut *tx).await?;
        let phase: String = job.try_get("phase")?;
        let cursor: String = job.try_get("cursor")?;
        if phase == "room_publish" {
            let (room_id, event_seq): (String, i64) = serde_json::from_str(&cursor)?;
            return Ok(Some(MemberRemovalRoomPage { room_id, event_seq }));
        }
        if phase != "rooms" {
            return Err(crate::central_removal_snapshots::invalid());
        }
        let actor: Option<String> = job.try_get("participant_id")?;
        let room: Option<String> = sqlx::query_scalar("SELECT room_id FROM participants WHERE participant_id=? AND room_id>? ORDER BY room_id LIMIT 1")
            .bind(&actor).bind(&cursor).fetch_optional(&mut *tx).await?;
        let Some(room_id) = room else {
            crate::central_removal_authority::checkpoint(&mut tx, key, &phase, "effects", "")
                .await?;
            tx.commit().await?;
            return Ok(None);
        };
        let actor = actor.ok_or_else(crate::central_removal_snapshots::invalid)?;
        if room_id.len() > 128 || actor.len() > 128 {
            return Err(crate::central_removal_snapshots::invalid());
        }
        // Remove PII inside SQLite; load only the canonical, now small participant descriptor.
        sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.status','left','$.display_name',?,'$.avatar_image_url','','$.avatar_label','','$.updated_at',?) WHERE room_id=? AND participant_id=? AND json_extract(participant_json,'$.participant_type')='human'")
            .bind(crate::DEPARTED_USER_NAME).bind(chrono::Utc::now().to_rfc3339()).bind(&room_id).bind(&actor).execute(&mut *tx).await?;
        let bytes: i64 = sqlx::query_scalar("SELECT length(CAST(participant_json AS BLOB)) FROM participants WHERE room_id=? AND participant_id=?")
            .bind(&room_id).bind(&actor).fetch_one(&mut *tx).await?;
        if bytes > 64 * 1024 {
            return Err(crate::central_removal_snapshots::invalid());
        }
        let participant =
            crate::agent_lifecycle::load_participant(&mut tx, &room_id, &actor).await?;
        if participant.participant_type != "human" || participant.status != ParticipantStatus::Left
        {
            return Err(crate::central_removal_snapshots::invalid());
        }
        let left = crate::participant_leave::participant_left_event(&mut tx, &participant).await?;
        crate::room_turns::support::insert_event(&mut tx, &left).await?;
        let event = anonymous_event(&mut tx, &participant).await?;
        crate::room_turns::support::insert_event(&mut tx, &event).await?;
        let handoff = serde_json::to_string(&(&room_id, event.seq))?;
        crate::central_removal_authority::checkpoint(
            &mut tx,
            key,
            &phase,
            "room_publish",
            &handoff,
        )
        .await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(Some(MemberRemovalRoomPage {
            room_id,
            event_seq: event.seq,
        }))
    }

    /// The existing publisher's durable sequence, not a client ACK, settles the handoff.
    /// # Errors
    /// Publication not yet committed or a conflicting cursor stays pending.
    pub async fn finish_member_removal_room(
        &self,
        key: &MemberRemovalKey,
        page: &MemberRemovalRoomPage,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let published: Option<i64> = sqlx::query_scalar(
            "SELECT published_seq FROM room_event_publication_cursors WHERE room_id=?",
        )
        .bind(&page.room_id)
        .fetch_optional(&mut *tx)
        .await?;
        if published.is_none_or(|seq| seq < page.event_seq) {
            return Err(crate::central_removal_snapshots::invalid());
        }
        let cursor = serde_json::to_string(&(&page.room_id, page.event_seq))?;
        let changed = sqlx::query("UPDATE central_member_removals SET phase='rooms',cursor=? WHERE issuer=? AND person_id=? AND phase='room_publish' AND cursor=? AND schema_revision=87")
            .bind(&page.room_id).bind(key.issuer()).bind(key.person_id()).bind(cursor).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(crate::central_removal_snapshots::invalid());
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn anonymous_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    participant: &agentsassemble_domain::Participant,
) -> Result<RoomEvent, PersistenceError> {
    let mut event = crate::participant_leave::participant_left_event(tx, participant).await?;
    event.event_type = agentsassemble_domain::PARTICIPANT_ANONYMIZED_EVENT_TYPE.into();
    event
        .extra
        .insert("avatar_image_url".into(), serde_json::json!(""));
    event
        .extra
        .insert("avatar_label".into(), serde_json::json!(""));
    Ok(event)
}
