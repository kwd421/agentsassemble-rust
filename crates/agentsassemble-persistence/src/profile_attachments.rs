use agentsassemble_domain::{
    AuthenticatedPrincipal, InviteScope, MAX_RASTER_BYTES, avatar_attachment_id,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::profile_store::ProfileIdentity;
use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    asset_storage::enforce_storage_replacement,
    authority::authorize_session,
    human_session_authority::revalidate_human_session,
    raster_assets::{CanonicalRaster, prepare_raster, sanitize_filename, validate_stored_raster},
};

const PENDING_ATTACHMENT_TTL: Duration = Duration::minutes(15);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileAttachmentMetadata {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub is_image: bool,
    pub url: String,
    pub download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileAttachment {
    pub metadata: ProfileAttachmentMetadata,
    pub content: Vec<u8>,
}

impl SqliteStore {
    /// Stores one bounded, decoded, canonical static-raster avatar as a pending upload.
    ///
    /// # Errors
    ///
    /// Fails closed on stale authority, malformed or mismatched image bytes, quota exhaustion,
    /// decode-resource limits, or `SQLite` errors.
    pub async fn store_profile_attachment(
        &self,
        principal: &AuthenticatedPrincipal,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<ProfileAttachmentMetadata, PersistenceError> {
        let (canonical, size) = prepare_raster(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        authorize_session(&mut transaction, principal).await?;
        let metadata = store_profile_attachment_in_transaction(
            &mut transaction,
            ProfileIdentity {
                user_id: &principal.principal_id,
                participant_id: &principal.participant_id,
            },
            canonical,
            size,
        )
        .await?;
        transaction.commit().await?;
        Ok(metadata)
    }

    /// Stores a pending avatar only after revalidating the consumed human-session grant.
    ///
    /// # Errors
    ///
    /// Fails closed for read-only scope, stale session provenance, malformed image bytes,
    /// decode-resource limits, storage exhaustion, or `SQLite` errors.
    pub async fn store_human_session_profile_attachment(
        &self,
        authorization: &HumanSessionAuthorization,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<ProfileAttachmentMetadata, PersistenceError> {
        require_avatar_upload_scope(authorization)?;
        let (canonical, size) = prepare_raster(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        require_avatar_upload_scope(&current)?;
        let principal = current.principal();
        let metadata = store_profile_attachment_in_transaction(
            &mut transaction,
            ProfileIdentity {
                user_id: &principal.principal_id,
                participant_id: &principal.participant_id,
            },
            canonical,
            size,
        )
        .await?;
        transaction.commit().await?;
        Ok(metadata)
    }

    /// Stores one local-operator avatar before any room exists.
    ///
    /// # Errors
    ///
    /// Fails closed on incomplete bootstrap, malformed image bytes, quota exhaustion, or storage
    /// errors.
    pub async fn store_local_operator_profile_attachment(
        &self,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<ProfileAttachmentMetadata, PersistenceError> {
        self.require_local_bootstrap_complete().await?;
        let (canonical, size) = prepare_raster(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        let metadata = store_profile_attachment_in_transaction(
            &mut transaction,
            ProfileIdentity::local_operator(),
            canonical,
            size,
        )
        .await?;
        transaction.commit().await?;
        Ok(metadata)
    }

    /// Reads one bound or live pre-admission profile-avatar blob by opaque identifier.
    ///
    /// # Errors
    ///
    /// Ordinary pending, expired pre-admission, malformed, and unknown identifiers
    /// all fail as not found.
    pub async fn profile_attachment(
        &self,
        attachment_id: &str,
    ) -> Result<ProfileAttachment, PersistenceError> {
        if !valid_attachment_id(attachment_id) {
            return Err(attachment_missing());
        }
        let rows = sqlx::query(
            "SELECT filename, content_type, content, size, created_at FROM profile_avatar_assets WHERE attachment_id = ? AND state = 'current' UNION ALL SELECT filename, content_type, content, size, created_at FROM prejoin_avatar_assets WHERE attachment_id = ? AND expires_at > ? LIMIT 2",
        )
        .bind(attachment_id)
        .bind(attachment_id)
        .bind(Utc::now().timestamp())
        .fetch_all(&self.pool)
        .await?;
        if rows.len() != 1 {
            return Err(attachment_missing());
        }
        let row = &rows[0];
        let content_type = row.get::<String, _>("content_type");
        let content = row.get::<Vec<u8>, _>("content");
        let size = row.get::<i64, _>("size");
        validate_stored_raster(
            &content_type,
            size,
            i64::try_from(content.len()).unwrap_or(i64::MAX),
            row.get::<String, _>("created_at").as_str(),
        )?;
        Ok(ProfileAttachment {
            metadata: attachment_metadata(
                attachment_id.to_owned(),
                sanitize_filename(row.get::<String, _>("filename").as_str()),
                content.len(),
            ),
            content,
        })
    }
}

fn require_avatar_upload_scope(
    authorization: &HumanSessionAuthorization,
) -> Result<(), PersistenceError> {
    if authorization.principal().invite_scope == InviteScope::ReadWrite {
        Ok(())
    } else {
        Err(rejected(
            "session_read_only",
            "Read-only room sessions cannot upload profile avatars.",
        ))
    }
}

async fn store_profile_attachment_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: ProfileIdentity<'_>,
    canonical: CanonicalRaster,
    size: i64,
) -> Result<ProfileAttachmentMetadata, PersistenceError> {
    let now = Utc::now();
    let expires_at = (now + PENDING_ATTACHMENT_TTL).timestamp();
    ensure_profile_exists(transaction, identity).await?;
    delete_expired_pending(transaction, now.timestamp()).await?;
    let previous = sqlx::query(
        "SELECT attachment_id, size FROM profile_avatar_assets WHERE owner_user_id = ? AND state = 'pending' ORDER BY attachment_id LIMIT 2",
    )
    .bind(identity.user_id)
    .fetch_all(&mut **transaction)
    .await?;
    if previous.len() > 1 {
        return Err(invalid_pending_cardinality());
    }
    let previous_id = previous
        .first()
        .map(|row| row.get::<String, _>("attachment_id"));
    let previous_size = previous.first().map(|row| row.get::<i64, _>("size"));
    enforce_storage_replacement(transaction, previous_size, size).await?;
    if let Some(previous_id) = previous_id {
        let deleted = sqlx::query(
            "DELETE FROM profile_avatar_assets WHERE attachment_id = ? AND owner_user_id = ? AND state = 'pending'",
        )
        .bind(previous_id)
        .bind(identity.user_id)
        .execute(&mut **transaction)
        .await?;
        if deleted.rows_affected() != 1 {
            return Err(invalid_pending_cardinality());
        }
    }
    let attachment_id = Uuid::new_v4().simple().to_string();
    sqlx::query(
            "INSERT INTO profile_avatar_assets(attachment_id, owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, ?, ?, 'image/png', ?, ?, ?, 'pending', ?)",
        )
        .bind(&attachment_id)
        .bind(identity.user_id)
        .bind(&canonical.filename)
        .bind(canonical.content)
        .bind(size)
        .bind(now.to_rfc3339())
        .bind(expires_at)
        .execute(&mut **transaction)
        .await?;
    Ok(attachment_metadata(
        attachment_id,
        canonical.filename,
        usize::try_from(size).unwrap_or(MAX_RASTER_BYTES),
    ))
}

pub(crate) async fn authorize_profile_avatar(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    attachment_id: &str,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    delete_expired_pending(transaction, now.timestamp()).await?;
    let row = sqlx::query(
        "SELECT owner_user_id, state, expires_at FROM profile_avatar_assets WHERE attachment_id = ?",
    )
    .bind(attachment_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(attachment_missing)?;
    if row.get::<String, _>("owner_user_id") != user_id {
        return Err(rejected(
            "attachment_owner_mismatch",
            "Profile avatar belongs to another user.",
        ));
    }
    let state = row.get::<String, _>("state");
    let available = state == "current"
        || (state == "pending"
            && row
                .get::<Option<i64>, _>("expires_at")
                .is_some_and(|expires_at| expires_at > now.timestamp()));
    if !available {
        return Err(attachment_missing());
    }
    Ok(())
}

pub(crate) async fn replace_profile_avatar(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    previous_avatar_url: &str,
    next_avatar_url: &str,
) -> Result<(), PersistenceError> {
    let previous_id = avatar_attachment_id(previous_avatar_url);
    let next_id = avatar_attachment_id(next_avatar_url);
    if previous_id == next_id {
        return Ok(());
    }
    if let Some(previous_id) = previous_id {
        let deleted = sqlx::query(
            "DELETE FROM profile_avatar_assets WHERE attachment_id = ? AND owner_user_id = ? AND state = 'current'",
        )
        .bind(previous_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
        if deleted.rows_affected() != 1 {
            return Err(attachment_missing());
        }
    }
    if let Some(next_id) = next_id {
        let promoted = sqlx::query(
            "UPDATE profile_avatar_assets SET state = 'current', expires_at = NULL WHERE attachment_id = ? AND owner_user_id = ? AND state = 'pending'",
        )
        .bind(next_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
        if promoted.rows_affected() != 1 {
            return Err(attachment_missing());
        }
    }
    Ok(())
}

async fn ensure_profile_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: ProfileIdentity<'_>,
) -> Result<(), PersistenceError> {
    let participant_id = sqlx::query_scalar::<_, String>(
        "SELECT participant_id FROM user_profiles WHERE user_id = ?",
    )
    .bind(identity.user_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        rejected(
            "user_profile_missing",
            "Authenticated user profile was not found.",
        )
    })?;
    if participant_id != identity.participant_id {
        return Err(rejected(
            "profile_authority_mismatch",
            "Authenticated user profile does not own this participant.",
        ));
    }
    Ok(())
}

async fn delete_expired_pending(
    transaction: &mut Transaction<'_, Sqlite>,
    now_timestamp: i64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "DELETE FROM profile_avatar_assets WHERE state = 'pending' AND expires_at IS NOT NULL AND expires_at <= ?",
    )
    .bind(now_timestamp)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) fn attachment_metadata(
    id: String,
    filename: String,
    size: usize,
) -> ProfileAttachmentMetadata {
    ProfileAttachmentMetadata {
        url: format!("/api/attachments/{id}?view=1"),
        download_url: format!("/api/attachments/{id}?download=1"),
        id,
        filename,
        content_type: "image/png".to_owned(),
        size,
        is_image: true,
    }
}

fn valid_attachment_id(value: &str) -> bool {
    (8..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn attachment_missing() -> PersistenceError {
    rejected("attachment_missing", "Profile avatar was not found.")
}

fn invalid_pending_cardinality() -> PersistenceError {
    rejected(
        "invalid_state",
        "Profile avatar pending ownership is invalid.",
    )
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, UserProfilePatch,
    };
    use chrono::Utc;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};

    use crate::{PersistenceError, SqliteStore};

    #[tokio::test]
    async fn invalid_or_mismatched_avatar_bytes_are_rejected() {
        let (store, principal) = fixture().await;
        let png = valid_png();
        assert_rejected_code(
            store
                .store_profile_attachment(&principal, "mismatch.jpg", "image/jpeg", png.clone())
                .await,
            "attachment_type_mismatch",
        );
        assert_rejected_code(
            store
                .store_profile_attachment(
                    &principal,
                    "not-an-image.png",
                    "image/png",
                    b"<html>active</html>".to_vec(),
                )
                .await,
            "attachment_invalid_image",
        );
    }

    #[tokio::test]
    async fn avatar_bytes_are_canonical_and_references_swap_atomically() {
        let (store, principal) = fixture().await;
        let png = valid_png();
        let first = store
            .store_profile_attachment(&principal, "first.gif", "image/png", png.clone())
            .await
            .unwrap_or_else(|error| panic!("store first avatar: {error}"));
        assert_eq!(first.content_type, "image/png");
        assert!(
            std::path::Path::new(&first.filename)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        );
        assert_rejected_code(
            store.profile_attachment(&first.id).await,
            "attachment_missing",
        );
        store
            .update_user_profile(
                &principal,
                1,
                UserProfilePatch {
                    avatar_image_url: Some(first.url.clone()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("bind first avatar: {error}"));
        let served = store
            .profile_attachment(&first.id)
            .await
            .unwrap_or_else(|error| panic!("read bound avatar: {error}"));
        assert_eq!(served.metadata.content_type, "image/png");
        assert_eq!(&served.content[..8], b"\x89PNG\r\n\x1a\n");

        let second = store
            .store_profile_attachment(&principal, "second.png", "image/png", png)
            .await
            .unwrap_or_else(|error| panic!("store second avatar: {error}"));
        sqlx::query(
            "CREATE TRIGGER reject_avatar_projection BEFORE INSERT ON room_events WHEN json_extract(NEW.event_json, '$.type') = 'participant_updated' BEGIN SELECT RAISE(ABORT, 'injected avatar projection failure'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("install rollback trigger: {error}"));
        assert!(matches!(
            store
                .update_user_profile(
                    &principal,
                    2,
                    UserProfilePatch {
                        avatar_image_url: Some(second.url.clone()),
                        ..UserProfilePatch::default()
                    },
                )
                .await,
            Err(PersistenceError::Database(_))
        ));
        store
            .profile_attachment(&first.id)
            .await
            .unwrap_or_else(|error| panic!("old avatar survives rollback: {error}"));
        assert_rejected_code(
            store.profile_attachment(&second.id).await,
            "attachment_missing",
        );
        sqlx::query("DROP TRIGGER reject_avatar_projection")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("drop rollback trigger: {error}"));
        store
            .update_user_profile(
                &principal,
                2,
                UserProfilePatch {
                    avatar_image_url: Some(second.url.clone()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("swap avatar: {error}"));
        assert_rejected_code(
            store.profile_attachment(&first.id).await,
            "attachment_missing",
        );
        store
            .profile_attachment(&second.id)
            .await
            .unwrap_or_else(|error| panic!("read replacement avatar: {error}"));
    }

    #[tokio::test]
    async fn expired_pending_avatar_is_hidden_excluded_and_collected() {
        let (store, principal) = fixture().await;
        let expired = store
            .store_profile_attachment(&principal, "expired.png", "image/png", valid_png())
            .await
            .unwrap_or_else(|error| panic!("store expiring avatar: {error}"));
        sqlx::query("UPDATE profile_avatar_assets SET expires_at = 0 WHERE attachment_id = ?")
            .bind(&expired.id)
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("expire avatar: {error}"));
        assert_rejected_code(
            store.profile_attachment(&expired.id).await,
            "attachment_missing",
        );
        let _replacement = store
            .store_profile_attachment(&principal, "fresh.png", "image/png", valid_png())
            .await
            .unwrap_or_else(|error| panic!("store after expiry: {error}"));
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM profile_avatar_assets WHERE attachment_id = ?",
        )
        .bind(&expired.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read expired row count: {error}"));
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn expired_foreign_lifecycle_rows_remain_charged_until_owner_cleanup() {
        let (store, principal) = fixture().await;
        let now = Utc::now();
        let foreign_rows = crate::asset_storage::MAX_RETAINED_ASSETS / 2;
        sqlx::query(
            "WITH digits(value) AS (VALUES (0),(1),(2),(3),(4),(5),(6),(7),(8),(9),(10),(11),(12),(13),(14),(15)), sequence(value) AS (SELECT first.value * 256 + second.value * 16 + third.value FROM digits AS first CROSS JOIN digits AS second CROSS JOIN digits AS third) INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) SELECT printf('expired-prejoin-%018d', value), 'general', CAST(printf('%032d', value) AS BLOB), ?, 'stored.png', 'image/png', X'00', 1, ?, 0 FROM sequence WHERE value < ?",
        )
        .bind([0x44_u8; 32].as_slice())
        .bind(now.to_rfc3339())
        .bind(foreign_rows)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("seed expired prejoin avatars: {error}"));
        sqlx::query(
            "WITH digits(value) AS (VALUES (0),(1),(2),(3),(4),(5),(6),(7),(8),(9),(10),(11),(12),(13),(14),(15)), sequence(value) AS (SELECT first.value * 256 + second.value * 16 + third.value FROM digits AS first CROSS JOIN digits AS second CROSS JOIN digits AS third) INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) SELECT printf('ma_%032x', value), 'general', ?, NULL, 'stored.bin', 'application/octet-stream', X'00', 1, 0, 1, 'pending', 2 FROM sequence WHERE value < ?",
        )
        .bind(&principal.principal_id)
        .bind(foreign_rows)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("seed expired message attachments: {error}"));

        assert_rejected_code(
            store
                .store_profile_attachment(&principal, "blocked.png", "image/png", valid_png())
                .await,
            "attachment_quota_reached",
        );
        let retained = sqlx::query_scalar::<_, i64>(
            "SELECT (SELECT COUNT(*) FROM prejoin_avatar_assets WHERE expires_at = 0) + (SELECT COUNT(*) FROM room_message_attachments WHERE expires_at = 2)",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read retained prejoin avatars: {error}"));
        assert_eq!(retained, crate::asset_storage::MAX_RETAINED_ASSETS);
    }

    #[tokio::test]
    async fn pending_avatar_replacement_has_no_generic_uploader_quota() {
        let (store, principal) = fixture().await;
        let mut first_id = String::new();
        let mut last_id = String::new();
        for index in 0..65 {
            let stored = store
                .store_profile_attachment(
                    &principal,
                    &format!("avatar-{index}.png"),
                    "image/png",
                    valid_png(),
                )
                .await
                .unwrap_or_else(|error| panic!("replace pending avatar {index}: {error}"));
            if index == 0 {
                first_id.clone_from(&stored.id);
            }
            last_id = stored.id;
        }

        let rows = sqlx::query_scalar::<_, String>(
            "SELECT attachment_id FROM profile_avatar_assets WHERE owner_user_id = ? AND state = 'pending'",
        )
        .bind(&principal.principal_id)
        .fetch_all(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read pending profile avatar: {error}"));
        assert_eq!(rows, vec![last_id]);
        assert_ne!(rows[0], first_id);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM profile_avatar_assets WHERE attachment_id = ?",
            )
            .bind(first_id)
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("check first pending avatar: {error}")),
            0
        );
    }

    fn valid_png() -> Vec<u8> {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(4, 4, Rgba([20, 40, 60, 255])));
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap_or_else(|error| panic!("encode png fixture: {error}"));
        encoded.into_inner()
    }

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
        let url = format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        );
        let store = SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open attachment fixture: {error}"));
        store
            .bootstrap_local_authority("c238aa38-30d3-416a-9778-97e8e2d15a09", "SeiNel")
            .await
            .unwrap_or_else(|error| panic!("bootstrap attachment identity: {error}"));
        store
            .create_room_for_local_operator(
                "20000000-0000-4000-8000-000000000007",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create attachment room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "SeiNel".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        (store, principal)
    }

    fn assert_rejected_code<T>(result: Result<T, PersistenceError>, expected: &'static str) {
        match result {
            Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
            Err(error) => panic!("expected {expected}, got {error}"),
            Ok(_) => panic!("expected {expected}, got success"),
        }
    }
}
