use crate::{
    HumanAdmissionCommit, HumanAdmissionDecision, HumanAdmissionRejection as Rejection,
    PersistenceError, PreparedHumanAdmission, SqliteStore,
    human_admission_store::{
        SESSION_TTL, admission_result, append_participant_joined, capacity_reached,
        invite_scope_storage, join_participant,
    },
    human_invite_preflight::{load_invite_and_room, require_credential_binding},
    session_bearer::{SessionBearerPurpose::HumanAdmission, derive_session_bearer},
};
use agentsassemble_domain::{ParticipantStatus, RoomStatus};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

/// Identity from a successful host-authenticated, issuer-pinned central redemption.
/// Neither serializable nor debuggable; browser payloads cannot be deserialized into it.
pub struct MemberAdmission {
    pub issuer: String,
    pub person_id: String,
    pub display_name: String,
    pub registration_epoch: String,
    pub challenge_expires_at: DateTime<Utc>,
}

impl MemberAdmission {
    async fn is_current(
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
        return Ok(denied(Rejection::SessionUnavailable));
    }
    // Canonical authority inputs exclude device, request UUID and name snapshot.
    let input_hash: [u8; 32] = Sha256::digest(serde_json::to_vec(&(
        &binding,
        &invite.invite_id,
        &invite.room_id,
        invite_scope_storage(invite.invite_scope),
        invite.signed_token_fingerprint,
    ))?)
    .into();
    let previous = sqlx::query("SELECT invite_id, input_hash, session_key, result_json FROM member_admissions WHERE binding_id = ? AND room_id = ?")
        .bind(&binding).bind(&invite.room_id).fetch_optional(&mut **tx).await?;
    if let Some(row) = previous {
        return replay(store, tx, &invite, input_hash, row, now).await;
    }
    if invite.use_count >= invite.effective_use_limit() {
        return Ok(denied(Rejection::InviteUseLimitReached));
    }
    if capacity_reached(tx, &invite.room_id, participant, now).await? {
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
    sqlx::query("INSERT INTO human_room_sessions(admission_key, key_kind, member_admission_id, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, result_json, admitted_at, expires_at, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'browser', ?, ?, ?, ?, ?, 'active')")
        .bind(key.as_slice()).bind(if invite.is_reusable() {"reusable"} else {"one_use"}).bind(&admission_id)
        .bind(request.request_id().to_string()).bind(&invite.invite_id).bind(input_hash.as_slice()).bind(issued.fingerprint.as_slice())
        .bind(&invite.room_id).bind(user).bind(participant).bind(invite_scope_storage(invite.invite_scope))
        .bind(request.browser_credential_fingerprint().as_slice()).bind(&result_json).bind(now.timestamp_micros()).bind(expires.timestamp_micros())
        .execute(&mut **tx).await?;
    let events = if is_joined {
        vec![append_participant_joined(tx, &joined, now).await?]
    } else {
        vec![]
    };
    // Single SQLite writer serializes competing devices; the loser reads the row above.
    Ok(HumanAdmissionDecision::Admitted(Box::new(
        HumanAdmissionCommit {
            result,
            session_bearer: issued.bearer,
            events,
            replaced_session_fingerprints: vec![],
            deduplicated: false,
        },
    )))
}

async fn replay(
    store: &SqliteStore,
    tx: &mut Transaction<'_, Sqlite>,
    invite: &crate::HumanInvite,
    input_hash: [u8; 32],
    row: sqlx::sqlite::SqliteRow,
    now: DateTime<Utc>,
) -> Result<HumanAdmissionDecision, PersistenceError> {
    if row.try_get::<String, _>("invite_id")? != invite.invite_id
        || row.try_get::<Vec<u8>, _>("input_hash")? != input_hash
    {
        return Ok(denied(Rejection::IdempotencyConflict));
    }
    let key =
        crate::human_session_authority::fixed_session_fingerprint(row.try_get("session_key")?)?;
    let issued = derive_session_bearer(store.host_key.session_hmac_key(), &key, HumanAdmission);
    if !matches!(
        crate::human_session_authority::resolve_human_session(
            tx,
            &issued.fingerprint,
            Some(&invite.room_id),
            now
        )
        .await?,
        crate::human_session_authority::ResolvedHumanSession::Live { .. }
    ) {
        return Ok(denied(Rejection::SessionUnavailable));
    }
    Ok(HumanAdmissionDecision::Admitted(Box::new(
        HumanAdmissionCommit {
            result: serde_json::from_str(row.try_get("result_json")?)?,
            session_bearer: issued.bearer,
            events: vec![],
            replaced_session_fingerprints: vec![],
            deduplicated: true,
        },
    )))
}

pub(crate) async fn session_provenance(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
) -> Result<bool, PersistenceError> {
    let row = sqlx::query("SELECT s.member_admission_id, EXISTS(SELECT 1 FROM member_admissions m JOIN central_identity_bindings b ON b.binding_id = m.binding_id WHERE m.admission_id = s.member_admission_id AND b.user_id = s.user_id AND m.user_id = s.user_id AND m.participant_id = s.participant_id AND m.room_id = s.room_id AND m.invite_id = s.invite_id AND m.invite_scope = s.invite_scope AND m.session_key = s.admission_key AND m.input_hash = s.payload_hash) AS valid FROM human_room_sessions s WHERE s.session_fingerprint = ?")
        .bind(fingerprint.as_slice()).fetch_one(&mut **tx).await?;
    if row
        .try_get::<Option<String>, _>("member_admission_id")?
        .is_none()
    {
        return Ok(false);
    }
    if !row.try_get::<bool, _>("valid")? {
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
