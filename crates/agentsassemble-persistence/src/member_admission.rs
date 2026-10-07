use crate::{
    HumanAdmissionCommit, HumanAdmissionDecision, HumanAdmissionRejection as Rejection,
    PersistenceError, PreparedHumanAdmission, SqliteStore,
    human_admission_store::{
        SESSION_TTL, admission_result, append_participant_joined, capacity_reached,
        invite_scope_storage, join_participant,
    },
    human_invite_preflight::{load_invite_and_room, require_credential_binding},
    member_projection::anchor,
    session_bearer::{SessionBearerPurpose::HumanAdmission, derive_session_bearer},
};
use agentsassemble_domain::{ParticipantStatus, RoomStatus};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

/// Identity from a successful host-authenticated, issuer-pinned central redemption.
/// Neither serializable nor debuggable; browser payloads cannot be deserialized into it.
#[derive(Clone)]
pub struct MemberAdmission {
    pub secure: Option<crate::SecureSessionBinding>,
    pub projection_id: String,
    pub issuer: String,
    pub person_id: String,
    pub display_name: String,
    pub registration_epoch: String,
    pub challenge_fingerprint: [u8; 32],
    pub challenge_expires_at: DateTime<Utc>,
}

impl MemberAdmission {
    pub(crate) async fn is_current(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        now: DateTime<Utc>,
    ) -> Result<bool, PersistenceError> {
        let epoch: Option<String> = sqlx::query_scalar(
            "SELECT value FROM runtime_metadata WHERE key = 'central_registration_epoch'",
        )
        .fetch_optional(&mut **tx)
        .await?;
        Ok(now < self.challenge_expires_at
            && epoch.as_deref() == Some(self.registration_epoch.as_str())
            && !self.issuer.is_empty()
            && !self.person_id.is_empty())
    }
}

fn denied(reason: Rejection) -> HumanAdmissionDecision {
    HumanAdmissionDecision::Rejected(reason)
}

pub(crate) async fn admit(
    store: &SqliteStore,
    tx: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    member: &MemberAdmission,
    now: DateTime<Utc>,
) -> Result<HumanAdmissionDecision, PersistenceError> {
    if !member.is_current(tx, now).await? {
        return Ok(denied(Rejection::SessionUnavailable));
    }
    let Some((invite, room)) = load_invite_and_room(tx, request.credential()).await? else {
        return Ok(denied(Rejection::InviteNotFound));
    };
    require_credential_binding(&invite, request.credential())?;
    if invite.revoked {
        return Ok(denied(Rejection::InviteRevoked));
    }
    if invite.expires_at <= now {
        return Ok(denied(Rejection::InviteExpired));
    }
    if room.status != RoomStatus::Active {
        return Ok(denied(Rejection::RoomUnavailable));
    }
    if !request.meeting_id_assertion().is_empty()
        && request.meeting_id_assertion() != invite.room_id
    {
        return Ok(denied(Rejection::MeetingMismatch));
    }
    let (binding, identity) =
        crate::human_admission_identity::resolve_member_identity(tx, member, now).await?;
    let participant = &identity.participant_id;
    if let Some(current) =
        crate::participant_rows::load_participant_by_key(tx, &invite.room_id, participant).await?
        && current.status != ParticipantStatus::Joined
    {
        return Ok(denied(match current.status {
            ParticipantStatus::Left | ParticipantStatus::Kicked => Rejection::MemberMembershipEnded,
            _ => Rejection::SessionUnavailable,
        }));
    }
    let input_hash = canonical_input_hash(&binding, &invite)?;
    let previous =
        sqlx::query("SELECT admission_id, participant_id, room_id, invite_id, invite_scope, input_hash, session_key, result_json, replay_floor FROM member_admissions WHERE binding_id = ? AND room_id = ?")
            .bind(&binding)
            .bind(&invite.room_id)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(row) = previous {
        anchor(tx, &binding, member, false).await?;
        return reenter(store, tx, request, &invite, input_hash, row, now).await;
    }
    if invite.use_count >= invite.effective_use_limit() {
        return Ok(denied(Rejection::InviteUseLimitReached));
    }
    if capacity_reached(tx, &invite.room_id, participant, 0, now).await? {
        return Ok(denied(Rejection::CapacityReached));
    }
    let is_new = identity.new;
    let identity = crate::human_admission_identity::persist_identity(
        tx, identity, request, &invite, None, now,
    )
    .await?;
    let user = &identity.user_id;
    let participant = &identity.participant_id;
    let profile = &identity.profile;
    if is_new {
        sqlx::query("INSERT INTO central_identity_bindings(binding_id, issuer, person_id, user_id, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&binding).bind(&member.issuer).bind(&member.person_id).bind(user).bind(now.timestamp_micros()).execute(&mut **tx).await?;
    }
    let (joined, is_joined) = join_participant(tx, &invite, participant, profile, now).await?;
    let expires = now + SESSION_TTL;
    let mut result =
        admission_result(tx, request, &invite, &room, participant, profile, expires).await?;
    result.stable_identity = true;
    let result_json = serde_json::to_string(&result)?;
    let admission_id = Uuid::new_v4().to_string();
    let key: [u8; 32] = Sha256::digest(admission_id.as_bytes()).into();
    let issued = derive_session_bearer(store.host_key.session_hmac_key(), &key, HumanAdmission);
    sqlx::query("INSERT INTO member_admissions(binding_id, invite_id, admission_id, room_id, invite_scope, input_hash, user_id, participant_id, session_key, result_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&binding).bind(&invite.invite_id).bind(&admission_id).bind(&invite.room_id).bind(invite_scope_storage(invite.invite_scope))
        .bind(input_hash.as_slice()).bind(user).bind(participant).bind(key.as_slice()).bind(&result_json).execute(&mut **tx).await?;
    sqlx::query("UPDATE room_invites SET use_count = use_count + 1 WHERE invite_id = ?")
        .bind(&invite.invite_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO human_room_sessions(admission_key, key_kind, member_admission_id, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, result_json, admitted_at, expires_at, member_challenge_expires_at, member_last_used_at, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'browser', ?, ?, ?, ?, ?, ?, ?, 'active')")
        .bind(key.as_slice()).bind(if invite.is_reusable() {"reusable"} else {"one_use"}).bind(&admission_id)
        .bind(request.request_id().to_string()).bind(&invite.invite_id).bind(input_hash.as_slice()).bind(issued.fingerprint.as_slice())
        .bind(&invite.room_id).bind(user).bind(participant).bind(invite_scope_storage(invite.invite_scope))
        .bind(request.browser_credential_fingerprint().as_slice()).bind(&result_json).bind(now.timestamp_micros()).bind(expires.timestamp_micros()).bind(member.challenge_expires_at.timestamp_micros()).bind(now.timestamp_micros())
        .execute(&mut **tx).await?;
    crate::secure_session::bind_member(tx, &issued.fingerprint, member.secure.as_ref()).await?;
    anchor(tx, &binding, member, true).await?;
    // Single SQLite writer serializes competing devices; the loser reads the row above.
    Ok(HumanAdmissionDecision::Admitted(Box::new(
        HumanAdmissionCommit {
            result,
            session_bearer: issued.bearer,
            events: if is_joined {
                vec![append_participant_joined(tx, &joined, now).await?]
            } else {
                vec![]
            },
            replaced_session_fingerprints: vec![],
            deduplicated: false,
        },
    )))
}

// Device, request UUID and name snapshot do not change the admission authority.
fn canonical_input_hash(
    binding: &str,
    invite: &crate::HumanInvite,
) -> Result<[u8; 32], PersistenceError> {
    Ok(Sha256::digest(serde_json::to_vec(&(
        binding,
        &invite.invite_id,
        &invite.room_id,
        invite_scope_storage(invite.invite_scope),
        invite.signed_token_fingerprint,
    ))?)
    .into())
}

fn reentry_key(
    seed: &[u8],
    browser: &[u8],
    request_id: &str,
    challenge: Option<&[u8]>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"agentsassemble-member-session-v1\0");
    hash.update(seed);
    hash.update(browser);
    hash.update(request_id.as_bytes());
    if let Some(challenge) = challenge {
        hash.update(challenge);
    }
    hash.finalize().into()
}

async fn reenter(
    store: &SqliteStore,
    tx: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    invite: &crate::HumanInvite,
    input_hash: [u8; 32],
    row: sqlx::sqlite::SqliteRow,
    now: DateTime<Utc>,
) -> Result<HumanAdmissionDecision, PersistenceError> {
    let member = request.member.as_ref().ok_or_else(|| {
        crate::account_identity::rejected("session_revoked", "Member admission is required.")
    })?;
    if row.try_get::<String, _>("invite_scope")? != invite_scope_storage(invite.invite_scope)
        || (row.try_get::<String, _>("invite_id")? == invite.invite_id
            && row.try_get::<Vec<u8>, _>("input_hash")? != input_hash)
    {
        return Ok(denied(Rejection::IdempotencyConflict));
    }
    mint_session(
        store,
        tx,
        member,
        (
            request.browser_credential_fingerprint(),
            &request.request_id().to_string(),
            request.client_id(),
        ),
        row,
        now,
    )
    .await
}

pub(crate) async fn mint_session(
    store: &SqliteStore,
    tx: &mut Transaction<'_, Sqlite>,
    member: &MemberAdmission,
    (browser, request_id, client_id): (&[u8; 32], &str, &str),
    row: sqlx::sqlite::SqliteRow,
    now: DateTime<Utc>,
) -> Result<HumanAdmissionDecision, PersistenceError> {
    let seed =
        crate::human_session_authority::fixed_session_fingerprint(row.try_get("session_key")?)?;
    let key = reentry_key(
        &seed,
        browser,
        request_id,
        Some(&member.challenge_fingerprint),
    );
    let exact = sqlx::query("SELECT admission_key, result_json, state, expires_at FROM human_room_sessions WHERE member_admission_id = ? AND first_request_id = ? AND browser_credential_fingerprint = ?")
        .bind(row.try_get::<&str, _>("admission_id")?)
        .bind(request_id.to_owned()).bind(browser.as_slice())
        .fetch_optional(&mut **tx).await?;
    if let Some(exact) = exact {
        if exact.try_get::<&str, _>("state")? != "active"
            || exact.try_get::<i64, _>("expires_at")? <= now.timestamp_micros()
        {
            return Ok(denied(Rejection::SessionUnavailable));
        }
        let key = crate::human_session_authority::fixed_session_fingerprint(
            exact.try_get("admission_key")?,
        )?;
        let issued = derive_session_bearer(store.host_key.session_hmac_key(), &key, HumanAdmission);
        session_provenance(tx, &issued.fingerprint).await?;
        crate::secure_session::require_member_binding(
            tx,
            &issued.fingerprint,
            member.secure.as_ref(),
        )
        .await?;
        return Ok(HumanAdmissionDecision::Admitted(Box::new(
            HumanAdmissionCommit {
                result: serde_json::from_str(exact.try_get("result_json")?)?,
                session_bearer: issued.bearer,
                events: vec![],
                replaced_session_fingerprints: vec![],
                deduplicated: true,
            },
        )));
    }
    if member.challenge_expires_at.timestamp_micros() <= row.try_get::<i64, _>("replay_floor")? {
        return Ok(denied(Rejection::SessionUnavailable));
    }
    let admission: &str = row.try_get("admission_id")?;
    let participant: &str = row.try_get("participant_id")?;
    let retained = crate::member_sessions::retained_devices(tx, admission, browser, now).await?;
    if capacity_reached(tx, row.try_get("room_id")?, participant, retained, now).await? {
        return Ok(denied(Rejection::CapacityReached));
    }
    let replaced_session_fingerprints =
        crate::member_sessions::replace_devices(tx, admission, browser, now).await?;
    let mut result: crate::HumanAdmissionResult =
        serde_json::from_str(row.try_get("result_json")?)?;
    result.request_id = request_id.to_owned();
    result.client_id = client_id.to_owned();
    result.expires_at = now + SESSION_TTL;
    let issued = derive_session_bearer(store.host_key.session_hmac_key(), &key, HumanAdmission);
    sqlx::query("INSERT INTO human_room_sessions(admission_key, key_kind, member_admission_id, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, result_json, admitted_at, expires_at, member_challenge, member_challenge_expires_at, member_last_used_at, state) SELECT ?, (SELECT key_kind FROM room_invites WHERE invite_id = member_admissions.invite_id), admission_id, ?, invite_id, input_hash, ?, room_id, user_id, participant_id, 'browser', invite_scope, ?, ?, ?, ?, ?, ?, ?, 'active' FROM member_admissions WHERE admission_id = ?")
        .bind(key.as_slice()).bind(&result.request_id).bind(issued.fingerprint.as_slice())
        .bind(browser.as_slice()).bind(serde_json::to_string(&result)?)
        .bind(now.timestamp_micros()).bind(result.expires_at.timestamp_micros())
        .bind(member.challenge_fingerprint.as_slice()).bind(member.challenge_expires_at.timestamp_micros()).bind(now.timestamp_micros())
        .bind(admission).execute(&mut **tx).await?;
    crate::secure_session::bind_member(tx, &issued.fingerprint, member.secure.as_ref()).await?;
    crate::member_sessions::prune(tx, admission).await?;
    // A Joined membership can be absent from live snapshots after all sessions expire.
    // Publish its canonical row again when a new session makes it visible.
    let participant =
        crate::participant_rows::load_participant_by_key(tx, &result.meeting_id, participant)
            .await?
            .ok_or(PersistenceError::RoomMissing)?;
    let joined = append_participant_joined(tx, &participant, now).await?;
    Ok(HumanAdmissionDecision::Admitted(Box::new(
        HumanAdmissionCommit {
            result,
            session_bearer: issued.bearer,
            events: vec![joined],
            replaced_session_fingerprints,
            deduplicated: false,
        },
    )))
}

pub(crate) async fn session_provenance(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
) -> Result<bool, PersistenceError> {
    let row = sqlx::query("SELECT s.member_admission_id, s.member_challenge, s.admission_key, s.first_request_id, s.browser_credential_fingerprint, (SELECT session_key FROM member_admissions WHERE admission_id = s.member_admission_id) AS seed, EXISTS(SELECT 1 FROM member_admissions m JOIN central_identity_bindings b ON b.binding_id = m.binding_id WHERE m.admission_id = s.member_admission_id AND b.user_id = s.user_id AND m.user_id = s.user_id AND m.participant_id = s.participant_id AND m.room_id = s.room_id AND m.invite_id = s.invite_id AND m.invite_scope = s.invite_scope AND m.input_hash = s.payload_hash) AS valid FROM human_room_sessions s WHERE s.session_fingerprint = ?")
        .bind(fingerprint.as_slice()).fetch_one(&mut **tx).await?;
    if row
        .try_get::<Option<String>, _>("member_admission_id")?
        .is_none()
    {
        return Ok(false);
    }
    let seed: Vec<u8> = row.try_get("seed")?;
    let key: Vec<u8> = row.try_get("admission_key")?;
    let derived = reentry_key(
        &seed,
        &row.try_get::<Vec<u8>, _>("browser_credential_fingerprint")?,
        row.try_get("first_request_id")?,
        row.try_get::<Option<&[u8]>, _>("member_challenge")?,
    );
    if !row.try_get::<bool, _>("valid")? || (key != seed && key != derived) {
        return Err(crate::account_identity::rejected(
            "session_revoked",
            "Member admission provenance is invalid.",
        ));
    }
    Ok(true)
}

#[cfg(test)]
#[path = "member_admission_tests.rs"]
mod tests;
