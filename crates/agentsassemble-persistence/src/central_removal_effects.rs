//! Replayable live effects; checkpoint only after the existing runtime owners finish.
use sqlx::Row;

use crate::{MemberRemovalKey, PersistenceError, RoomRuntimeCleanupKey, SqliteStore};

pub struct MemberRemovalEffectPage {
    pub owner_fingerprints: Vec<[u8; 32]>,
    pub room_sessions: Vec<(String, [u8; 32])>,
    pub cleanup: Option<RoomRuntimeCleanupKey>,
    old_phase: String,
    old_cursor: String,
    next_phase: String,
    next_cursor: String,
}

impl SqliteStore {
    /// Existing local lifecycle wake/startup enumerates only committed unfinished work.
    /// # Errors
    /// Database/schema errors remain errors, never an empty successful inventory.
    pub async fn pending_member_removals(
        &self,
        after: Option<&MemberRemovalKey>,
    ) -> Result<Vec<MemberRemovalKey>, PersistenceError> {
        let rows=sqlx::query("SELECT issuer,person_id FROM central_member_removals WHERE phase!='complete' AND (? IS NULL OR issuer>? OR (issuer=? AND person_id>?)) ORDER BY issuer,person_id LIMIT 32")
            .bind(after.map(MemberRemovalKey::issuer)).bind(after.map(MemberRemovalKey::issuer))
            .bind(after.map(MemberRemovalKey::issuer)).bind(after.map(MemberRemovalKey::person_id)).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(MemberRemovalKey {
                    issuer: row.try_get("issuer")?,
                    person_id: row.try_get("person_id")?,
                })
            })
            .collect()
    }

    /// Reads one exact effect without advancing its cursor; interruption safely repeats it.
    /// # Errors
    /// Invalid phase/custody or oversized descriptor leaves durable work unfinished.
    pub async fn member_removal_effect(
        &self,
        key: &MemberRemovalKey,
    ) -> Result<MemberRemovalEffectPage, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        let job=sqlx::query("SELECT phase,cursor,user_id FROM central_member_removals WHERE issuer=? AND person_id=?")
            .bind(key.issuer()).bind(key.person_id()).fetch_one(&mut *tx).await?;
        let old_phase: String = job.try_get("phase")?;
        let old_cursor: String = job.try_get("cursor")?;
        let user: Option<String> = job.try_get("user_id")?;
        let phase = if old_phase == "effects" {
            "effect_owners"
        } else {
            &old_phase
        };
        let row=match phase {
            "effect_owners"=>sqlx::query("SELECT hex(fingerprint) AS id,'' AS room_id,fingerprint FROM host_owner_sessions WHERE person_id=? AND hex(fingerprint)>? ORDER BY hex(fingerprint) LIMIT 1")
                .bind(key.person_id()).bind(&old_cursor).fetch_optional(&mut *tx).await?,
            "effect_pairings"=>sqlx::query("SELECT pairing_id AS id,room_id,session_fingerprint AS fingerprint FROM operator_pairings p WHERE pairing_id>? AND EXISTS(SELECT 1 FROM host_owner_sessions h WHERE h.fingerprint=p.host_owner_session_fingerprint AND h.person_id=?) ORDER BY pairing_id LIMIT 1")
                .bind(&old_cursor).bind(key.person_id()).fetch_optional(&mut *tx).await?,
            "effect_humans"=>sqlx::query("SELECT hex(admission_key) AS id,room_id,session_fingerprint AS fingerprint FROM human_room_sessions WHERE user_id=? AND hex(admission_key)>? ORDER BY hex(admission_key) LIMIT 1")
                .bind(&user).bind(&old_cursor).fetch_optional(&mut *tx).await?,
            "effect_companions"=>sqlx::query("SELECT i.invite_id AS id,i.room_id,i.participant_id,i.session_fingerprint AS fingerprint FROM room_attendee_invites i WHERE i.invite_id>? AND i.participant_id IS NOT NULL AND (EXISTS(SELECT 1 FROM human_room_sessions h WHERE h.session_fingerprint=i.parent_fingerprint AND h.user_id=?) OR EXISTS(SELECT 1 FROM operator_pairings p JOIN host_owner_sessions h ON h.fingerprint=p.host_owner_session_fingerprint WHERE p.session_fingerprint=i.parent_fingerprint AND h.person_id=?)) ORDER BY i.invite_id LIMIT 1")
                .bind(&old_cursor).bind(&user).bind(key.person_id()).fetch_optional(&mut *tx).await?,
            _=>return Err(crate::central_removal_snapshots::invalid()),
        };
        let mut page = MemberRemovalEffectPage {
            owner_fingerprints: Vec::new(),
            room_sessions: Vec::new(),
            cleanup: None,
            old_phase: old_phase.clone(),
            old_cursor: old_cursor.clone(),
            next_phase: phase.into(),
            next_cursor: String::new(),
        };
        if let Some(row) = row {
            page.next_cursor = row.try_get("id")?;
            let room: String = row.try_get("room_id")?;
            let fingerprint: Option<Vec<u8>> = row.try_get("fingerprint")?;
            if let Some(fingerprint) = fingerprint {
                let fingerprint =
                    crate::human_session_authority::fixed_session_fingerprint(fingerprint)?;
                if phase == "effect_owners" {
                    page.owner_fingerprints.push(fingerprint);
                } else {
                    page.room_sessions.push((room.clone(), fingerprint));
                }
            }
            if phase == "effect_companions" {
                page.cleanup = Some(RoomRuntimeCleanupKey {
                    room_id: room,
                    session_id: row.try_get("participant_id")?,
                });
            }
        } else {
            page.next_phase = match phase {
                "effect_owners" => "effect_pairings",
                "effect_pairings" => "effect_humans",
                "effect_humans" => "effect_companions",
                "effect_companions" => "complete",
                _ => return Err(crate::central_removal_snapshots::invalid()),
            }
            .into();
        }
        if serde_json::to_vec(&(
            key.issuer(),
            key.person_id(),
            &page.next_phase,
            &page.next_cursor,
            &page.room_sessions,
            &page.owner_fingerprints,
            page.cleanup.as_ref().map(|k| (&k.room_id, &k.session_id)),
        ))?
        .len()
            > 128 * 1024
        {
            return Err(crate::central_removal_snapshots::invalid());
        }
        tx.commit().await?;
        Ok(page)
    }

    /// Runtime custody must be physically settled before this exact page can advance.
    /// # Errors
    /// Pending cleanup or conflicting checkpoint retains the work and fails explicitly.
    pub async fn finish_member_removal_effect(
        &self,
        key: &MemberRemovalKey,
        page: &MemberRemovalEffectPage,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(cleanup) = &page.cleanup
            && crate::room_runtime_cleanup::cleanup_exists(
                &mut tx,
                &cleanup.room_id,
                &cleanup.session_id,
            )
            .await?
        {
            return Err(crate::central_removal_snapshots::invalid());
        }
        let changed=sqlx::query("UPDATE central_member_removals SET phase=?,cursor=? WHERE issuer=? AND person_id=? AND phase=? AND cursor=? AND schema_revision=87")
            .bind(&page.next_phase).bind(&page.next_cursor).bind(key.issuer()).bind(key.person_id()).bind(&page.old_phase).bind(&page.old_cursor).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(crate::central_removal_snapshots::invalid());
        }
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(())
    }

    /// Event-driven secure transport cutoff for member channels, including idle HTTP.
    /// # Errors
    /// Invalid database authority fails closed at the connection owner.
    pub async fn secure_channel_member_removed(
        &self,
        channel: &str,
    ) -> Result<bool, PersistenceError> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM human_room_sessions h JOIN central_member_removals r USING(user_id) WHERE h.secure_channel_id=?)")
            .bind(channel).fetch_one(&self.pool).await?)
    }
}
