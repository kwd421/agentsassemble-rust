use agentsassemble_domain::RoomStatus;
use chrono::{DateTime, Duration, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanInviteCredentialEvidence, PersistenceError, ProfileAttachmentMetadata, SqliteStore,
    human_admission::prejoin_avatar_custody_fingerprint,
    human_invite_preflight::{load_invite_and_room, require_credential_binding},
    profile_attachments::attachment_metadata,
    raster_assets::{enforce_storage_replacement, prepare_raster},
};

const PREJOIN_ATTACHMENT_TTL: Duration = Duration::hours(1);

struct PrejoinAuthority {
    room_id: String,
    custody_fingerprint: [u8; 32],
    invite_fingerprint: [u8; 32],
}

/// Opaque evidence that the invite and room were current before image decoding.
///
/// Fields are private so callers cannot construct or alter this capability. The
/// final write still revalidates the durable authority to close the decode-time race.
pub struct HumanPrejoinAvatarAuthorization {
    credential: HumanInviteCredentialEvidence,
    browser_credential_fingerprint: [u8; 32],
}

impl SqliteStore {
    /// Authorizes image decoding for one current invite-and-browser custody subject.
    ///
    /// # Errors
    ///
    /// Fails on a non-current invite or durable authority/storage errors.
    pub async fn authorize_human_prejoin_avatar(
        &self,
        credential: &HumanInviteCredentialEvidence,
        browser_credential_fingerprint: &[u8; 32],
    ) -> Result<HumanPrejoinAvatarAuthorization, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        current_prejoin_authority(
            &mut transaction,
            credential,
            browser_credential_fingerprint,
            Utc::now(),
        )
        .await?;
        transaction.commit().await?;
        Ok(HumanPrejoinAvatarAuthorization {
            credential: credential.clone(),
            browser_credential_fingerprint: *browser_credential_fingerprint,
        })
    }

    /// Stores one canonical avatar under preauthorized invite-and-browser custody.
    ///
    /// The opaque authorization proves authority was checked before image decoding.
    /// This method checks it again inside the final write transaction. A successful
    /// write atomically supersedes only the same custody subject's older pending avatar.
    ///
    /// # Errors
    ///
    /// Fails on a non-current invite, malformed image, absolute storage exhaustion, or durable
    /// authority/storage errors. Raw invite and browser credentials are never accepted.
    pub async fn store_human_prejoin_avatar(
        &self,
        authorization: &HumanPrejoinAvatarAuthorization,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<ProfileAttachmentMetadata, PersistenceError> {
        let (canonical, size) = prepare_raster(filename, content_type, content).await?;
        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        let authority = current_prejoin_authority(
            &mut transaction,
            &authorization.credential,
            &authorization.browser_credential_fingerprint,
            now,
        )
        .await?;
        delete_expired_pending(&mut transaction, now.timestamp()).await?;
        let previous = sqlx::query(
            "SELECT attachment_id, size FROM prejoin_avatar_assets WHERE custody_fingerprint = ?",
        )
        .bind(authority.custody_fingerprint.as_slice())
        .fetch_optional(&mut *transaction)
        .await?;
        let previous_id = previous
            .as_ref()
            .map(|row| row.get::<String, _>("attachment_id"));
        let previous_size = previous.as_ref().map(|row| row.get::<i64, _>("size"));
        enforce_storage_replacement(&mut transaction, previous_size, size, now.timestamp()).await?;
        if let Some(previous_id) = previous_id {
            let deleted = sqlx::query(
                "DELETE FROM prejoin_avatar_assets WHERE attachment_id = ? AND custody_fingerprint = ?",
            )
            .bind(previous_id)
            .bind(authority.custody_fingerprint.as_slice())
            .execute(&mut *transaction)
            .await?;
            if deleted.rows_affected() != 1 {
                return Err(invalid_prejoin_cardinality());
            }
        }

        let attachment_id = Uuid::new_v4().simple().to_string();
        let filename = canonical.filename;
        let public_size = usize::try_from(size).map_err(|_| invalid_stored_size())?;
        sqlx::query(
            "INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) VALUES (?, ?, ?, ?, ?, 'image/png', ?, ?, ?, ?)",
        )
        .bind(&attachment_id)
        .bind(&authority.room_id)
        .bind(authority.custody_fingerprint.as_slice())
        .bind(authority.invite_fingerprint.as_slice())
        .bind(&filename)
        .bind(canonical.content)
        .bind(size)
        .bind(now.to_rfc3339())
        .bind((now + PREJOIN_ATTACHMENT_TTL).timestamp())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(attachment_metadata(attachment_id, filename, public_size))
    }
}

async fn current_prejoin_authority(
    transaction: &mut Transaction<'_, Sqlite>,
    credential: &HumanInviteCredentialEvidence,
    browser_credential_fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) -> Result<PrejoinAuthority, PersistenceError> {
    let Some((invite, room)) = load_invite_and_room(transaction, credential).await? else {
        return Err(rejected("invite_invalid", "Invite is invalid."));
    };
    require_credential_binding(&invite, credential)?;
    if invite.revoked {
        return Err(rejected("invite_revoked", "Invite was revoked."));
    }
    if invite.expires_at <= now {
        return Err(rejected("token_expired", "Invite has expired."));
    }
    if invite.use_count >= invite.effective_use_limit() {
        return Err(rejected(
            "invite_use_limit_reached",
            "Invite use limit was reached.",
        ));
    }
    if room.status != RoomStatus::Active {
        return Err(rejected(
            "room_unavailable",
            "Room was deleted or is unavailable.",
        ));
    }
    Ok(PrejoinAuthority {
        room_id: invite.room_id,
        custody_fingerprint: prejoin_avatar_custody_fingerprint(
            credential,
            browser_credential_fingerprint,
        ),
        invite_fingerprint: invite.signed_token_fingerprint,
    })
}

async fn delete_expired_pending(
    transaction: &mut Transaction<'_, Sqlite>,
    now_timestamp: i64,
) -> Result<(), PersistenceError> {
    sqlx::query("DELETE FROM prejoin_avatar_assets WHERE expires_at <= ?")
        .bind(now_timestamp)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn invalid_stored_size() -> PersistenceError {
    rejected("invalid_state", "Canonical profile avatar size is invalid.")
}

fn invalid_prejoin_cardinality() -> PersistenceError {
    rejected("invalid_state", "Pre-join avatar custody is inconsistent.")
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

    use agentsassemble_domain::LOCAL_OPERATOR_USER_ID;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use sqlx::Row;

    use crate::{HumanInviteCredentialEvidence, PersistenceError, SqliteStore};

    const SIGNED: [u8; 32] = [0x11; 32];
    const JOIN: [u8; 32] = [0x22; 32];

    #[tokio::test]
    async fn exact_custody_replaces_only_its_pending_avatar_and_revalidates_invite() {
        let store = fixture().await;
        let credential = join_credential();
        assert_rejected_code(
            store
                .authorize_human_prejoin_avatar(
                    &HumanInviteCredentialEvidence::JoinCode {
                        fingerprint: [0x99; 32],
                    },
                    &[0x31; 32],
                )
                .await,
            "invite_invalid",
        );
        let first = store_prejoin_avatar(
            &store,
            &credential,
            &[0x31; 32],
            "../avatar.webp",
            "image/webp",
            valid_image(ImageFormat::WebP),
        )
        .await
        .unwrap_or_else(|error| panic!("store first prejoin avatar: {error}"));
        let second = store_prejoin_avatar(
            &store,
            &credential,
            &[0x31; 32],
            "replacement.jpg",
            "image/jpeg",
            valid_image(ImageFormat::Jpeg),
        )
        .await
        .unwrap_or_else(|error| panic!("replace exact-custody avatar: {error}"));
        assert_ne!(first.id, second.id);
        assert_eq!(pending_count(&store).await, 1);
        assert!(!attachment_exists(&store, &first.id).await);
        let row = sqlx::query(
            "SELECT room_id, invite_fingerprint, content_type, size, length(content) AS content_length, created_at, expires_at FROM prejoin_avatar_assets WHERE attachment_id = ?",
        )
        .bind(&second.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read replacement avatar: {error}"));
        assert_eq!(row.get::<String, _>("room_id"), "general");
        assert_eq!(row.get::<Vec<u8>, _>("invite_fingerprint"), SIGNED);
        assert_eq!(row.get::<String, _>("content_type"), "image/png");
        assert_eq!(
            row.get::<i64, _>("size"),
            row.get::<i64, _>("content_length")
        );
        let created = DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .unwrap_or_else(|error| panic!("parse avatar created_at: {error}"));
        assert_eq!(row.get::<i64, _>("expires_at") - created.timestamp(), 3600);

        store_prejoin_avatar(
            &store,
            &credential,
            &[0x32; 32],
            "other.png",
            "image/png",
            valid_image(ImageFormat::Png),
        )
        .await
        .unwrap_or_else(|error| panic!("store other browser avatar: {error}"));
        assert_eq!(pending_count(&store).await, 2);

        let stale_authorization = store
            .authorize_human_prejoin_avatar(&credential, &[0x33; 32])
            .await
            .unwrap_or_else(|error| panic!("authorize avatar before revoke: {error}"));

        sqlx::query("UPDATE room_invites SET revoked = 1")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("revoke invite: {error}"));
        assert_rejected_code(
            store
                .store_human_prejoin_avatar(
                    &stale_authorization,
                    "blocked.png",
                    "image/png",
                    valid_image(ImageFormat::Png),
                )
                .await,
            "invite_revoked",
        );
        assert_eq!(pending_count(&store).await, 2);
    }

    #[tokio::test]
    async fn distinct_custodies_are_not_subject_to_a_generic_invite_quota() {
        let store = fixture().await;
        let credential = join_credential();
        let mut first_id = String::new();
        for byte in 0..9 {
            let stored = store_prejoin_avatar(
                &store,
                &credential,
                &[byte; 32],
                "avatar.png",
                "image/png",
                valid_image(ImageFormat::Png),
            )
            .await
            .unwrap_or_else(|error| panic!("store custody avatar {byte}: {error}"));
            if byte == 0 {
                first_id = stored.id;
            }
        }
        assert_eq!(pending_count(&store).await, 9);
        let replacement = store_prejoin_avatar(
            &store,
            &credential,
            &[0; 32],
            "first-replacement.png",
            "image/png",
            valid_image(ImageFormat::Png),
        )
        .await
        .unwrap_or_else(|error| panic!("replace exact custody: {error}"));
        assert_ne!(replacement.id, first_id);
        assert!(!attachment_exists(&store, &first_id).await);
        assert_eq!(pending_count(&store).await, 9);
    }

    #[tokio::test]
    async fn absolute_limit_allows_exact_replacement_but_rejects_net_growth() {
        let store = fixture().await;
        let now = Utc::now();
        sqlx::query(
            "WITH digits(value) AS (VALUES (0),(1),(2),(3),(4),(5),(6),(7),(8),(9),(10),(11),(12),(13),(14),(15)), sequence(value) AS (SELECT first.value * 256 + second.value * 16 + third.value FROM digits AS first CROSS JOIN digits AS second CROSS JOIN digits AS third) INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) SELECT printf('seed-avatar-%018d', value), 'general', CAST(printf('%032d', value) AS BLOB), ?, 'stored.png', 'image/png', X'00', 1, ?, ? FROM sequence WHERE value < ?",
        )
        .bind(SIGNED.as_slice())
        .bind(now.to_rfc3339())
        .bind((now + Duration::hours(1)).timestamp())
        .bind(crate::raster_assets::MAX_LIVE_RASTER_ASSETS - 1)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("seed absolute-limit avatars: {error}"));
        let first = store_prejoin_avatar(
            &store,
            &join_credential(),
            &[0x31; 32],
            "boundary.png",
            "image/png",
            valid_image(ImageFormat::Png),
        )
        .await
        .unwrap_or_else(|error| panic!("fill last absolute-limit slot: {error}"));
        let replacement = store_prejoin_avatar(
            &store,
            &join_credential(),
            &[0x31; 32],
            "replacement.png",
            "image/png",
            valid_image(ImageFormat::Png),
        )
        .await
        .unwrap_or_else(|error| panic!("replace at absolute limit: {error}"));
        assert_ne!(replacement.id, first.id);
        assert!(!attachment_exists(&store, &first.id).await);

        assert_rejected_code(
            store_prejoin_avatar(
                &store,
                &join_credential(),
                &[0x32; 32],
                "over-limit.png",
                "image/png",
                valid_image(ImageFormat::Png),
            )
            .await,
            "attachment_quota_reached",
        );
    }

    async fn store_prejoin_avatar(
        store: &SqliteStore,
        credential: &HumanInviteCredentialEvidence,
        browser_credential_fingerprint: &[u8; 32],
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<crate::ProfileAttachmentMetadata, PersistenceError> {
        let authorization = store
            .authorize_human_prejoin_avatar(credential, browser_credential_fingerprint)
            .await?;
        store
            .store_human_prejoin_avatar(&authorization, filename, content_type, content)
            .await
    }

    fn join_credential() -> HumanInviteCredentialEvidence {
        HumanInviteCredentialEvidence::JoinCode { fingerprint: JOIN }
    }

    async fn fixture() -> SqliteStore {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open prejoin fixture: {error}"));
        store
            .bootstrap_local_authority("9de91c15-9c53-4c19-b53a-7991bab73a58", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap prejoin fixture: {error}"));
        store
            .create_room_for_local_operator(
                "93143985-93d5-44e4-a3df-8fba152985d0",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create prejoin room: {error}"));
        let now = Utc
            .timestamp_micros(Utc::now().timestamp_micros())
            .single()
            .unwrap_or_else(|| panic!("canonical prejoin clock"));
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'guest', 'Guest', 'read_write', 0, 0, ?, 0, ?, ?)",
        )
        .bind(hex::encode(&SIGNED[..8]))
        .bind(SIGNED.as_slice())
        .bind(JOIN.as_slice())
        .bind((now + Duration::hours(2)).timestamp_micros())
        .bind(LOCAL_OPERATOR_USER_ID)
        .bind((now - Duration::minutes(1)).timestamp_micros())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert prejoin invite: {error}"));
        store
    }

    fn valid_image(format: ImageFormat) -> Vec<u8> {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(4, 4, Rgba([20, 40, 60, 255])));
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, format)
            .unwrap_or_else(|error| panic!("encode image fixture: {error}"));
        encoded.into_inner()
    }

    async fn pending_count(store: &SqliteStore) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM prejoin_avatar_assets")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count pending avatars: {error}"))
    }

    async fn attachment_exists(store: &SqliteStore, attachment_id: &str) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM prejoin_avatar_assets WHERE attachment_id = ?",
        )
        .bind(attachment_id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("check avatar row: {error}"))
            == 1
    }

    fn assert_rejected_code<T>(result: Result<T, PersistenceError>, expected: &'static str) {
        match result {
            Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
            Err(error) => panic!("expected {expected}, got {error}"),
            Ok(_) => panic!("expected {expected}, got success"),
        }
    }
}
