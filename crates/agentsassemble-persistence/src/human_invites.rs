use agentsassemble_domain::InviteScope;
use chrono::{DateTime, Utc};
use sqlx::{Row, sqlite::SqliteRow};

use crate::{PersistenceError, SqliteStore};

const MAX_EFFECTIVE_INVITE_USES: i64 = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanInvite {
    pub invite_id: String,
    pub signed_token_fingerprint: [u8; 32],
    pub join_code_fingerprint: [u8; 32],
    pub room_id: String,
    pub base_participant_id: String,
    pub display_name: String,
    pub invite_scope: InviteScope,
    pub max_uses: i64,
    pub use_count: i64,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
    pub created_by_user_id: String,
    pub created_at: DateTime<Utc>,
}

impl HumanInvite {
    #[must_use]
    pub const fn is_reusable(&self) -> bool {
        self.max_uses != 1
    }

    #[must_use]
    pub const fn effective_use_limit(&self) -> i64 {
        if self.max_uses == 0 || self.max_uses > MAX_EFFECTIVE_INVITE_USES {
            MAX_EFFECTIVE_INVITE_USES
        } else {
            self.max_uses
        }
    }
}

impl SqliteStore {
    /// Finds one canonical human invite by the complete signed-token fingerprint.
    ///
    /// # Errors
    ///
    /// Fails on database errors or any malformed stored authority.
    pub async fn human_invite_by_signed_fingerprint(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<Option<HumanInvite>, PersistenceError> {
        let row = sqlx::query(
            "SELECT invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at FROM room_invites WHERE signed_token_fingerprint = ?",
        )
        .bind(fingerprint.as_slice())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(decode_human_invite).transpose()
    }

    /// Finds one canonical human invite by the complete join-code fingerprint.
    ///
    /// # Errors
    ///
    /// Fails on database errors or any malformed stored authority.
    pub async fn human_invite_by_join_code_fingerprint(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<Option<HumanInvite>, PersistenceError> {
        let row = sqlx::query(
            "SELECT invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at FROM room_invites WHERE join_code_fingerprint = ?",
        )
        .bind(fingerprint.as_slice())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(decode_human_invite).transpose()
    }

    /// Lists the durable human invite rows in stable creation order.
    ///
    /// Expiry and revocation are retained in the result because callers own the
    /// exact current-view policy; this method does not mutate or clean up rows.
    ///
    /// # Errors
    ///
    /// Fails on database errors or any malformed stored authority.
    pub async fn list_human_invites(&self) -> Result<Vec<HumanInvite>, PersistenceError> {
        sqlx::query(
            "SELECT invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at FROM room_invites ORDER BY created_at, invite_id",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(decode_human_invite)
        .collect()
    }
}

fn decode_human_invite(row: &SqliteRow) -> Result<HumanInvite, PersistenceError> {
    let invite_id = row.try_get::<String, _>("invite_id")?;
    let signed_token_fingerprint = fingerprint(row, "signed_token_fingerprint")?;
    let join_code_fingerprint = fingerprint(row, "join_code_fingerprint")?;
    let room_id = row.try_get::<String, _>("room_id")?;
    let base_participant_id = row.try_get::<String, _>("base_participant_id")?;
    let display_name = row.try_get::<String, _>("display_name")?;
    let created_by_user_id = row.try_get::<String, _>("created_by_user_id")?;
    let max_uses = row.try_get::<i64, _>("max_uses")?;
    let use_count = row.try_get::<i64, _>("use_count")?;
    let created_at = timestamp(row, "created_at")?;
    let expires_at = timestamp(row, "expires_at")?;
    let revoked = match row.try_get::<i64, _>("revoked")? {
        0 => false,
        1 => true,
        _ => return Err(PersistenceError::InvalidHumanInvite),
    };
    let invite_scope = match row.try_get::<String, _>("invite_scope")?.as_str() {
        "read_write" => InviteScope::ReadWrite,
        "read_only" => InviteScope::ReadOnly,
        _ => return Err(PersistenceError::InvalidHumanInvite),
    };
    let expected_invite_id = fingerprint_hex_prefix(&signed_token_fingerprint);
    if invite_id != expected_invite_id
        || room_id.is_empty()
        || base_participant_id.is_empty()
        || display_name.is_empty()
        || created_by_user_id.is_empty()
        || max_uses < 0
        || use_count < 0
        || use_count > effective_use_limit(max_uses)
        || expires_at <= created_at
    {
        return Err(PersistenceError::InvalidHumanInvite);
    }
    Ok(HumanInvite {
        invite_id,
        signed_token_fingerprint,
        join_code_fingerprint,
        room_id,
        base_participant_id,
        display_name,
        invite_scope,
        max_uses,
        use_count,
        expires_at,
        revoked,
        created_by_user_id,
        created_at,
    })
}

fn fingerprint(row: &SqliteRow, column: &str) -> Result<[u8; 32], PersistenceError> {
    row.try_get::<Vec<u8>, _>(column)?
        .try_into()
        .map_err(|_| PersistenceError::InvalidHumanInvite)
}

fn timestamp(row: &SqliteRow, column: &str) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::from_timestamp_micros(row.try_get(column)?)
        .ok_or(PersistenceError::InvalidHumanInvite)
}

fn effective_use_limit(max_uses: i64) -> i64 {
    if max_uses == 0 || max_uses > MAX_EFFECTIVE_INVITE_USES {
        MAX_EFFECTIVE_INVITE_USES
    } else {
        max_uses
    }
}

fn fingerprint_hex_prefix(fingerprint: &[u8; 32]) -> String {
    hex::encode(&fingerprint[..8])
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{HumanInvite, MAX_EFFECTIVE_INVITE_USES};
    use crate::SqliteStore;

    #[tokio::test]
    async fn both_fingerprints_resolve_the_same_typed_invite_without_writes() {
        let store = fixture().await;
        let signed = [0xAB; 32];
        let join = [0xCD; 32];
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES ('abababababababab', ?, ?, 'general', 'guest-ab', 'Guest AB', 'read_only', 0, 3, 2000000, 0, 'operator-local-user', 1000000)",
        )
        .bind(signed.as_slice())
        .bind(join.as_slice())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert invite fixture: {error}"));

        let expected = HumanInvite {
            invite_id: "abababababababab".to_owned(),
            signed_token_fingerprint: signed,
            join_code_fingerprint: join,
            room_id: "general".to_owned(),
            base_participant_id: "guest-ab".to_owned(),
            display_name: "Guest AB".to_owned(),
            invite_scope: agentsassemble_domain::InviteScope::ReadOnly,
            max_uses: 0,
            use_count: 3,
            expires_at: Utc
                .timestamp_micros(2_000_000)
                .single()
                .unwrap_or_else(|| panic!("valid expiry")),
            revoked: false,
            created_by_user_id: "operator-local-user".to_owned(),
            created_at: Utc
                .timestamp_micros(1_000_000)
                .single()
                .unwrap_or_else(|| panic!("valid creation time")),
        };

        assert_eq!(
            store
                .human_invite_by_signed_fingerprint(&signed)
                .await
                .unwrap_or_else(|error| panic!("read signed invite: {error}")),
            Some(expected.clone())
        );
        assert_eq!(
            store
                .human_invite_by_join_code_fingerprint(&join)
                .await
                .unwrap_or_else(|error| panic!("read join invite: {error}")),
            Some(expected.clone())
        );
        assert_eq!(
            store
                .list_human_invites()
                .await
                .unwrap_or_else(|error| panic!("list invites: {error}")),
            vec![expected.clone()]
        );
        assert!(expected.is_reusable());
        assert_eq!(expected.effective_use_limit(), MAX_EFFECTIVE_INVITE_USES);

        let missing = [0xEF; 32];
        assert!(
            store
                .human_invite_by_signed_fingerprint(&missing)
                .await
                .unwrap_or_else(|error| panic!("read missing invite: {error}"))
                .is_none()
        );
    }

    async fn fixture() -> SqliteStore {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("e5f63872-a170-4e34-98af-55940ff4a91a", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap authority: {error}"));
        store
            .create_room_for_local_operator(
                "15ebaf41-12b9-4b30-94d1-d62435b30fba",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        store
    }
}
