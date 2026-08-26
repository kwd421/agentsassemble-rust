use agentsassemble_domain::RoomStatus;
use chrono::{DateTime, Duration, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanInviteCredentialEvidence, PersistenceError, ProfileAttachmentMetadata, SqliteStore,
    human_admission::prejoin_avatar_custody_fingerprint,
    human_invite_preflight::{load_invite_and_room, require_credential_binding},
    profile_attachments::{attachment_metadata, prepare_profile_attachment},
};

const PREJOIN_ATTACHMENT_TTL: Duration = Duration::hours(1);
const MAX_ATTACHMENTS_PER_INVITE: i64 = 8;
const MAX_ATTACHMENT_BYTES_PER_INVITE: i64 = 32 * 1024 * 1024;
const MAX_PENDING_ATTACHMENTS_PER_ROOM: i64 = 64;
const MAX_PENDING_ATTACHMENT_BYTES_PER_ROOM: i64 = 128 * 1024 * 1024;
const MAX_ATTACHMENTS_PER_ROOM: i64 = 512;
const MAX_ATTACHMENT_BYTES_PER_ROOM: i64 = 1024 * 1024 * 1024;
const MAX_ATTACHMENTS_TOTAL: i64 = 4096;
const MAX_ATTACHMENT_BYTES_TOTAL: i64 = 8 * 1024 * 1024 * 1024;

struct PrejoinAuthority {
    room_id: String,
    custody_fingerprint: [u8; 32],
    invite_quota_fingerprint: [u8; 32],
}

struct QuotaUsage {
    invite_count: i64,
    invite_bytes: i64,
    pending_room_count: i64,
    pending_room_bytes: i64,
    room_count: i64,
    room_bytes: i64,
    total_count: i64,
    total_bytes: i64,
}

impl SqliteStore {
    /// Stores one canonical avatar under exact invite-and-browser custody.
    ///
    /// Invite authority is checked before image decoding and again inside the final
    /// write transaction. The successful write atomically supersedes only the same
    /// custody subject's older pending avatar.
    ///
    /// # Errors
    ///
    /// Fails on a non-current invite, malformed image, quota exhaustion, or durable
    /// authority/storage errors. Raw invite and browser credentials are never accepted.
    pub async fn store_human_prejoin_avatar(
        &self,
        credential: &HumanInviteCredentialEvidence,
        browser_credential_fingerprint: &[u8; 32],
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<ProfileAttachmentMetadata, PersistenceError> {
        let mut precheck = self.pool.begin().await?;
        current_prejoin_authority(
            &mut precheck,
            credential,
            browser_credential_fingerprint,
            Utc::now(),
        )
        .await?;
        precheck.commit().await?;

        let (canonical, size) = prepare_profile_attachment(filename, content_type, content).await?;
        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        let authority = current_prejoin_authority(
            &mut transaction,
            credential,
            browser_credential_fingerprint,
            now,
        )
        .await?;
        delete_expired_pending(&mut transaction, now.timestamp()).await?;
        let usage = quota_usage(&mut transaction, &authority, now.timestamp()).await?;
        enforce_quota(&usage, size)?;

        sqlx::query(
            "DELETE FROM profile_attachments WHERE state = 'admission_pending' AND admission_custody_fingerprint = ?",
        )
        .bind(authority.custody_fingerprint.as_slice())
        .execute(&mut *transaction)
        .await?;

        let attachment_id = Uuid::new_v4().simple().to_string();
        let filename = canonical.filename;
        let public_size = usize::try_from(size).map_err(|_| invalid_stored_size())?;
        sqlx::query(
            "INSERT INTO profile_attachments(attachment_id, owner_user_id, admission_room_id, admission_custody_fingerprint, invite_quota_fingerprint, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, NULL, ?, ?, ?, ?, 'image/png', ?, ?, ?, 'admission_pending', ?)",
        )
        .bind(&attachment_id)
        .bind(&authority.room_id)
        .bind(authority.custody_fingerprint.as_slice())
        .bind(authority.invite_quota_fingerprint.as_slice())
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
        invite_quota_fingerprint: invite.signed_token_fingerprint,
    })
}

async fn delete_expired_pending(
    transaction: &mut Transaction<'_, Sqlite>,
    now_timestamp: i64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "DELETE FROM profile_attachments WHERE state IN ('pending', 'admission_pending') AND expires_at IS NOT NULL AND expires_at <= ?",
    )
    .bind(now_timestamp)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn quota_usage(
    transaction: &mut Transaction<'_, Sqlite>,
    authority: &PrejoinAuthority,
    now_timestamp: i64,
) -> Result<QuotaUsage, PersistenceError> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS total_count, COALESCE(SUM(size), 0) AS total_bytes, COALESCE(SUM(CASE WHEN admission_room_id = ? THEN 1 ELSE 0 END), 0) AS room_count, COALESCE(SUM(CASE WHEN admission_room_id = ? THEN size ELSE 0 END), 0) AS room_bytes, COALESCE(SUM(CASE WHEN admission_room_id = ? AND state = 'admission_pending' THEN 1 ELSE 0 END), 0) AS pending_room_count, COALESCE(SUM(CASE WHEN admission_room_id = ? AND state = 'admission_pending' THEN size ELSE 0 END), 0) AS pending_room_bytes, COALESCE(SUM(CASE WHEN invite_quota_fingerprint = ? THEN 1 ELSE 0 END), 0) AS invite_count, COALESCE(SUM(CASE WHEN invite_quota_fingerprint = ? THEN size ELSE 0 END), 0) AS invite_bytes FROM profile_attachments WHERE (state = 'bound' OR expires_at > ?) AND (state != 'admission_pending' OR admission_custody_fingerprint != ?)",
    )
    .bind(&authority.room_id)
    .bind(&authority.room_id)
    .bind(&authority.room_id)
    .bind(&authority.room_id)
    .bind(authority.invite_quota_fingerprint.as_slice())
    .bind(authority.invite_quota_fingerprint.as_slice())
    .bind(now_timestamp)
    .bind(authority.custody_fingerprint.as_slice())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(QuotaUsage {
        invite_count: row.try_get("invite_count")?,
        invite_bytes: row.try_get("invite_bytes")?,
        pending_room_count: row.try_get("pending_room_count")?,
        pending_room_bytes: row.try_get("pending_room_bytes")?,
        room_count: row.try_get("room_count")?,
        room_bytes: row.try_get("room_bytes")?,
        total_count: row.try_get("total_count")?,
        total_bytes: row.try_get("total_bytes")?,
    })
}

fn enforce_quota(usage: &QuotaUsage, size: i64) -> Result<(), PersistenceError> {
    if quota_reached(
        usage.invite_count,
        usage.invite_bytes,
        size,
        MAX_ATTACHMENTS_PER_INVITE,
        MAX_ATTACHMENT_BYTES_PER_INVITE,
    ) || quota_reached(
        usage.pending_room_count,
        usage.pending_room_bytes,
        size,
        MAX_PENDING_ATTACHMENTS_PER_ROOM,
        MAX_PENDING_ATTACHMENT_BYTES_PER_ROOM,
    ) || quota_reached(
        usage.room_count,
        usage.room_bytes,
        size,
        MAX_ATTACHMENTS_PER_ROOM,
        MAX_ATTACHMENT_BYTES_PER_ROOM,
    ) || quota_reached(
        usage.total_count,
        usage.total_bytes,
        size,
        MAX_ATTACHMENTS_TOTAL,
        MAX_ATTACHMENT_BYTES_TOTAL,
    ) {
        return Err(rejected(
            "attachment_quota_reached",
            "Pre-join profile avatar storage quota reached.",
        ));
    }
    Ok(())
}

const fn quota_reached(count: i64, bytes: i64, size: i64, max_count: i64, max_bytes: i64) -> bool {
    count >= max_count || bytes.saturating_add(size) > max_bytes
}

fn invalid_stored_size() -> PersistenceError {
    rejected("invalid_state", "Canonical profile avatar size is invalid.")
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
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    };
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
        let first = store
            .store_human_prejoin_avatar(
                &credential,
                &[0x31; 32],
                "../avatar.webp",
                "image/webp",
                valid_image(ImageFormat::WebP),
            )
            .await
            .unwrap_or_else(|error| panic!("store first prejoin avatar: {error}"));
        let second = store
            .store_human_prejoin_avatar(
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
            "SELECT admission_room_id, invite_quota_fingerprint, content_type, size, length(content) AS content_length, created_at, expires_at FROM profile_attachments WHERE attachment_id = ?",
        )
        .bind(&second.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read replacement avatar: {error}"));
        assert_eq!(row.get::<String, _>("admission_room_id"), "general");
        assert_eq!(row.get::<Vec<u8>, _>("invite_quota_fingerprint"), SIGNED);
        assert_eq!(row.get::<String, _>("content_type"), "image/png");
        assert_eq!(
            row.get::<i64, _>("size"),
            row.get::<i64, _>("content_length")
        );
        let created = DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .unwrap_or_else(|error| panic!("parse avatar created_at: {error}"));
        assert_eq!(row.get::<i64, _>("expires_at") - created.timestamp(), 3600);

        store
            .store_human_prejoin_avatar(
                &credential,
                &[0x32; 32],
                "other.png",
                "image/png",
                valid_image(ImageFormat::Png),
            )
            .await
            .unwrap_or_else(|error| panic!("store other browser avatar: {error}"));
        assert_eq!(pending_count(&store).await, 2);

        sqlx::query("UPDATE room_invites SET revoked = 1")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("revoke invite: {error}"));
        assert_rejected_code(
            store
                .store_human_prejoin_avatar(
                    &credential,
                    &[0x33; 32],
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
    async fn invite_quota_is_shared_across_browsers_but_exact_replacement_stays_available() {
        let store = fixture().await;
        let credential = join_credential();
        let mut first_id = String::new();
        for byte in 0..8 {
            let stored = store
                .store_human_prejoin_avatar(
                    &credential,
                    &[byte; 32],
                    "avatar.png",
                    "image/png",
                    valid_image(ImageFormat::Png),
                )
                .await
                .unwrap_or_else(|error| panic!("store quota avatar {byte}: {error}"));
            if byte == 0 {
                first_id = stored.id;
            }
        }
        assert_eq!(pending_count(&store).await, 8);
        assert_rejected_code(
            store
                .store_human_prejoin_avatar(
                    &credential,
                    &[0x40; 32],
                    "ninth.png",
                    "image/png",
                    valid_image(ImageFormat::Png),
                )
                .await,
            "attachment_quota_reached",
        );
        let replacement = store
            .store_human_prejoin_avatar(
                &credential,
                &[0; 32],
                "first-replacement.png",
                "image/png",
                valid_image(ImageFormat::Png),
            )
            .await
            .unwrap_or_else(|error| panic!("replace at quota boundary: {error}"));
        assert_ne!(replacement.id, first_id);
        assert!(!attachment_exists(&store, &first_id).await);
        assert_eq!(pending_count(&store).await, 8);
    }

    #[tokio::test]
    async fn admission_assets_share_runtime_limits_without_charging_ordinary_user_quota() {
        let store = fixture().await;
        let now = Utc::now();
        sqlx::query(
            "WITH digits(value) AS (VALUES (0),(1),(2),(3),(4),(5),(6),(7),(8),(9),(10),(11),(12),(13),(14),(15)), sequence(value) AS (SELECT first.value * 16 + second.value FROM digits AS first CROSS JOIN digits AS second LIMIT 64) INSERT INTO profile_attachments(attachment_id, owner_user_id, admission_room_id, admission_custody_fingerprint, invite_quota_fingerprint, filename, content_type, content, size, created_at, state, expires_at) SELECT printf('bound-avatar-%04d', value), ?, 'general', NULL, ?, 'stored.png', 'image/png', X'00', 1, ?, 'bound', NULL FROM sequence",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .bind(SIGNED.as_slice())
        .bind(now.to_rfc3339())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert admission-bound avatars: {error}"));
        store
            .store_profile_attachment(
                &local_principal(),
                "ordinary.png",
                "image/png",
                valid_image(ImageFormat::Png),
            )
            .await
            .unwrap_or_else(|error| panic!("store ordinary avatar: {error}"));

        let isolated = fixture().await;
        sqlx::query(
            "WITH digits(value) AS (VALUES (0),(1),(2),(3),(4),(5),(6),(7),(8),(9),(10),(11),(12),(13),(14),(15)), sequence(value) AS (SELECT first.value * 256 + second.value * 16 + third.value FROM digits AS first CROSS JOIN digits AS second CROSS JOIN digits AS third) INSERT INTO profile_attachments(attachment_id, owner_user_id, admission_room_id, admission_custody_fingerprint, invite_quota_fingerprint, filename, content_type, content, size, created_at, state, expires_at) SELECT printf('pending-avatar-%018d', value), NULL, 'general', CAST(printf('%032d', value) AS BLOB), ?, 'stored.png', 'image/png', X'00', 1, ?, 'admission_pending', ? FROM sequence",
        )
        .bind(SIGNED.as_slice())
        .bind(now.to_rfc3339())
        .bind((now + Duration::hours(1)).timestamp())
        .execute(&isolated.pool)
        .await
        .unwrap_or_else(|error| panic!("insert runtime-bound pending avatars: {error}"));
        assert_rejected_code(
            isolated
                .store_profile_attachment(
                    &local_principal(),
                    "over-runtime-limit.png",
                    "image/png",
                    valid_image(ImageFormat::Png),
                )
                .await,
            "attachment_quota_reached",
        );
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

    fn local_principal() -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Host".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        }
    }

    async fn pending_count(store: &SqliteStore) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM profile_attachments WHERE state = 'admission_pending'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count pending avatars: {error}"))
    }

    async fn attachment_exists(store: &SqliteStore, attachment_id: &str) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM profile_attachments WHERE attachment_id = ?",
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
