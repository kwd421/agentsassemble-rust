//! Person-derived companion removal uses existing runtime cleanup custody.
use agentsassemble_domain::RoomEvent;
use chrono::Utc;
use sqlx::{Row, Sqlite, Transaction};

use crate::{MemberRemovalKey, PersistenceError, RoomRuntimeCleanupKey, SqliteStore};

pub struct MemberRemovalCompanionPage {
    pub room_sessions: Vec<(String, [u8; 32])>,
    pub cleanup: Option<RoomRuntimeCleanupKey>,
    pub events: Vec<RoomEvent>,
    pub next_phase: String,
}

impl SqliteStore {
    /// Removes one actual parent-bound companion; independent local/server AI stays outside scope.
    /// # Errors
    /// Invalid committed phase, unknown custody and persistence errors retain unfinished work.
    pub async fn advance_member_removal_companions(
        &self,
        key: &MemberRemovalKey,
    ) -> Result<MemberRemovalCompanionPage, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (phase, cursor, user) = committed_companion_job(&mut tx, key).await?;
        let row=sqlx::query("SELECT invite_id,room_id,participant_id,session_fingerprint FROM room_attendee_invites i WHERE invite_id>? AND (EXISTS(SELECT 1 FROM human_room_sessions h WHERE h.session_fingerprint=i.parent_fingerprint AND h.user_id=?) OR EXISTS(SELECT 1 FROM operator_pairings p JOIN host_owner_sessions h ON h.fingerprint=p.host_owner_session_fingerprint WHERE p.session_fingerprint=i.parent_fingerprint AND h.person_id=?)) ORDER BY invite_id LIMIT 1")
            .bind(&cursor).bind(user).bind(key.person_id()).fetch_optional(&mut *tx).await?;
        let mut result = MemberRemovalCompanionPage {
            room_sessions: Vec::new(),
            cleanup: None,
            events: Vec::new(),
            next_phase: phase.clone(),
        };
        let last = if let Some(row) = row {
            let invite: String = row.try_get("invite_id")?;
            let room: String = row.try_get("room_id")?;
            let actor: Option<String> = row.try_get("participant_id")?;
            let fingerprint: Option<Vec<u8>> = row.try_get("session_fingerprint")?;
            require_companion_descriptor(
                key,
                &invite,
                &room,
                actor.as_deref(),
                fingerprint.as_deref(),
            )?;
            sqlx::query("UPDATE room_attendee_invites SET revoked=1 WHERE invite_id=?")
                .bind(&invite)
                .execute(&mut *tx)
                .await?;
            if let Some(actor) = actor {
                let cleanup = RoomRuntimeCleanupKey {
                    room_id: room.clone(),
                    session_id: actor.clone(),
                };
                crate::room_runtime_cleanup::request_member_runtime_cleanup(&mut tx, &cleanup)
                    .await?;
                let changed=sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.status','left','$.updated_at',?) WHERE room_id=? AND participant_id=? AND json_extract(participant_json,'$.participant_type')='agent'")
                    .bind(Utc::now().to_rfc3339()).bind(&room).bind(&actor).execute(&mut *tx).await?;
                if changed.rows_affected() != 1 {
                    return Err(invalid());
                }
                let fingerprint = crate::human_session_authority::fixed_session_fingerprint(
                    fingerprint.ok_or_else(invalid)?,
                )?;
                result.room_sessions.push((room.clone(), fingerprint));
                let participant =
                    crate::agent_lifecycle::load_participant(&mut tx, &room, &actor).await?;
                let event =
                    crate::participant_leave::participant_left_event(&mut tx, &participant).await?;
                if serde_json::to_vec(&event)?.len() > 64 * 1024 {
                    return Err(invalid());
                }
                crate::room_turns::support::insert_event(&mut tx, &event).await?;
                result.events.push(event);
                result.events.extend(
                    crate::provider_request_lifecycle::cancel_removed_companion_in(
                        &mut tx,
                        &cleanup.room_id,
                        &cleanup.session_id,
                    )
                    .await?,
                );
                result.cleanup = Some(cleanup);
            } else if fingerprint.is_some() {
                return Err(invalid());
            }
            invite
        } else {
            result.next_phase = "member_invites".into();
            String::new()
        };
        if serde_json::to_vec(&(
            key.issuer(),
            key.person_id(),
            &last,
            &result.events,
            &result.room_sessions,
        ))?
        .len()
            + 1024
            > 128 * 1024
        {
            return Err(invalid());
        }
        crate::central_removal_authority::checkpoint(
            &mut tx,
            key,
            &phase,
            &result.next_phase,
            &last,
        )
        .await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(result)
    }
}
fn invalid() -> PersistenceError {
    PersistenceError::CommandUnresolved {code:"account_removal_companion_unconfirmed".into(),message:"Person-derived companion cleanup requires intact local custody and its committed phase.".into()}
}

fn require_companion_descriptor(
    key: &MemberRemovalKey,
    invite: &str,
    room: &str,
    actor: Option<&str>,
    fingerprint: Option<&[u8]>,
) -> Result<(), PersistenceError> {
    if invite.len() > 128
        || room.len() > 128
        || actor.is_some_and(|value| value.len() > 128)
        || serde_json::to_vec(&(
            key.issuer(),
            key.person_id(),
            invite,
            room,
            actor,
            fingerprint,
        ))?
        .len()
            > 64 * 1024
    {
        return Err(invalid());
    }
    Ok(())
}

async fn committed_companion_job(
    tx: &mut Transaction<'_, Sqlite>,
    key: &MemberRemovalKey,
) -> Result<(String, String, Option<String>), PersistenceError> {
    crate::central_member_removal::require_schema(tx).await?;
    let job=sqlx::query("SELECT phase,cursor,user_id FROM central_member_removals WHERE issuer=? AND person_id=? AND schema_revision=87")
            .bind(key.issuer()).bind(key.person_id()).fetch_optional(&mut **tx).await?.ok_or_else(invalid)?;
    let phase: String = job.try_get("phase")?;
    if phase != "companions" {
        return Err(invalid());
    }
    let cursor: String = job.try_get("cursor")?;
    let user: Option<String> = job.try_get("user_id")?;
    Ok((phase, cursor, user))
}
