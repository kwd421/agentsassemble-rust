use agentsassemble_domain::{RoomEvent, UserProfile, UserProfilePatch};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    HumanInvite, PersistenceError, PreparedHumanAdmission,
    profile_attachments::replace_profile_avatar,
    profile_store::{ProfileIdentity, decode_bound_profile, project_profile_into_rooms},
    raster_assets::validate_stored_raster,
};

pub(super) struct ResolvedIdentity {
    pub(super) user_id: String,
    pub(super) participant_id: String,
    pub(super) profile: UserProfile,
    pub(super) profile_events: Vec<RoomEvent>,
    previous_avatar_url: String,
    profile_changed: bool,
    new: bool,
}

pub(super) struct AdmissionAvatar {
    attachment_id: String,
    url: String,
}

pub(super) async fn resolve_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    admission_key: &[u8; 32],
    avatar: Option<&AdmissionAvatar>,
    now: DateTime<Utc>,
) -> Result<Option<ResolvedIdentity>, PersistenceError> {
    if invite.is_reusable()
        && let Some(row) = sqlx::query(
            "SELECT credentials.user_id, profiles.participant_id, profiles.profile_json FROM human_device_credentials AS credentials JOIN user_profiles AS profiles ON profiles.user_id = credentials.user_id WHERE credentials.credential_fingerprint = ?",
        )
        .bind(request.browser_credential_fingerprint().as_slice())
        .fetch_optional(&mut **transaction)
        .await?
        {
            let user_id = row.get::<String, _>("user_id");
            let participant_id = row.get::<String, _>("participant_id");
            let profile = decode_bound_profile(
                row.get::<String, _>("participant_id").as_str(),
                &participant_id,
                row.get::<String, _>("profile_json").as_str(),
            )?;
            let previous_avatar_url = profile.avatar_image_url.clone();
            let mut updated = profile;
            let profile_changed = updated.apply_patch(
                UserProfilePatch {
                    display_name: (!request.display_name().is_empty())
                        .then(|| request.display_name().to_owned()),
                    avatar_image_url: avatar.map(|avatar| avatar.url.clone()),
                    ..UserProfilePatch::default()
                },
                now,
            );
            return Ok(Some(ResolvedIdentity {
                user_id,
                participant_id,
                profile: updated,
                profile_events: Vec::new(),
                previous_avatar_url,
                profile_changed,
                new: false,
            }));
    }

    let (user_id, participant_id) = if invite.is_reusable() {
        (
            format!(
                "u-{}",
                hex_prefix(request.browser_credential_fingerprint(), 12)
            ),
            format!(
                "guest-{}",
                hex_prefix(request.browser_credential_fingerprint(), 8)
            ),
        )
    } else {
        (
            format!("u-admission-{}", hex_prefix(admission_key, 16)),
            invite.base_participant_id.clone(),
        )
    };
    if identity_exists(transaction, &user_id, &participant_id).await? {
        return Ok(None);
    }
    let display_name = if request.display_name().is_empty() {
        invite.display_name.as_str()
    } else {
        request.display_name()
    };
    let Some(profile) = UserProfile::for_admitted_human(
        display_name,
        avatar.map_or("", |avatar| avatar.url.as_str()),
        now,
    ) else {
        return Ok(None);
    };
    Ok(Some(ResolvedIdentity {
        user_id,
        participant_id,
        profile,
        profile_events: Vec::new(),
        previous_avatar_url: String::new(),
        profile_changed: false,
        new: true,
    }))
}

pub(super) async fn persist_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    mut identity: ResolvedIdentity,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    avatar: Option<&AdmissionAvatar>,
    now: DateTime<Utc>,
) -> Result<ResolvedIdentity, PersistenceError> {
    if identity.new {
        sqlx::query(
            "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, ?)",
        )
        .bind(&identity.user_id)
        .bind(&identity.participant_id)
        .bind(serde_json::to_string(&identity.profile)?)
        .execute(&mut **transaction)
        .await?;
        if let Some(avatar) = avatar {
            transfer_admission_avatar(transaction, avatar, &identity.user_id, request, invite, now)
                .await?;
        }
        if invite.is_reusable() {
            sqlx::query(
                "INSERT INTO human_device_credentials(credential_fingerprint, user_id, created_at) VALUES (?, ?, ?)",
            )
            .bind(request.browser_credential_fingerprint().as_slice())
            .bind(&identity.user_id)
            .bind(now.timestamp_micros())
            .execute(&mut **transaction)
            .await?;
        }
        return Ok(identity);
    }

    if let Some(avatar) = avatar {
        transfer_admission_avatar(transaction, avatar, &identity.user_id, request, invite, now)
            .await?;
    }
    if identity.profile_changed {
        sqlx::query("UPDATE user_profiles SET profile_json = ? WHERE user_id = ?")
            .bind(serde_json::to_string(&identity.profile)?)
            .bind(&identity.user_id)
            .execute(&mut **transaction)
            .await?;
        if identity.previous_avatar_url != identity.profile.avatar_image_url {
            replace_profile_avatar(
                transaction,
                &identity.user_id,
                &identity.previous_avatar_url,
                &identity.profile.avatar_image_url,
            )
            .await?;
        }
        identity.profile_events = project_profile_into_rooms(
            transaction,
            ProfileIdentity {
                user_id: &identity.user_id,
                participant_id: &identity.participant_id,
            },
            &identity.profile,
        )
        .await?;
    }
    Ok(identity)
}

pub(super) async fn resolve_admission_avatar(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    now: DateTime<Utc>,
) -> Result<Option<AdmissionAvatar>, PersistenceError> {
    let Some(attachment_id) = request.avatar_attachment_id() else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT state, admission_room_id, admission_custody_fingerprint, invite_quota_fingerprint, expires_at, content_type, size, length(content) AS content_length, created_at FROM profile_attachments WHERE attachment_id = ?",
    )
    .bind(attachment_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let custody = request.avatar_custody_fingerprint();
    let valid = row.get::<String, _>("state") == "admission_pending"
        && row.get::<Option<String>, _>("admission_room_id").as_deref()
            == Some(invite.room_id.as_str())
        && row
            .get::<Option<Vec<u8>>, _>("admission_custody_fingerprint")
            .as_deref()
            == Some(custody.as_slice())
        && row
            .get::<Option<Vec<u8>>, _>("invite_quota_fingerprint")
            .as_deref()
            == Some(invite.signed_token_fingerprint.as_slice())
        && row
            .get::<Option<i64>, _>("expires_at")
            .is_some_and(|expires_at| expires_at > now.timestamp());
    if !valid {
        return Ok(None);
    }
    validate_stored_raster(
        row.get::<String, _>("content_type").as_str(),
        row.get("size"),
        row.get("content_length"),
        row.get::<String, _>("created_at").as_str(),
    )?;
    Ok(Some(AdmissionAvatar {
        attachment_id: attachment_id.to_owned(),
        url: format!("/api/attachments/{attachment_id}?view=1"),
    }))
}

async fn identity_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    participant_id: &str,
) -> Result<bool, PersistenceError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM user_profiles WHERE user_id = ? OR participant_id = ?) OR EXISTS(SELECT 1 FROM participants WHERE participant_id = ?)",
    )
    .bind(user_id)
    .bind(participant_id)
    .bind(participant_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(exists)
}

async fn transfer_admission_avatar(
    transaction: &mut Transaction<'_, Sqlite>,
    avatar: &AdmissionAvatar,
    user_id: &str,
    request: &PreparedHumanAdmission,
    invite: &HumanInvite,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let custody = request.avatar_custody_fingerprint();
    let updated = sqlx::query(
        "UPDATE profile_attachments SET owner_user_id = ?, admission_custody_fingerprint = NULL, state = 'bound', expires_at = NULL WHERE attachment_id = ? AND state = 'admission_pending' AND admission_room_id = ? AND admission_custody_fingerprint = ? AND invite_quota_fingerprint = ? AND expires_at > ?",
    )
    .bind(user_id)
    .bind(&avatar.attachment_id)
    .bind(&invite.room_id)
    .bind(custody.as_slice())
    .bind(invite.signed_token_fingerprint.as_slice())
    .bind(now.timestamp())
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(PersistenceError::CommandRejected {
            code: "invalid_state",
            message: "Admission avatar custody changed inside one transaction.".to_owned(),
        });
    }
    Ok(())
}

fn hex_prefix(value: &[u8; 32], length: usize) -> String {
    hex::encode(value).chars().take(length).collect()
}
