use agentsassemble_domain::InviteScope;
use chrono::{DateTime, Utc};
use sqlx::{Row, sqlite::SqliteRow};

use crate::{
    PersistenceError, RoomUserIdentity, SqliteStore,
    room_user_identity::{require_current_local_room_manager, resolve_room_user_identity},
};

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

pub struct NewHumanInvite {
    pub signed_token_fingerprint: [u8; 32],
    pub join_code_fingerprint: [u8; 32],
    pub base_participant_id: String,
    pub display_name: String,
    pub invite_scope: InviteScope,
    pub max_uses: i64,
    pub expires_at: DateTime<Utc>,
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
    /// Creates one canonical human invite under current local room-manager authority.
    ///
    /// The caller supplies only credential fingerprints and public invite policy. Room and
    /// creator authority are re-resolved from the current manager inside the write transaction.
    ///
    /// # Errors
    ///
    /// Fails on invalid invite input, stale manager authority, conflicts, or database errors.
    pub async fn create_human_invite_for_local_manager(
        &self,
        manager: &RoomUserIdentity,
        invite: NewHumanInvite,
    ) -> Result<HumanInvite, PersistenceError> {
        validate_new_human_invite(&invite)?;
        let invite_id = fingerprint_hex_prefix(&invite.signed_token_fingerprint);
        let mut transaction = self.pool.begin().await?;
        let current = resolve_room_user_identity(
            &mut transaction,
            &manager.room_id,
            &manager.user_id,
            &manager.participant_id,
        )
        .await?;
        require_current_local_room_manager(&mut transaction, &current).await?;
        let row = sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, 0, ?, ?) RETURNING invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at",
        )
        .bind(invite_id)
        .bind(invite.signed_token_fingerprint.as_slice())
        .bind(invite.join_code_fingerprint.as_slice())
        .bind(&current.room_id)
        .bind(invite.base_participant_id)
        .bind(invite.display_name)
        .bind(invite_scope_text(invite.invite_scope))
        .bind(invite.max_uses)
        .bind(invite.expires_at.timestamp_micros())
        .bind(&current.user_id)
        .bind(invite.created_at.timestamp_micros())
        .fetch_one(&mut *transaction)
        .await?;
        let stored = decode_human_invite(&row)?;
        transaction.commit().await?;
        Ok(stored)
    }

    /// Revokes one room-owned invite for future admissions.
    ///
    /// Existing revoked rows remain successful. Established sessions are separate authority and
    /// deliberately remain live; their exact retry and revocation lifecycle does not belong here.
    ///
    /// # Errors
    ///
    /// Fails on an invalid public ID, stale manager authority, or database errors.
    pub async fn revoke_human_invite_for_local_manager(
        &self,
        manager: &RoomUserIdentity,
        invite_id: &str,
    ) -> Result<bool, PersistenceError> {
        if !is_invite_id(invite_id) {
            return Err(rejected(
                "invalid_human_invite_id",
                "Human invite ID is invalid.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let current = resolve_room_user_identity(
            &mut transaction,
            &manager.room_id,
            &manager.user_id,
            &manager.participant_id,
        )
        .await?;
        require_current_local_room_manager(&mut transaction, &current).await?;
        let found = sqlx::query_scalar::<_, String>(
            "UPDATE room_invites SET revoked = 1 WHERE invite_id = ? AND room_id = ? RETURNING invite_id",
        )
        .bind(invite_id)
        .bind(&current.room_id)
        .fetch_optional(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(found.is_some())
    }

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

fn validate_new_human_invite(invite: &NewHumanInvite) -> Result<(), PersistenceError> {
    if !is_canonical_text(&invite.base_participant_id, 64)
        || !is_canonical_text(&invite.display_name, 128)
        || invite.max_uses < 0
        || invite.expires_at <= invite.created_at
        || !is_exact_microsecond(invite.created_at)
        || !is_exact_microsecond(invite.expires_at)
    {
        return Err(rejected(
            "invalid_human_invite",
            "Human invite policy is invalid.",
        ));
    }
    Ok(())
}

fn is_exact_microsecond(value: DateTime<Utc>) -> bool {
    value.timestamp_subsec_nanos().is_multiple_of(1_000)
}

fn is_canonical_text(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max_chars
        && !value.contains(['\r', '\n'])
}

fn is_invite_id(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

const fn invite_scope_text(scope: InviteScope) -> &'static str {
    match scope {
        InviteScope::ReadWrite => "read_write",
        InviteScope::ReadOnly => "read_only",
    }
}

fn fingerprint_hex_prefix(fingerprint: &[u8; 32]) -> String {
    hex::encode(&fingerprint[..8])
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, Participant,
    };
    use chrono::{Duration, TimeZone, Utc};

    use super::{HumanInvite, MAX_EFFECTIVE_INVITE_USES, NewHumanInvite};
    use crate::{PersistenceError, SqliteStore};

    const GUEST_USER_ID: &str = "invite-user-ab";
    const GUEST_PARTICIPANT_ID: &str = "guest-ab";

    #[tokio::test]
    async fn revoke_blocks_future_admission_without_ending_existing_sessions() {
        let store = fixture().await;
        let manager = store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize manager: {error}"));
        let mut draft = new_invite(0xAB, 0xCD);
        draft.max_uses = 1;
        let invite = store
            .create_human_invite_for_local_manager(&manager, draft)
            .await
            .unwrap_or_else(|error| panic!("create invite: {error}"));
        add_guest_human(&store).await;
        insert_active_session(&store, &invite.invite_id, [0x33; 32]).await;

        assert!(
            store
                .revoke_human_invite_for_local_manager(&manager, &invite.invite_id)
                .await
                .unwrap_or_else(|error| panic!("revoke invite: {error}"))
        );
        assert_eq!(stored_session_state(&store).await, "active");
        assert!(
            store
                .human_invite_by_signed_fingerprint(&invite.signed_token_fingerprint)
                .await
                .unwrap_or_else(|error| panic!("read revoked invite: {error}"))
                .is_some_and(|stored| stored.revoked)
        );

        assert!(
            store
                .revoke_human_invite_for_local_manager(&manager, &invite.invite_id)
                .await
                .unwrap_or_else(|error| panic!("replay invite revoke: {error}"))
        );
        assert_eq!(stored_session_state(&store).await, "active");
        assert!(
            !store
                .revoke_human_invite_for_local_manager(&manager, "ffffffffffffffff")
                .await
                .unwrap_or_else(|error| panic!("revoke missing invite: {error}"))
        );
    }

    #[tokio::test]
    async fn create_derives_room_and_creator_then_rejects_stale_manager() {
        let store = fixture().await;
        let manager = store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize manager: {error}"));
        let mut sub_microsecond = new_invite(0xAA, 0xCC);
        sub_microsecond.expires_at += Duration::nanoseconds(1);
        assert!(matches!(
            store
                .create_human_invite_for_local_manager(&manager, sub_microsecond)
                .await,
            Err(PersistenceError::CommandRejected {
                code: "invalid_human_invite",
                ..
            })
        ));
        let created = store
            .create_human_invite_for_local_manager(&manager, new_invite(0xAB, 0xCD))
            .await
            .unwrap_or_else(|error| panic!("create invite: {error}"));
        assert_eq!(created.invite_id, "abababababababab");
        assert_eq!(created.room_id, "general");
        assert_eq!(created.created_by_user_id, LOCAL_OPERATOR_USER_ID);
        assert_eq!(created.use_count, 0);
        assert!(!created.revoked);

        sqlx::query("DELETE FROM participants WHERE room_id = ? AND participant_id = ?")
            .bind(&manager.room_id)
            .bind(&manager.participant_id)
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove manager membership: {error}"));
        assert!(matches!(
            store
                .create_human_invite_for_local_manager(&manager, new_invite(0xEF, 0x12))
                .await,
            Err(PersistenceError::ParticipantMissing)
        ));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_invites")
                .fetch_one(&store.pool)
                .await
                .unwrap_or_else(|error| panic!("count invites: {error}")),
            1
        );
    }

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

    fn new_invite(signed_marker: u8, join_marker: u8) -> NewHumanInvite {
        NewHumanInvite {
            signed_token_fingerprint: [signed_marker; 32],
            join_code_fingerprint: [join_marker; 32],
            base_participant_id: format!("guest-{signed_marker:02x}"),
            display_name: "Guest".to_owned(),
            invite_scope: InviteScope::ReadWrite,
            max_uses: 5,
            expires_at: Utc
                .timestamp_micros(2_000_000)
                .single()
                .unwrap_or_else(|| panic!("valid expiry")),
            created_at: Utc
                .timestamp_micros(1_000_000)
                .single()
                .unwrap_or_else(|| panic!("valid creation time")),
        }
    }

    async fn insert_active_session(
        store: &SqliteStore,
        invite_id: &str,
        session_fingerprint: [u8; 32],
    ) {
        sqlx::query(
            "INSERT INTO human_room_sessions(admission_key, key_kind, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) VALUES (?, 'one_use', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', ?, ?, ?, 'general', ?, ?, 'browser', 'read_write', ?, NULL, '{}', 1100000, 1900000, 'active')",
        )
        .bind([0x11; 32].as_slice())
        .bind(invite_id)
        .bind([0x22; 32].as_slice())
        .bind(session_fingerprint.as_slice())
        .bind(GUEST_USER_ID)
        .bind(GUEST_PARTICIPANT_ID)
        .bind([0x44; 32].as_slice())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert active session: {error}"));
    }

    async fn stored_session_state(store: &SqliteStore) -> String {
        sqlx::query_scalar("SELECT state FROM human_room_sessions")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read session state: {error}"))
    }

    async fn add_guest_human(store: &SqliteStore) {
        let profile_json = sqlx::query_scalar::<_, String>(
            "SELECT profile_json FROM user_profiles WHERE user_id = ?",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read profile fixture: {error}"));
        sqlx::query(
            "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, ?)",
        )
        .bind(GUEST_USER_ID)
        .bind(GUEST_PARTICIPANT_ID)
        .bind(profile_json)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert guest profile: {error}"));

        let participant_json = sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read participant fixture: {error}"));
        let mut participant: Participant = serde_json::from_str(&participant_json)
            .unwrap_or_else(|error| panic!("decode participant fixture: {error}"));
        participant.participant_id = GUEST_PARTICIPANT_ID.to_owned();
        participant.display_name = "Guest".to_owned();
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', ?, ?)",
        )
        .bind(GUEST_PARTICIPANT_ID)
        .bind(serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode guest participant: {error}")))
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert guest participant: {error}"));
    }
}
