//! Account-scoped device management. Native host authority remains outside revocable sessions.
use chrono::Utc;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, ServerOwnerAuthority, SqliteStore,
    host_owner_session::{fingerprint_column, invalid},
};

pub struct OwnerDeviceSession {
    pub session_id: Uuid,
    pub device_name: String,
    pub browser: String,
    pub os: String,
    pub last_connected_at: Option<i64>,
    pub current: bool,
    pub connected: Option<bool>,
    pub pairing: bool,
}

pub struct OwnerDevicesRevocation {
    pub revoked_count: usize,
    pub owner_fingerprints: Vec<[u8; 32]>,
    pub room_sessions: Vec<(String, [u8; 32])>,
}

impl SqliteStore {
    /// Lists only this account's host-issued devices, or every remote device for the native host.
    /// # Errors
    /// Rejects stale server-owner authority and propagates storage failures.
    pub async fn owner_device_sessions(
        &self,
        owner: &ServerOwnerAuthority,
    ) -> Result<Vec<OwnerDeviceSession>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let person = scope(&mut tx, owner).await?;
        let current = match owner {
            ServerOwnerAuthority::CentralSession(session) => Some(session.session_id()),
            _ => None,
        };
        let roots = sqlx::query("SELECT * FROM host_owner_sessions WHERE revoked = 0 AND (? IS NULL OR person_id = ?) AND (connected = 1 OR EXISTS (SELECT 1 FROM operator_pairings WHERE host_owner_session_fingerprint = host_owner_sessions.fingerprint AND central_owner = 0 AND revoked = 0 AND COALESCE(session_expires_at, expires_at) > ?)) ORDER BY last_connected_at DESC, session_id")
            .bind(person).bind(person).bind(Utc::now().timestamp_micros()).fetch_all(&mut *tx).await?;
        let mut sessions = Vec::new();
        for row in roots {
            let id =
                Uuid::parse_str(&row.try_get::<String, _>("session_id")?).map_err(|_| invalid())?;
            sessions.push(OwnerDeviceSession {
                session_id: id,
                device_name: row.try_get("device_name")?,
                browser: row.try_get("browser")?,
                os: row.try_get("os")?,
                last_connected_at: Some(row.try_get("last_connected_at")?),
                current: current == Some(id),
                connected: Some(row.try_get("connected")?),
                pairing: false,
            });
        }
        let pairings = sqlx::query("SELECT * FROM operator_pairings WHERE central_owner = 0 AND revoked = 0 AND session_fingerprint IS NOT NULL AND (session_expires_at > ? OR (session_expires_at = 0 AND host_owner_session_fingerprint IS NULL AND last_connected_at > ?)) AND (? IS NULL OR EXISTS (SELECT 1 FROM host_owner_sessions WHERE fingerprint = operator_pairings.host_owner_session_fingerprint AND person_id = ?)) ORDER BY last_connected_at DESC, pairing_id")
            .bind(Utc::now().timestamp_micros()).bind((Utc::now() - crate::operator_pairing::NATIVE_IDLE_TTL).timestamp()).bind(person).bind(person).fetch_all(&mut *tx).await?;
        for row in pairings {
            sessions.push(OwnerDeviceSession {
                session_id: Uuid::parse_str(&row.try_get::<String, _>("pairing_id")?)
                    .map_err(|_| invalid())?,
                device_name: row.try_get("device_name")?,
                browser: row.try_get("browser")?,
                os: row.try_get("os")?,
                last_connected_at: row.try_get("last_connected_at")?,
                current: false,
                connected: None,
                pairing: true,
            });
        }
        tx.commit().await?;
        Ok(sessions)
    }

    /// Commits single or whole-account revocation before returning transport notifications.
    /// # Errors
    /// Rejects stale/foreign authority or an unknown device, and propagates failed writes.
    pub async fn revoke_owner_devices(
        &self,
        owner: &ServerOwnerAuthority,
        target: Option<Uuid>,
    ) -> Result<OwnerDevicesRevocation, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let person = scope(&mut tx, owner).await?;
        let target = target.map(|id| id.to_string());
        let roots = sqlx::query("SELECT fingerprint, revoked FROM host_owner_sessions WHERE (? IS NULL OR person_id = ?) AND (? IS NULL OR session_id = ?)")
            .bind(person).bind(person).bind(&target).bind(&target).fetch_all(&mut *tx).await?;
        let mut commit = OwnerDevicesRevocation {
            revoked_count: 0,
            owner_fingerprints: Vec::new(),
            room_sessions: Vec::new(),
        };
        for root in roots {
            let fingerprint = fingerprint_column(&root, "fingerprint")?;
            commit.revoked_count += usize::from(!root.try_get::<bool, _>("revoked")?);
            sqlx::query(
                "UPDATE host_owner_sessions SET revoked = 1, connected = 0 WHERE fingerprint = ?",
            )
            .bind(fingerprint.as_slice())
            .execute(&mut *tx)
            .await?;
            let pairings = sqlx::query("SELECT pairing_id, room_id, session_fingerprint, central_owner FROM operator_pairings WHERE host_owner_session_fingerprint = ? AND revoked = 0")
                .bind(fingerprint.as_slice()).fetch_all(&mut *tx).await?;
            for pairing in pairings {
                commit.revoked_count += usize::from(!pairing.try_get::<bool, _>("central_owner")?);
                revoke_pairing(&mut tx, &pairing, &mut commit.room_sessions).await?;
            }
            commit.owner_fingerprints.push(fingerprint);
        }
        let pairings = sqlx::query("SELECT pairing_id, room_id, session_fingerprint, revoked FROM operator_pairings WHERE central_owner = 0 AND (? IS NULL OR pairing_id = ?) AND (? IS NULL OR EXISTS (SELECT 1 FROM host_owner_sessions WHERE fingerprint = operator_pairings.host_owner_session_fingerprint AND person_id = ?))")
            .bind(&target).bind(&target).bind(person).bind(person).fetch_all(&mut *tx).await?;
        if target.is_some() && commit.owner_fingerprints.is_empty() && pairings.is_empty() {
            return Err(PersistenceError::CommandRejected {
                code: "owner_device_not_found".into(),
                message: "This device session is unavailable.".into(),
            });
        }
        for pairing in pairings {
            commit.revoked_count += usize::from(!pairing.try_get::<bool, _>("revoked")?);
            revoke_pairing(&mut tx, &pairing, &mut commit.room_sessions).await?;
        }
        tx.commit().await?;
        Ok(commit)
    }

    /// Records successful authorized traffic and optional connection metadata.
    /// # Errors
    /// Rejects stale pairing authority and propagates metadata persistence failure.
    pub async fn record_operator_connection(
        &self,
        expected: &crate::OperatorSessionAuthorization,
        description: Option<&crate::OwnerDeviceDescription>,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now();
        crate::operator_pairing::revalidate_operator_session(&mut tx, expected, now).await?;
        let cutoff = now.timestamp() - crate::operator_pairing::ACTIVITY_WRITE_INTERVAL_SECONDS;
        sqlx::query("UPDATE operator_pairings SET device_name = COALESCE(?, device_name), browser = COALESCE(?, browser), os = COALESCE(?, os), last_connected_at = CASE WHEN last_connected_at IS NULL OR last_connected_at < ? THEN ? ELSE last_connected_at END WHERE session_fingerprint = ? AND ((? IS NOT NULL AND device_name IS NOT ?) OR (? IS NOT NULL AND browser IS NOT ?) OR (? IS NOT NULL AND os IS NOT ?) OR last_connected_at IS NULL OR last_connected_at < ?)")
            .bind(description.map(|value| value.device_name.as_str())).bind(description.map(|value| value.browser.as_str())).bind(description.map(|value| value.os.as_str())).bind(cutoff).bind(now.timestamp())
            .bind(expected.session_fingerprint().as_slice()).bind(description.map(|value| value.device_name.as_str())).bind(description.map(|value| value.device_name.as_str()))
            .bind(description.map(|value| value.browser.as_str())).bind(description.map(|value| value.browser.as_str()))
            .bind(description.map(|value| value.os.as_str())).bind(description.map(|value| value.os.as_str())).bind(cutoff).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn scope<'a>(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &'a ServerOwnerAuthority,
) -> Result<Option<&'a str>, PersistenceError> {
    owner.revalidate(tx).await?;
    match owner {
        ServerOwnerAuthority::LocalOperator => Ok(None),
        ServerOwnerAuthority::CentralSession(session) => Ok(Some(&session.binding().person_id)),
        ServerOwnerAuthority::CentralOwner(_) => Err(invalid()),
    }
}

async fn revoke_pairing(
    tx: &mut Transaction<'_, Sqlite>,
    pairing: &sqlx::sqlite::SqliteRow,
    notifications: &mut Vec<(String, [u8; 32])>,
) -> Result<(), PersistenceError> {
    sqlx::query("UPDATE operator_pairings SET revoked = 1 WHERE pairing_id = ?")
        .bind(pairing.try_get::<String, _>("pairing_id")?)
        .execute(&mut **tx)
        .await?;
    if let Some(bytes) = pairing.try_get::<Option<Vec<u8>>, _>("session_fingerprint")? {
        let fingerprint: [u8; 32] = bytes.try_into().map_err(|_| invalid())?;
        let room_id: String = pairing.try_get("room_id")?;
        let attendees = sqlx::query("SELECT session_fingerprint FROM room_attendee_invites WHERE parent_fingerprint = ? AND session_fingerprint IS NOT NULL")
            .bind(fingerprint.as_slice()).fetch_all(&mut **tx).await?;
        for attendee in attendees {
            notifications.push((
                room_id.clone(),
                fingerprint_column(&attendee, "session_fingerprint")?,
            ));
        }
        notifications.push((room_id, fingerprint));
    }
    Ok(())
}
