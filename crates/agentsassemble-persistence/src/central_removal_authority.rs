//! Bounded access-revocation pages in the existing person-local removal job.
use sqlx::{Row, Sqlite, Transaction};

use crate::{MemberRemovalKey, PersistenceError, SqliteStore};

const PAGE: u8 = 32;
const DESCRIPTOR_BYTES: usize = 128 * 1024;

pub struct MemberRemovalAuthorityPage {
    pub owner_fingerprints: Vec<[u8; 32]>,
    pub room_sessions: Vec<(String, [u8; 32])>,
    pub next_phase: String,
}

impl SqliteStore {
    /// Advances only committed access work; the terminal person fence already owns authority.
    /// # Errors
    /// Missing work, unknown phase/schema, invalid stored custody and write failures stay errors.
    pub async fn advance_member_removal_authority(
        &self,
        key: &MemberRemovalKey,
    ) -> Result<MemberRemovalAuthorityPage, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        let job = sqlx::query("SELECT phase,cursor,user_id FROM central_member_removals WHERE issuer=? AND person_id=? AND schema_revision=87")
            .bind(key.issuer()).bind(key.person_id()).fetch_optional(&mut *tx).await?
            .ok_or_else(invalid)?;
        let phase: String = job.try_get("phase")?;
        let cursor: String = job.try_get("cursor")?;
        let user: Option<String> = job.try_get("user_id")?;
        let rows = match phase.as_str() {
            "owner_sessions" => sqlx::query("SELECT hex(fingerprint) AS id,'' AS room_id,fingerprint AS session_fingerprint FROM host_owner_sessions WHERE person_id=? AND hex(fingerprint)>? ORDER BY hex(fingerprint) LIMIT ?")
                .bind(key.person_id()).bind(&cursor).bind(i64::from(PAGE)).fetch_all(&mut *tx).await?,
            "owner_pairings" => sqlx::query("SELECT pairing_id AS id,room_id,session_fingerprint FROM operator_pairings WHERE pairing_id>? AND EXISTS(SELECT 1 FROM host_owner_sessions h WHERE h.fingerprint=operator_pairings.host_owner_session_fingerprint AND h.person_id=?) ORDER BY pairing_id LIMIT ?")
                .bind(&cursor).bind(key.person_id()).bind(i64::from(PAGE)).fetch_all(&mut *tx).await?,
            "human_sessions" => sqlx::query("SELECT hex(admission_key) AS id,room_id,session_fingerprint FROM human_room_sessions WHERE user_id=? AND hex(admission_key)>? ORDER BY hex(admission_key) LIMIT ?")
                .bind(user).bind(&cursor).bind(i64::from(PAGE)).fetch_all(&mut *tx).await?,
            _ => return Err(invalid()),
        };
        let mut result = MemberRemovalAuthorityPage {
            owner_fingerprints: Vec::new(),
            room_sessions: Vec::new(),
            next_phase: phase.clone(),
        };
        let mut last = cursor;
        let mut descriptors = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            let room: String = row.try_get("room_id")?;
            let fingerprint: Option<Vec<u8>> = row.try_get("session_fingerprint")?;
            let fingerprint = fingerprint
                .map(crate::human_session_authority::fixed_session_fingerprint)
                .transpose()?;
            descriptors.push((id.clone(), room.clone(), fingerprint));
            // Encoded descriptors include notification custody and cursor, never stored replay JSON.
            if serde_json::to_vec(&descriptors)?.len()
                + id.len()
                + phase.len()
                + key.issuer().len()
                + key.person_id().len()
                + usize::from(PAGE) * 64
                > DESCRIPTOR_BYTES
            {
                return Err(invalid());
            }
            match phase.as_str() {
                "owner_sessions" => {
                    sqlx::query("UPDATE host_owner_sessions SET revoked=1,connected=0,device_name='',browser='',os='' WHERE hex(fingerprint)=? AND person_id=?")
                        .bind(&id).bind(key.person_id()).execute(&mut *tx).await?;
                    result
                        .owner_fingerprints
                        .push(fingerprint.ok_or_else(invalid)?);
                }
                "owner_pairings" => {
                    sqlx::query("UPDATE operator_pairings SET revoked=1,device_name='',browser='',os='' WHERE pairing_id=?")
                        .bind(&id).execute(&mut *tx).await?;
                    if let Some(fingerprint) = fingerprint {
                        result.room_sessions.push((room.clone(), fingerprint));
                    }
                }
                "human_sessions" => {
                    sqlx::query(
                        "UPDATE human_room_sessions SET state='ended' WHERE hex(admission_key)=?",
                    )
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
                    result
                        .room_sessions
                        .push((room.clone(), fingerprint.ok_or_else(invalid)?));
                }
                _ => return Err(invalid()),
            }
            last.clone_from(&id);
        }
        if rows.len() < usize::from(PAGE) {
            result.next_phase = match phase.as_str() {
                "owner_sessions" => "owner_pairings",
                "owner_pairings" => "human_sessions",
                "human_sessions" => "companions",
                _ => return Err(invalid()),
            }
            .into();
            last.clear();
        }
        checkpoint(&mut tx, key, &phase, &result.next_phase, &last).await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(result)
    }
}

pub(crate) async fn checkpoint(
    tx: &mut Transaction<'_, Sqlite>,
    key: &MemberRemovalKey,
    old: &str,
    new: &str,
    cursor: &str,
) -> Result<(), PersistenceError> {
    let result = sqlx::query("UPDATE central_member_removals SET phase=?,cursor=? WHERE issuer=? AND person_id=? AND phase=? AND schema_revision=87")
        .bind(new).bind(cursor).bind(key.issuer()).bind(key.person_id()).bind(old).execute(&mut **tx).await?;
    if result.rows_affected() != 1 {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: "account_removal_phase_unhandled".into(),
        message: "Person-local removal requires valid committed work and its exact schema phase."
            .into(),
    }
}
