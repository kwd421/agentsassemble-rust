use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, InviteScope, Participant, ParticipantRole, ParticipantStatus, RoomEvent, RoomSettings,
    RoomStatus, UserProfile,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanAdmissionCommit, HumanAdmissionDecision, HumanAdmissionRejection, HumanAdmissionResult,
    HumanInvite, PersistenceError, PreparedHumanAdmission, SqliteStore,
    human_admission_identity::{persist_identity, resolve_admission_avatar, resolve_identity},
    human_invite_preflight::{load_invite_and_room, require_credential_binding},
    profile_store::decode_bound_profile,
};

const SESSION_BEARER_CONTEXT: &[u8] = b"agentsassemble-human-session-bearer-v1\0";
const SESSION_BEARER_PREFIX: &str = "aas1.";
const SESSION_TTL: Duration = Duration::hours(1);
const MAX_PUBLIC_SESSIONS: i64 = 448;
const MAX_PUBLIC_ROOM_SESSIONS: i64 = 112;

impl SqliteStore {
    /// Resolves only the room queue that must own an admission attempt.
    ///
    /// This is not an authorization decision. Signed evidence already carries its
    /// authenticated room claim; join-code evidence needs one indexed lookup. The
    /// admission transaction re-resolves and validates the complete invite authority.
    ///
    /// # Errors
    ///
    /// Fails only when the join-code routing lookup cannot read persistence.
    pub async fn human_admission_room_id(
        &self,
        request: &PreparedHumanAdmission,
    ) -> Result<Option<String>, PersistenceError> {
        match request.credential() {
            crate::HumanInviteCredentialEvidence::Signed { room_id, .. } => {
                Ok(Some(room_id.clone()))
            }
            crate::HumanInviteCredentialEvidence::JoinCode { fingerprint } => {
                Ok(sqlx::query_scalar(
                    "SELECT room_id FROM room_invites WHERE join_code_fingerprint = ?",
                )
                .bind(fingerprint.as_slice())
                .fetch_optional(&self.pool)
                .await?)
            }
        }
    }

    /// Atomically consumes one browser invite and commits its profile, membership, session,
    /// result snapshot, and canonical room events.
    ///
    /// # Errors
    ///
    /// Fails on database errors or malformed durable authority. Product rejections are returned
    /// as typed decisions and never commit partial admission state.
    pub async fn admit_human(
        &self,
        request: &PreparedHumanAdmission,
        now: DateTime<Utc>,
    ) -> Result<HumanAdmissionDecision, PersistenceError> {
        let now = timestamp(now.timestamp_micros())?;
        let mut transaction = self.pool.begin().await?;
        let decision = admit_human_in_transaction(self, &mut transaction, request, now).await?;
        transaction.commit().await?;
        Ok(decision)
    }
}

async fn admit_human_in_transaction(
    store: &SqliteStore,
    transaction: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    now: DateTime<Utc>,
) -> Result<HumanAdmissionDecision, PersistenceError> {
    let Some((invite, room)) = load_invite_and_room(transaction, request.credential()).await?
    else {
        return Ok(rejected(HumanAdmissionRejection::InviteNotFound));
    };
    require_credential_binding(&invite, request.credential())?;

    let payload_hash = request.payload_hash();
    if !invite.is_reusable() {
        let admission_key = request.one_use_admission_key();
        if let Some(decision) = exact_admission(
            store,
            transaction,
            request,
            &invite,
            admission_key,
            "one_use",
            payload_hash,
            now,
        )
        .await?
        {
            return Ok(decision);
        }
    }

    if invite.revoked {
        return Ok(rejected(HumanAdmissionRejection::InviteRevoked));
    }
    if invite.expires_at <= now {
        return Ok(rejected(HumanAdmissionRejection::InviteExpired));
    }
    if invite.use_count >= invite.effective_use_limit() {
        return Ok(rejected(HumanAdmissionRejection::InviteUseLimitReached));
    }
    if room.status != RoomStatus::Active {
        return Ok(rejected(HumanAdmissionRejection::RoomUnavailable));
    }
    if !request.meeting_id_assertion().is_empty()
        && request.meeting_id_assertion() != invite.room_id
    {
        return Ok(rejected(HumanAdmissionRejection::MeetingMismatch));
    }

    let admission_key = if invite.is_reusable() {
        let key = request.reusable_admission_key();
        if let Some(decision) = exact_admission(
            store,
            transaction,
            request,
            &invite,
            key,
            "reusable",
            payload_hash,
            now,
        )
        .await?
        {
            return Ok(decision);
        }
        key
    } else {
        request.one_use_admission_key()
    };

    commit_new_admission(
        store,
        transaction,
        request,
        &invite,
        &room,
        admission_key,
        now,
    )
    .await
}

async fn commit_new_admission(
    store: &SqliteStore,
    transaction: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    room: &agentsassemble_domain::Room,
    admission_key: [u8; 32],
    now: DateTime<Utc>,
) -> Result<HumanAdmissionDecision, PersistenceError> {
    let payload_hash = request.payload_hash();
    let avatar = resolve_admission_avatar(transaction, request, invite, now).await?;
    let identity = resolve_identity(
        transaction,
        request,
        invite,
        &admission_key,
        avatar.as_ref(),
        now,
    )
    .await?;
    let Some(identity) = identity else {
        return Ok(rejected(HumanAdmissionRejection::IdentityConflict));
    };
    if capacity_reached(transaction, &invite.room_id, &identity.participant_id, now).await? {
        return Ok(rejected(HumanAdmissionRejection::CapacityReached));
    }

    sqlx::query(
        "UPDATE human_room_sessions SET state = 'ended' WHERE room_id = ? AND participant_id = ? AND state = 'active' AND expires_at <= ?",
    )
    .bind(&invite.room_id)
    .bind(&identity.participant_id)
    .bind(now.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    let identity =
        persist_identity(transaction, identity, request, invite, avatar.as_ref(), now).await?;
    let replaced_session_fingerprints =
        replace_live_sessions(transaction, &invite.room_id, &identity.participant_id, now).await?;
    let (participant, joined) = join_participant(
        transaction,
        invite,
        &identity.participant_id,
        &identity.profile,
        now,
    )
    .await?;

    let consumed = sqlx::query(
        "UPDATE room_invites SET use_count = use_count + 1 WHERE invite_id = ? AND revoked = 0 AND expires_at > ? AND use_count < CASE WHEN max_uses = 1 THEN 1 WHEN max_uses = 0 OR max_uses > 128 THEN 128 ELSE max_uses END",
    )
    .bind(&invite.invite_id)
    .bind(now.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    if consumed.rows_affected() != 1 {
        return Err(invalid_state(
            "Invite authority changed inside one admission transaction.",
        ));
    }

    let mut events = identity.profile_events;
    if joined {
        events.push(append_participant_joined(transaction, &participant, now).await?);
    }
    let expires_at = now
        .checked_add_signed(SESSION_TTL)
        .ok_or_else(|| invalid_state("Human session expiry overflowed."))?;
    let result = admission_result(
        transaction,
        request,
        invite,
        room,
        &identity.participant_id,
        &identity.profile,
        expires_at,
    )
    .await?;
    let issued = issue_session_bearer(store, &admission_key);
    sqlx::query(
        "INSERT INTO human_room_sessions(admission_key, key_kind, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'browser', ?, ?, ?, ?, ?, ?, 'active')",
    )
    .bind(admission_key.as_slice())
    .bind(if invite.is_reusable() { "reusable" } else { "one_use" })
    .bind(request.request_id().to_string())
    .bind(&invite.invite_id)
    .bind(payload_hash.as_slice())
    .bind(issued.fingerprint.as_slice())
    .bind(&invite.room_id)
    .bind(&identity.user_id)
    .bind(&identity.participant_id)
    .bind(invite_scope_storage(invite.invite_scope))
    .bind(request.browser_credential_fingerprint().as_slice())
    .bind(invite.is_reusable().then_some(request.browser_credential_fingerprint().as_slice()))
    .bind(serde_json::to_string(&result)?)
    .bind(now.timestamp_micros())
    .bind(expires_at.timestamp_micros())
    .execute(&mut **transaction)
    .await?;

    Ok(HumanAdmissionDecision::Admitted(Box::new(
        HumanAdmissionCommit {
            result,
            session_bearer: issued.bearer,
            events,
            replaced_session_fingerprints,
            deduplicated: false,
        },
    )))
}

#[allow(clippy::too_many_arguments)]
async fn exact_admission(
    store: &SqliteStore,
    transaction: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    admission_key: [u8; 32],
    key_kind: &str,
    payload_hash: [u8; 32],
    now: DateTime<Utc>,
) -> Result<Option<HumanAdmissionDecision>, PersistenceError> {
    let row = sqlx::query(
        "SELECT sessions.key_kind, sessions.first_request_id, sessions.invite_id, sessions.payload_hash, sessions.session_fingerprint, sessions.room_id, sessions.user_id, sessions.participant_id, sessions.client_kind, sessions.invite_scope, sessions.browser_credential_fingerprint, sessions.result_json, sessions.expires_at, sessions.state, profiles.participant_id AS profile_participant_id, profiles.profile_json, participants.participant_json, rooms.room_json FROM human_room_sessions AS sessions JOIN user_profiles AS profiles ON profiles.user_id = sessions.user_id AND profiles.participant_id = sessions.participant_id JOIN participants ON participants.room_id = sessions.room_id AND participants.participant_id = sessions.participant_id JOIN rooms ON rooms.room_id = sessions.room_id WHERE sessions.admission_key = ?",
    )
    .bind(admission_key.as_slice())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.get::<String, _>("key_kind") != key_kind
        || row.get::<String, _>("invite_id") != invite.invite_id
        || row.get::<String, _>("room_id") != invite.room_id
        || row.get::<String, _>("client_kind") != "browser"
        || row.get::<String, _>("invite_scope") != invite_scope_storage(invite.invite_scope)
        || row
            .get::<Vec<u8>, _>("browser_credential_fingerprint")
            .as_slice()
            != request.browser_credential_fingerprint()
    {
        return Err(invalid_state(
            "Stored human admission authority is cross-bound.",
        ));
    }
    if row.get::<Vec<u8>, _>("payload_hash").as_slice() != payload_hash {
        return Ok(Some(rejected(HumanAdmissionRejection::IdempotencyConflict)));
    }
    let state = row.get::<String, _>("state");
    let expires_at = timestamp(row.get::<i64, _>("expires_at"))?;
    let room: agentsassemble_domain::Room =
        serde_json::from_str(row.get::<String, _>("room_json").as_str())?;
    let participant: Participant =
        serde_json::from_str(row.get::<String, _>("participant_json").as_str())?;
    let room_id = row.get::<String, _>("room_id");
    let participant_id = row.get::<String, _>("participant_id");
    if room.room_id != room_id
        || participant.room_id != room_id
        || participant.participant_id != participant_id
        || participant.participant_type != "human"
    {
        return Err(invalid_state(
            "Stored human admission room or participant authority is invalid.",
        ));
    }
    decode_bound_profile(
        row.get::<String, _>("profile_participant_id").as_str(),
        &participant_id,
        row.get::<String, _>("profile_json").as_str(),
    )?;
    if state != "active" {
        return Ok(Some(rejected(HumanAdmissionRejection::SessionUnavailable)));
    }
    if expires_at <= now {
        sqlx::query("UPDATE human_room_sessions SET state = 'ended' WHERE admission_key = ?")
            .bind(admission_key.as_slice())
            .execute(&mut **transaction)
            .await?;
        return Ok(Some(rejected(HumanAdmissionRejection::SessionUnavailable)));
    }
    if room.status != RoomStatus::Active || participant.status != ParticipantStatus::Joined {
        return Err(invalid_state(
            "Stored active human session has unavailable room membership authority.",
        ));
    }
    let result: HumanAdmissionResult =
        serde_json::from_str(row.get::<String, _>("result_json").as_str())?;
    if result.status != "admitted"
        || result.request_id != row.get::<String, _>("first_request_id")
        || result.agent_id != participant.participant_id
        || result.meeting_id != room.room_id
        || result.invite_scope != invite_scope_public(invite.invite_scope)
        || result.expires_at != expires_at
    {
        return Err(invalid_state(
            "Stored human admission result is cross-bound.",
        ));
    }
    let issued = issue_session_bearer(store, &admission_key);
    if row.get::<Vec<u8>, _>("session_fingerprint").as_slice() != issued.fingerprint {
        return Err(invalid_state(
            "Stored human session fingerprint is invalid.",
        ));
    }
    Ok(Some(HumanAdmissionDecision::Admitted(Box::new(
        HumanAdmissionCommit {
            result,
            session_bearer: issued.bearer,
            events: Vec::new(),
            replaced_session_fingerprints: Vec::new(),
            deduplicated: true,
        },
    ))))
}

async fn capacity_reached(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
    now: DateTime<Utc>,
) -> Result<bool, PersistenceError> {
    let global = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM human_room_sessions WHERE state = 'active' AND expires_at > ? AND NOT (room_id = ? AND participant_id = ?)",
    )
    .bind(now.timestamp_micros())
    .bind(room_id)
    .bind(participant_id)
    .fetch_one(&mut **transaction)
    .await?;
    let room = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM human_room_sessions WHERE room_id = ? AND state = 'active' AND expires_at > ? AND participant_id != ?",
    )
    .bind(room_id)
    .bind(now.timestamp_micros())
    .bind(participant_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(global >= MAX_PUBLIC_SESSIONS || room >= MAX_PUBLIC_ROOM_SESSIONS)
}

async fn replace_live_sessions(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
    now: DateTime<Utc>,
) -> Result<Vec<[u8; 32]>, PersistenceError> {
    let rows = sqlx::query(
        "UPDATE human_room_sessions SET state = 'ended' WHERE room_id = ? AND participant_id = ? AND state = 'active' AND expires_at > ? RETURNING session_fingerprint",
    )
    .bind(room_id)
    .bind(participant_id)
    .bind(now.timestamp_micros())
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(|row| fixed_digest(row.get::<Vec<u8>, _>("session_fingerprint")))
        .collect()
}

async fn join_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    invite: &HumanInvite,
    participant_id: &str,
    profile: &UserProfile,
    now: DateTime<Utc>,
) -> Result<(Participant, bool), PersistenceError> {
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(&invite.room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let (participant, joined, changed) = match existing {
        Some(json) => {
            let mut participant: Participant = serde_json::from_str(&json)?;
            if participant.room_id != invite.room_id
                || participant.participant_id != participant_id
                || participant.participant_type != "human"
            {
                return Err(invalid_state(
                    "Stored human participant authority is invalid.",
                ));
            }
            let joined = participant.status != ParticipantStatus::Joined;
            let changed = joined
                || participant.display_name != profile.display_name
                || participant.avatar_image_url != profile.avatar_image_url;
            if changed {
                participant.status = ParticipantStatus::Joined;
                participant.display_name.clone_from(&profile.display_name);
                participant
                    .avatar_image_url
                    .clone_from(&profile.avatar_image_url);
                participant.updated_at = now;
            }
            (participant, joined, changed)
        }
        None => (
            Participant {
                room_id: invite.room_id.clone(),
                participant_id: participant_id.to_owned(),
                display_name: profile.display_name.clone(),
                avatar_image_url: profile.avatar_image_url.clone(),
                participant_type: "human".to_owned(),
                status: ParticipantStatus::Joined,
                role: ParticipantRole::Human,
                owner_id: invite.created_by_user_id.clone(),
                muted: false,
                created_at: now,
                updated_at: now,
            },
            true,
            true,
        ),
    };
    if changed {
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?) ON CONFLICT(room_id, participant_id) DO UPDATE SET participant_json = excluded.participant_json",
        )
        .bind(&invite.room_id)
        .bind(participant_id)
        .bind(serde_json::to_string(&participant)?)
        .execute(&mut **transaction)
        .await?;
    }
    Ok((participant, joined))
}

async fn append_participant_joined(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
    now: DateTime<Utc>,
) -> Result<RoomEvent, PersistenceError> {
    let seq = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
    )
    .bind(&participant.room_id)
    .fetch_one(&mut **transaction)
    .await?;
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq,
        created_at: now,
        room_id: participant.room_id.clone(),
        event_type: "participant_joined".to_owned(),
        actor: Actor {
            participant_id: participant.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some("human".to_owned()),
        actor_id: Some(participant.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::from([("participant".to_owned(), json!(participant))]),
    };
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
        .bind(&event.room_id)
        .bind(event.seq)
        .bind(serde_json::to_string(&event)?)
        .execute(&mut **transaction)
        .await?;
    Ok(event)
}

async fn admission_result(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    room: &agentsassemble_domain::Room,
    participant_id: &str,
    profile: &UserProfile,
    expires_at: DateTime<Utc>,
) -> Result<HumanAdmissionResult, PersistenceError> {
    let settings_json =
        sqlx::query_scalar::<_, String>("SELECT settings_json FROM rooms WHERE room_id = ?")
            .bind(&invite.room_id)
            .fetch_one(&mut **transaction)
            .await?;
    let settings: RoomSettings = serde_json::from_str(&settings_json)?;
    Ok(HumanAdmissionResult {
        status: "admitted".to_owned(),
        request_id: request.request_id().to_string(),
        agent_id: participant_id.to_owned(),
        display_name: profile.display_name.clone(),
        avatar_image_url: profile.avatar_image_url.clone(),
        meeting_id: invite.room_id.clone(),
        invite_scope: invite_scope_public(invite.invite_scope).to_owned(),
        participant_type: "human".to_owned(),
        client_type: "browser".to_owned(),
        provider_kind: "manual".to_owned(),
        owner_display_name: request.owner_display_name().to_owned(),
        owner_id: invite.created_by_user_id.clone(),
        stable_identity: invite.is_reusable(),
        operator: false,
        connection_kind: "native_remote_room_client".to_owned(),
        client_id: request.client_id().to_owned(),
        expires_at,
        room_label: settings.label,
        room_topic: settings.topic,
        room_created_at: room.created_at,
        guide: human_usage_guide(
            &invite.room_id,
            participant_id,
            &profile.display_name,
            invite.is_reusable(),
        ),
    })
}

fn human_usage_guide(
    room_id: &str,
    participant_id: &str,
    display_name: &str,
    reusable: bool,
) -> Value {
    json!({
        "welcome": format!(
            "You joined room '{room_id}' as '{display_name}' ({participant_id}). Your identity is enforced by the room session."
        ),
        "how_to": [
            "Use the room browser to read and send messages.",
            "The browser uses the canonical room WebSocket for live updates.",
            "Use the room leave action when you are finished."
        ],
        "etiquette": [],
        "session": {
            "expires_in_seconds": 3600,
            "rejoin": if reusable {
                "This invite link is reusable; if your session expires, join again with the same link."
            } else {
                "This invite was single-use; ask the host for a new link if your session expires."
            }
        }
    })
}

struct IssuedBearer {
    bearer: String,
    fingerprint: [u8; 32],
}

fn issue_session_bearer(store: &SqliteStore, admission_key: &[u8; 32]) -> IssuedBearer {
    derive_session_bearer(store.host_key.session_hmac_key(), admission_key)
}

fn derive_session_bearer(key: &[u8; 32], admission_key: &[u8; 32]) -> IssuedBearer {
    let mut signer = Hmac::<Sha256>::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts a 32-byte key"));
    signer.update(SESSION_BEARER_CONTEXT);
    signer.update(admission_key);
    let mac: [u8; 32] = signer.finalize().into_bytes().into();
    let mut bearer = String::with_capacity(48);
    bearer.push_str(SESSION_BEARER_PREFIX);
    URL_SAFE_NO_PAD.encode_string(mac, &mut bearer);
    let fingerprint = Sha256::digest(bearer.as_bytes()).into();
    IssuedBearer {
        bearer,
        fingerprint,
    }
}

fn fixed_digest(value: Vec<u8>) -> Result<[u8; 32], PersistenceError> {
    value
        .try_into()
        .map_err(|_| invalid_state("Stored session fingerprint has an invalid length."))
}

fn timestamp(value: i64) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::from_timestamp_micros(value)
        .ok_or_else(|| invalid_state("Stored human admission timestamp is invalid."))
}

const fn invite_scope_storage(scope: InviteScope) -> &'static str {
    match scope {
        InviteScope::ReadWrite => "read_write",
        InviteScope::ReadOnly => "read_only",
    }
}

const fn invite_scope_public(scope: InviteScope) -> &'static str {
    match scope {
        InviteScope::ReadWrite => "room",
        InviteScope::ReadOnly => "read_only",
    }
}

const fn rejected(rejection: HumanAdmissionRejection) -> HumanAdmissionDecision {
    HumanAdmissionDecision::Rejected(rejection)
}

fn invalid_state(message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "human_admission_store_tests.rs"]
mod tests;
