//! Schema-specific identity snapshot pages. Stored replay JSON never leaves `SQLite`.
use sqlx::{Row, Sqlite, Transaction};

use crate::{MemberRemovalKey, PersistenceError, SqliteStore};

pub use agentsassemble_domain::DEPARTED_USER_NAME;

// These keys are the stable primary keys, encoded identically for selection and checkpoint.
const SNAPSHOTS: &[(&str, &str, &str)] = &[
    (
        "room_events",
        "event_json",
        "json_array(room_id,printf('%020d',seq))",
    ),
    (
        "member_admissions",
        "result_json",
        "json_array(binding_id,invite_id)",
    ),
    ("human_room_sessions", "result_json", "hex(admission_key)"),
    (
        "command_results",
        "result_json",
        "json_array(room_id,principal_id,request_id)",
    ),
    (
        "room_create_results",
        "result_json",
        "json_array(principal_id,request_id)",
    ),
    (
        "room_delete_results",
        "result_json",
        "json_array(room_id,principal_id,request_id)",
    ),
    (
        "lifecycle_command_reservations",
        "prepared_result_json",
        "json_array(room_id,principal_id,request_id)",
    ),
];

// Match identity objects, never arbitrary text/name occurrences. agent_id is the human
// admission result's historical spelling. An event's explicit subject precedes actor.
const IDENTITY: &str = "(coalesce(json_extract(o.value,'$.participant_id'),json_extract(o.value,'$.agent_id'),json_extract(o.value,'$.actor_id'),json_extract(o.value,'$.actor.participant_id'))=?1 OR (?2 IS NOT NULL AND json_extract(o.value,'$.user_id')=?2))";

impl SqliteStore {
    /// Executes one bounded metadata or one-row avatar page of committed person work.
    /// # Errors
    /// Unknown schema/phase, invalid JSON and checkpoint failures leave work pending.
    pub async fn advance_member_removal_snapshots(
        &self,
        key: &MemberRemovalKey,
    ) -> Result<String, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        let row = sqlx::query("SELECT phase,cursor,user_id,participant_id FROM central_member_removals WHERE issuer=? AND person_id=? AND schema_revision=87")
            .bind(key.issuer()).bind(key.person_id()).fetch_one(&mut *tx).await?;
        let phase: String = row.try_get("phase")?;
        let cursor: String = row.try_get("cursor")?;
        let user: Option<String> = row.try_get("user_id")?;
        let actor: Option<String> = row.try_get("participant_id")?;
        let (next, last) = match phase.as_str() {
            "member_invites" => {
                revoke_invite_page(&mut tx, user.as_deref(), actor.as_deref(), &cursor).await?
            }
            "profile" => {
                sqlx::query("UPDATE user_profiles SET profile_json=json_set(profile_json,'$.display_name',?,'$.handle','','$.custom_status','','$.avatar_label','','$.avatar_image_url','','$.banner_preset','default','$.accent_color','#5865f2','$.status','offline','$.revision',json_extract(profile_json,'$.revision')+1) WHERE user_id=?")
                    .bind(DEPARTED_USER_NAME).bind(&user).execute(&mut *tx).await?;
                ("avatars".into(), String::new())
            }
            "avatars" => avatar_page(&mut tx, user.as_deref(), &cursor).await?,
            "search" => search_page(&mut tx, actor.as_deref(), &cursor).await?,
            _ => snapshot_page(&mut tx, &phase, &cursor, user.as_deref(), actor.as_deref()).await?,
        };
        if serde_json::to_vec(&(key.issuer(), key.person_id(), &next, &last))?.len() > 128 * 1024 {
            return Err(invalid());
        }
        crate::central_removal_authority::checkpoint(&mut tx, key, &phase, &next, &last).await?;
        tx.commit().await?;
        Ok(next)
    }
}

async fn revoke_invite_page(
    tx: &mut Transaction<'_, Sqlite>,
    user: Option<&str>,
    actor: Option<&str>,
    cursor: &str,
) -> Result<(String, String), PersistenceError> {
    let invite: Option<String> = sqlx::query_scalar("SELECT invite_id FROM room_invites WHERE invite_id>? AND (created_by_user_id=? OR base_participant_id=?) ORDER BY invite_id LIMIT 1")
        .bind(cursor).bind(user).bind(actor).fetch_optional(&mut **tx).await?;
    if let Some(id) = invite {
        sqlx::query("UPDATE room_invites SET revoked=1,display_name=CASE WHEN base_participant_id=? THEN ? ELSE display_name END WHERE invite_id=?")
            .bind(actor).bind(DEPARTED_USER_NAME).bind(&id).execute(&mut **tx).await?;
        Ok(("member_invites".into(), id))
    } else {
        Ok(("profile".into(), String::new()))
    }
}

async fn avatar_page(
    tx: &mut Transaction<'_, Sqlite>,
    user: Option<&str>,
    cursor: &str,
) -> Result<(String, String), PersistenceError> {
    let asset: Option<String> = sqlx::query_scalar("SELECT attachment_id FROM profile_avatar_assets WHERE owner_user_id=? AND attachment_id>? AND size<=10485760 ORDER BY attachment_id LIMIT 1")
        .bind(user).bind(cursor).fetch_optional(&mut **tx).await?;
    if let Some(id) = asset {
        sqlx::query("DELETE FROM profile_avatar_assets WHERE attachment_id=? AND owner_user_id=?")
            .bind(&id)
            .bind(user)
            .execute(&mut **tx)
            .await?;
        Ok(("avatars".into(), id))
    } else {
        Ok((SNAPSHOTS[0].0.into(), String::new()))
    }
}

async fn snapshot_page(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    cursor: &str,
    user: Option<&str>,
    actor: Option<&str>,
) -> Result<(String, String), PersistenceError> {
    let index = SNAPSHOTS
        .iter()
        .position(|s| s.0 == phase)
        .ok_or_else(invalid)?;
    let (table, column, primary) = SNAPSHOTS[index];
    let select =
        format!("SELECT {primary} AS id FROM {table} WHERE {primary}>? ORDER BY {primary} LIMIT 1");
    // Identifiers/expressions come only from the fixed schema inventory above; data is bound.
    let id: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(select))
        .bind(cursor)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(id) = id else {
        return Ok((
            SNAPSHOTS.get(index + 1).map_or("search", |s| s.0).into(),
            String::new(),
        ));
    };
    if id.len() > 64 * 1024 {
        return Err(invalid());
    }
    // Only patch descriptors (the fixed SQL and key) cross the application boundary.
    // Nested snapshots/other principals' replay results are transformed in the same row.
    let update = format!("WITH RECURSIVE
        original(doc) AS (SELECT {column} FROM {table} WHERE {primary}=?3),
        identities(path) AS (SELECT o.fullkey FROM original,json_tree(doc) o WHERE o.type='object' AND {IDENTITY}),
        objects(path) AS (SELECT path FROM identities UNION SELECT path||'.profile' FROM identities,original WHERE json_type(doc,path||'.profile')='object'),
        patches(path,value) AS (
            SELECT p.fullkey,CASE WHEN p.key IN ('display_name','name') THEN ?4 ELSE '' END
            FROM original,json_tree(doc) p JOIN objects o ON p.path=o.path
            WHERE p.key IN ('display_name','name','avatar_image_url','avatar_url','avatar_label','handle','custom_status')
            UNION ALL SELECT path||'.guide','{{}}' FROM identities,original WHERE json_type(doc,path||'.guide')='object'),
        numbered(n,path,value) AS (SELECT row_number() OVER (ORDER BY path),path,value FROM patches),
        rewrite(n,doc) AS (SELECT 0,doc FROM original UNION ALL SELECT p.n,CASE WHEN p.path LIKE '%.guide' THEN json_set(r.doc,p.path,json(p.value)) ELSE json_set(r.doc,p.path,p.value) END FROM rewrite r JOIN numbered p ON p.n=r.n+1)
        UPDATE {table} SET {column}=(SELECT doc FROM rewrite ORDER BY n DESC LIMIT 1) WHERE {primary}=?3");
    sqlx::query(sqlx::AssertSqlSafe(update))
        .bind(actor)
        .bind(user)
        .bind(&id)
        .bind(DEPARTED_USER_NAME)
        .execute(&mut **tx)
        .await?;
    Ok((phase.into(), id))
}

async fn search_page(
    tx: &mut Transaction<'_, Sqlite>,
    actor: Option<&str>,
    cursor: &str,
) -> Result<(String, String), PersistenceError> {
    let after = if cursor.is_empty() {
        0
    } else {
        cursor.parse::<i64>().map_err(|_| invalid())?
    };
    let id:Option<i64> = sqlx::query_scalar("SELECT r.id FROM room_message_search_records r JOIN room_events e ON e.room_id=r.room_id AND e.seq=r.event_seq WHERE r.id>? AND json_extract(e.event_json,'$.actor.participant_id')=? ORDER BY r.id LIMIT 1")
        .bind(after).bind(actor).fetch_optional(&mut **tx).await?;
    if let Some(id) = id {
        crate::message_search_index::anonymize_search_author(tx, id).await?;
        Ok(("search".into(), id.to_string()))
    } else {
        Ok(("rooms".into(), String::new()))
    }
}

pub(crate) fn invalid() -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: "account_removal_phase_unhandled".into(),
        message: "Identity removal requires its intact schema-specific snapshot phase.".into(),
    }
}
