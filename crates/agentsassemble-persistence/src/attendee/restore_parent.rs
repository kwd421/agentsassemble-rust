//! Rebind retained unredeemed invites only to the same creator's fresh authority.
use super::{invites::rejected, session::require_parent};
use crate::PersistenceError;
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

pub(super) async fn restore_parent(
    tx: &mut Transaction<'_, Sqlite>,
    row: &SqliteRow,
    now: DateTime<Utc>,
) -> Result<Option<Vec<u8>>, PersistenceError> {
    let previous: Option<Vec<u8>> = row.try_get("parent_fingerprint")?;
    let kind: Option<&str> = row.try_get("restore_parent_kind")?;
    let Some(kind) = kind else {
        return Ok(previous);
    };
    if previous.is_some()
        || row
            .try_get::<Option<Vec<u8>>, _>("session_fingerprint")?
            .is_some()
    {
        return Err(rejected(
            "invalid_state",
            "Restored parent custody is inconsistent.",
        ));
    }
    let room: &str = row.try_get("room_id")?;
    let user: &str = row.try_get("creator_id")?;
    let participant: &str = row.try_get("owner_participant_id")?;
    // Each provenance has one authority owner. Never fall from an invalid human
    // session through to local operator authority, or choose a different user.
    let query = match kind {
        "human" => {
            "SELECT session_fingerprint FROM human_room_sessions WHERE room_id=? AND user_id=? AND participant_id=? AND state='active' AND expires_at>? ORDER BY admitted_at DESC,session_fingerprint LIMIT 1"
        }
        "operator" => {
            "SELECT session_fingerprint FROM operator_pairings WHERE room_id=? AND user_id=? AND participant_id=? AND revoked=0 AND session_fingerprint IS NOT NULL AND (session_expires_at=0 OR session_expires_at>?) ORDER BY last_connected_at DESC,pairing_id LIMIT 1"
        }
        _ => {
            return Err(rejected(
                "invalid_state",
                "Restored parent provenance is invalid.",
            ));
        }
    };
    let fingerprint: Option<Vec<u8>> = sqlx::query_scalar(query)
        .bind(room)
        .bind(user)
        .bind(participant)
        .bind(now.timestamp_micros())
        .fetch_optional(&mut **tx)
        .await?;
    let fingerprint = fingerprint.ok_or_else(|| {
        rejected(
            "parent_login_required",
            "The invitation's creator must sign in to this room again.",
        )
    })?;
    require_parent(tx, room, &fingerprint, now).await?;
    sqlx::query("UPDATE room_attendee_invites SET parent_fingerprint=?,restore_parent_kind=NULL WHERE invite_id=? AND parent_fingerprint IS NULL AND session_fingerprint IS NULL AND restore_parent_kind=?")
        .bind(&fingerprint).bind(row.try_get::<&str,_>("invite_id")?).bind(kind).execute(&mut **tx).await?;
    Ok(Some(fingerprint))
}
