use agentsassemble_domain::{Participant, ParticipantStatus, Room, RoomSettings, RoomStatus};
use chrono::{DateTime, Utc};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    AccountAuthority, AccountIdentity, PersistenceError, SqliteStore,
    account_identity::{
        bind_account_device, load_account_user, rejected, require_public_account_user,
        revalidate_account_identity,
    },
    human_admission_store::{SESSION_TTL, capacity_reached, replace_live_sessions},
    human_session_authority::{
        ResolvedHumanSession, fixed_session_fingerprint, resolve_human_session,
    },
    session_bearer::{IssuedBearer, SessionBearerPurpose, derive_session_bearer},
};

pub struct GuestRecoveryRequest<'a> {
    pub fingerprint: &'a [u8; 32],
    pub device: &'a [u8; 32],
    pub room_id: &'a str,
    pub client_id: &'a str,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GuestRecoveryResult {
    pub agent_id: String,
    pub display_name: String,
    pub meeting_id: String,
    pub room_uid: String,
    pub invite_scope: String,
    pub participant_type: String,
    pub client_type: String,
    pub provider_kind: String,
    pub connection_kind: String,
    pub client_id: String,
    pub joined_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub room_label: String,
    pub room_topic: String,
    pub room_created_at: DateTime<Utc>,
}

pub struct GuestRecoveryCommit {
    pub result: GuestRecoveryResult,
    pub session_bearer: String,
    pub recovery_code: String,
    pub replaced_session_fingerprints: Vec<[u8; 32]>,
}

impl SqliteStore {
    /// Rotates the sole recovery code for a currently authenticated human identity.
    ///
    /// # Errors
    /// Rejects non-human account authority, changed session/device ownership and failed storage.
    pub async fn issue_guest_recovery_code(
        &self,
        identity: &AccountIdentity,
    ) -> Result<String, PersistenceError> {
        if !matches!(identity.authority, AccountAuthority::HumanSession { .. }) {
            return Err(rejected(
                "human_identity_required",
                "A human room session is required.",
            ));
        }
        let code = recovery_code(self, &random_seed()?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let user = revalidate_account_identity(&mut tx, identity)
            .await?
            .ok_or_else(invalid_code)?;
        sqlx::query("INSERT INTO guest_recovery_codes(user_id, fingerprint) VALUES (?, ?) ON CONFLICT(user_id) DO UPDATE SET fingerprint = excluded.fingerprint, previous_fingerprint = NULL, session_key = NULL")
            .bind(&user.user_id).bind(code.fingerprint.as_slice()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(code.bearer)
    }

    /// Atomically recovers active membership onto a device and rotates its one-use code.
    /// The single previous receipt permits an exact same-device retry while its session is live.
    ///
    /// # Errors
    /// Rejects invalid/retired codes, conflicting devices, unavailable membership and storage failures.
    pub async fn redeem_guest_recovery_code(
        &self,
        request: &GuestRecoveryRequest<'_>,
        now: DateTime<Utc>,
    ) -> Result<GuestRecoveryCommit, PersistenceError> {
        let GuestRecoveryRequest {
            fingerprint,
            device,
            room_id,
            client_id,
        } = *request;
        if room_id.is_empty()
            || room_id.len() > 128
            || client_id.is_empty()
            || client_id.len() > 128
        {
            return Err(rejected(
                "recovery_request_invalid",
                "A room and browser client identity are required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT user_id, fingerprint, previous_fingerprint, session_key FROM guest_recovery_codes WHERE fingerprint = ? OR previous_fingerprint = ?")
            .bind(fingerprint.as_slice()).bind(fingerprint.as_slice()).fetch_optional(&mut *tx).await?
            .ok_or_else(invalid_code)?;
        let user_id: String = row.try_get("user_id")?;
        require_public_account_user(&user_id)?;
        if row.try_get::<Vec<u8>, _>("fingerprint")?.as_slice() != fingerprint {
            let seed = row
                .try_get::<Option<Vec<u8>>, _>("session_key")?
                .ok_or_else(invalid_code)?;
            let result = replay(
                self,
                &mut tx,
                &user_id,
                &fixed_session_fingerprint(seed)?,
                request,
                now,
            )
            .await?;
            tx.commit().await?;
            return Ok(result);
        }
        let result = recover(self, &mut tx, &user_id, device, room_id, client_id, now).await?;
        tx.commit().await?;
        Ok(result)
    }
}

async fn replay(
    store: &SqliteStore,
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    seed: &[u8; 32],
    request: &GuestRecoveryRequest<'_>,
    now: DateTime<Utc>,
) -> Result<GuestRecoveryCommit, PersistenceError> {
    let GuestRecoveryRequest {
        device,
        room_id,
        client_id,
        ..
    } = *request;
    let issued = derive_session_bearer(
        store.host_key.session_hmac_key(),
        seed,
        SessionBearerPurpose::HumanAdmission,
    );
    let ResolvedHumanSession::Live { authorization, .. } =
        resolve_human_session(tx, &issued.fingerprint, Some(room_id), now).await?
    else {
        return Err(invalid_code());
    };
    let row = sqlx::query("SELECT browser_credential_fingerprint, result_json FROM human_room_sessions WHERE admission_key = ?")
        .bind(seed.as_slice()).fetch_one(&mut **tx).await?;
    let result: GuestRecoveryResult = serde_json::from_str(row.try_get("result_json")?)?;
    if authorization.principal().principal_id != user_id
        || row
            .try_get::<Vec<u8>, _>("browser_credential_fingerprint")?
            .as_slice()
            != device
        || result.client_id != client_id
    {
        return Err(invalid_code());
    }
    Ok(GuestRecoveryCommit {
        result,
        session_bearer: issued.bearer,
        recovery_code: recovery_code(store, seed).bearer,
        replaced_session_fingerprints: Vec::new(),
    })
}

async fn recover(
    store: &SqliteStore,
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    device: &[u8; 32],
    room_id: &str,
    client_id: &str,
    now: DateTime<Utc>,
) -> Result<GuestRecoveryCommit, PersistenceError> {
    let user = load_account_user(tx, user_id).await?;
    // The latest admitted scope owns this membership. Recovery neither consumes an
    // invitation nor converts an inactive participant back into a joined member.
    let source = sqlx::query("SELECT sessions.invite_scope, rooms.room_json, rooms.settings_json, participants.participant_json FROM human_room_sessions AS sessions JOIN rooms ON rooms.room_id = sessions.room_id JOIN participants ON participants.room_id = sessions.room_id AND participants.participant_id = sessions.participant_id WHERE sessions.user_id = ? AND sessions.room_id = ? ORDER BY sessions.admitted_at DESC, sessions.rowid DESC LIMIT 1")
        .bind(user_id).bind(room_id).fetch_optional(&mut **tx).await?.ok_or_else(unavailable_membership)?;
    let room: Room = serde_json::from_str(source.try_get("room_json")?)?;
    let participant: Participant = serde_json::from_str(source.try_get("participant_json")?)?;
    let settings: RoomSettings = serde_json::from_str(source.try_get("settings_json")?)?;
    if room.room_id != room_id
        || participant.room_id != room_id
        || participant.participant_id != user.participant_id
        || participant.participant_type != "human"
    {
        return Err(rejected(
            "invalid_state",
            "Recovery membership is cross-bound.",
        ));
    }
    if room.status != RoomStatus::Active || participant.status != ParticipantStatus::Joined {
        return Err(unavailable_membership());
    }
    if capacity_reached(tx, room_id, &user.participant_id, now).await? {
        return Err(rejected(
            "recovery_capacity_reached",
            "Room session capacity is full.",
        ));
    }
    bind_account_device(tx, user_id, device).await?;
    // Retire expired rows too: the existing unique active-participant index owns
    // one session per membership, even before an expired session is observed.
    sqlx::query("UPDATE human_room_sessions SET state = 'ended' WHERE room_id = ? AND participant_id = ? AND state = 'active' AND expires_at <= ?")
        .bind(room_id).bind(&user.participant_id).bind(now.timestamp_micros()).execute(&mut **tx).await?;
    let replaced_session_fingerprints =
        replace_live_sessions(tx, room_id, &user.participant_id, now).await?;
    let seed = random_seed()?;
    let issued = derive_session_bearer(
        store.host_key.session_hmac_key(),
        &seed,
        SessionBearerPurpose::HumanAdmission,
    );
    let code = recovery_code(store, &seed);
    let scope: String = source.try_get("invite_scope")?;
    let result = GuestRecoveryResult {
        agent_id: user.participant_id.clone(),
        display_name: user.profile.display_name,
        meeting_id: room.room_id,
        room_uid: room.room_uid.to_string(),
        invite_scope: match scope.as_str() {
            "read_write" => "room",
            "read_only" => "read_only",
            _ => {
                return Err(rejected(
                    "invalid_state",
                    "Recovery membership scope is invalid.",
                ));
            }
        }
        .into(),
        participant_type: "human".into(),
        client_type: "browser".into(),
        provider_kind: "manual".into(),
        connection_kind: "native_remote_room_client".into(),
        client_id: client_id.into(),
        joined_at: now,
        expires_at: now
            .checked_add_signed(SESSION_TTL)
            .ok_or_else(|| rejected("invalid_state", "Recovery expiry overflowed."))?,
        room_label: settings.label,
        room_topic: settings.topic,
        room_created_at: room.created_at,
    };
    sqlx::query("INSERT INTO human_room_sessions(admission_key, key_kind, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) VALUES (?, 'recovery', ?, ?, ?, ?, 'browser', ?, ?, ?, ?, ?, ?, 'active')")
        .bind(seed.as_slice()).bind(issued.fingerprint.as_slice()).bind(room_id).bind(user_id).bind(&user.participant_id)
        .bind(scope).bind(device.as_slice()).bind(device.as_slice())
        .bind(serde_json::to_string(&result)?).bind(now.timestamp_micros()).bind(result.expires_at.timestamp_micros()).execute(&mut **tx).await?;
    sqlx::query("UPDATE guest_recovery_codes SET previous_fingerprint = fingerprint, fingerprint = ?, session_key = ? WHERE user_id = ?")
        .bind(code.fingerprint.as_slice()).bind(seed.as_slice()).bind(user_id).execute(&mut **tx).await?;
    Ok(GuestRecoveryCommit {
        result,
        session_bearer: issued.bearer,
        recovery_code: code.bearer,
        replaced_session_fingerprints,
    })
}

fn recovery_code(store: &SqliteStore, seed: &[u8; 32]) -> IssuedBearer {
    derive_session_bearer(
        store.host_key.session_hmac_key(),
        seed,
        SessionBearerPurpose::GuestIdentityRecovery,
    )
}

fn random_seed() -> Result<[u8; 32], PersistenceError> {
    let mut seed = [0; 32];
    SystemRandom::new().fill(&mut seed).map_err(|_| {
        rejected(
            "recovery_unavailable",
            "Recovery credential generation failed.",
        )
    })?;
    Ok(seed)
}

fn invalid_code() -> PersistenceError {
    rejected(
        "recovery_invalid",
        "The recovery code is invalid, retired or belongs to a different request.",
    )
}

fn unavailable_membership() -> PersistenceError {
    rejected(
        "recovery_membership_inactive",
        "An active human membership in this room is required.",
    )
}

#[cfg(test)]
#[path = "guest_identity_recovery_tests.rs"]
mod tests;
