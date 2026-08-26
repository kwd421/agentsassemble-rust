use agentsassemble_domain::{AuthenticatedPrincipal, avatar_attachment_id};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::profile_store::ProfileIdentity;
use crate::{
    PersistenceError, SqliteStore,
    authority::authorize_session,
    raster_assets::{
        CanonicalRaster, MAX_RASTER_BYTES, enforce_storage_replacement, prepare_raster,
        sanitize_filename, validate_stored_raster,
    },
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
        let row = sqlx::query(
            "SELECT filename, content_type, content, size, created_at FROM profile_attachments WHERE attachment_id = ? AND (state = 'bound' OR (state = 'admission_pending' AND expires_at > ?))",
        )
        .bind(attachment_id)
        .bind(Utc::now().timestamp())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(attachment_missing)?;
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
        "SELECT attachment_id, size FROM profile_attachments WHERE owner_user_id = ? AND state = 'pending' ORDER BY attachment_id LIMIT 2",
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
    enforce_storage_replacement(transaction, previous_size, size, now.timestamp()).await?;
    if let Some(previous_id) = previous_id {
        let deleted = sqlx::query(
            "DELETE FROM profile_attachments WHERE attachment_id = ? AND owner_user_id = ? AND state = 'pending'",
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
            "INSERT INTO profile_attachments(attachment_id, owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, ?, ?, 'image/png', ?, ?, ?, 'pending', ?)",
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
        "SELECT owner_user_id, state, expires_at FROM profile_attachments WHERE attachment_id = ?",
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
    let available = state == "bound"
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
    if let Some(next_id) = next_id {
        let promoted = sqlx::query(
            "UPDATE profile_attachments SET state = 'bound', expires_at = NULL WHERE attachment_id = ? AND owner_user_id = ? AND state IN ('pending', 'bound')",
        )
        .bind(next_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
        if promoted.rows_affected() != 1 {
            return Err(attachment_missing());
        }
    }
    if let Some(previous_id) = previous_id {
        sqlx::query(
            "DELETE FROM profile_attachments WHERE attachment_id = ? AND owner_user_id = ? AND state = 'bound'",
        )
        .bind(previous_id)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
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
        "DELETE FROM profile_attachments WHERE state = 'pending' AND expires_at IS NOT NULL AND expires_at <= ?",
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
        sqlx::query("UPDATE profile_attachments SET expires_at = 0 WHERE attachment_id = ?")
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
            "SELECT COUNT(*) FROM profile_attachments WHERE attachment_id = ?",
        )
        .bind(&expired.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read expired row count: {error}"));
        assert_eq!(count, 0);
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
