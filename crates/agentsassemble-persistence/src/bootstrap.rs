use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, UserProfile};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};

use crate::{PersistenceError, SqliteStore, sqlite::SCHEMA_OWNER};

const BOOTSTRAP_DIGEST_CONTEXT: &[u8] = b"agentsassemble-local-bootstrap-v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalBootstrapPhase {
    Empty,
    Initializing,
    Complete,
    RepairRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBootstrapStatus {
    pub phase: LocalBootstrapPhase,
    pub authority_lineage_id: String,
    pub server_id: String,
    pub profile: Option<UserProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBootstrapCommit {
    pub status: LocalBootstrapStatus,
    pub deduplicated: bool,
}

impl SqliteStore {
    pub(crate) async fn initialize(&self) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        for statement in crate::schema::statements() {
            sqlx::query(statement).execute(&mut *transaction).await?;
        }
        install_metadata(&mut transaction, self.host_key.public_key()).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Reads and validates local bootstrap authority without creating product state.
    ///
    /// # Errors
    ///
    /// Returns a persistence error when the marker or completed references are corrupt.
    pub async fn local_bootstrap_status(&self) -> Result<LocalBootstrapStatus, PersistenceError> {
        let mut connection = self.pool.acquire().await?;
        inspect_bootstrap(&mut connection).await
    }

    /// Creates the canonical local human profile in one immediate writer transaction.
    ///
    /// # Errors
    ///
    /// Rejects invalid input, a conflicting request, or inconsistent authority state.
    pub async fn bootstrap_local_authority(
        &self,
        request_id: &str,
        display_name: &str,
    ) -> Result<LocalBootstrapCommit, PersistenceError> {
        if uuid::Uuid::parse_str(request_id).is_err() {
            return Err(rejected(
                "bootstrap_request_invalid",
                "Bootstrap request id must be a UUID.",
            ));
        }
        let profile =
            UserProfile::for_local_identity(display_name, Utc::now()).ok_or_else(|| {
                rejected(
                    "bootstrap_profile_invalid",
                    "Local display name must not be empty.",
                )
            })?;
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let outcome = bootstrap_in_transaction(&mut connection, request_id, &profile).await;
        match outcome {
            Ok(commit) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(commit)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    /// Requires completed bootstrap before issuing any local credential.
    ///
    /// # Errors
    ///
    /// Fails closed for empty, initializing, or inconsistent authority state.
    pub async fn require_local_bootstrap_complete(&self) -> Result<(), PersistenceError> {
        match self.local_bootstrap_status().await? {
            LocalBootstrapStatus {
                phase: LocalBootstrapPhase::Complete,
                ..
            } => Ok(()),
            LocalBootstrapStatus {
                phase: LocalBootstrapPhase::RepairRequired,
                ..
            } => Err(rejected(
                "bootstrap_repair_required",
                "Local authority requires explicit repair before credentials can be issued.",
            )),
            _ => Err(rejected(
                "bootstrap_required",
                "Local identity bootstrap is not complete.",
            )),
        }
    }
}

async fn install_metadata(
    transaction: &mut Transaction<'_, Sqlite>,
    host_public_key: &[u8; 32],
) -> Result<(), PersistenceError> {
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'schema_owner'",
    )
    .fetch_optional(&mut **transaction)
    .await?;
    match owner {
        Some(owner) if owner != SCHEMA_OWNER => {
            return Err(PersistenceError::AuthorityConflict(owner));
        }
        Some(_) => {}
        None => {
            sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('schema_owner', ?)")
                .bind(SCHEMA_OWNER)
                .execute(&mut **transaction)
                .await?;
        }
    }
    let server_id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('server_id', ?)")
        .bind(&server_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO runtime_host_identity(singleton, server_id, public_key) VALUES (1, ?, ?)",
    )
    .bind(&server_id)
    .bind(host_public_key.as_slice())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO runtime_metadata(key, value) VALUES ('schema_version', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(crate::schema_version::CURRENT_SCHEMA_VERSION.to_string())
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO local_bootstrap_authority(singleton, authority_lineage_id, state, schema_revision, created_at) VALUES (1, ?, 'empty', ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(crate::schema_version::CURRENT_SCHEMA_VERSION)
    .bind(canonical_timestamp(Utc::now()))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn bootstrap_in_transaction(
    connection: &mut SqliteConnection,
    request_id: &str,
    profile: &UserProfile,
) -> Result<LocalBootstrapCommit, PersistenceError> {
    let marker = load_marker(connection).await?;
    let server_id = load_server_id(connection).await?;
    match marker.state.as_str() {
        "complete" => replay_bootstrap(connection, &marker, &server_id, request_id, profile).await,
        "empty" => complete_bootstrap(connection, marker, server_id, request_id, profile).await,
        _ => Err(bootstrap_repair_required()),
    }
}

async fn replay_bootstrap(
    connection: &mut SqliteConnection,
    marker: &BootstrapMarker,
    server_id: &str,
    request_id: &str,
    profile: &UserProfile,
) -> Result<LocalBootstrapCommit, PersistenceError> {
    let status = inspect_complete(connection, marker, server_id).await?;
    if marker.request_id != request_id {
        return Err(rejected(
            "bootstrap_already_complete",
            "Local authority was completed by a different bootstrap request.",
        ));
    }
    let stored: LocalBootstrapCommit = serde_json::from_str(&marker.result_json)?;
    let Some(initial_profile) = stored.status.profile.as_ref() else {
        return Err(bootstrap_repair_required());
    };
    let mut requested_profile = profile.clone();
    requested_profile.created_at = initial_profile.created_at;
    requested_profile.updated_at = initial_profile.updated_at;
    let requested_digest = bootstrap_digest(&BootstrapDigestContract {
        lineage_id: &marker.authority_lineage_id,
        request_id,
        user_id: LOCAL_OPERATOR_USER_ID,
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID,
        schema_revision: marker.schema_revision,
        server_id,
        created_at: &marker.created_at,
        completed_at: marker.completed_at.as_deref().unwrap_or_default(),
        profile: &requested_profile,
    });
    if requested_digest != marker.initialization_digest {
        return Err(rejected(
            "bootstrap_request_conflict",
            "Bootstrap request id was reused with a different profile.",
        ));
    }
    if stored.status.authority_lineage_id != status.authority_lineage_id
        || stored.status.server_id != status.server_id
    {
        return Err(bootstrap_repair_required());
    }
    Ok(LocalBootstrapCommit {
        status: stored.status,
        deduplicated: true,
    })
}

async fn complete_bootstrap(
    connection: &mut SqliteConnection,
    marker: BootstrapMarker,
    server_id: String,
    request_id: &str,
    profile: &UserProfile,
) -> Result<LocalBootstrapCommit, PersistenceError> {
    require_empty_marker(connection, &marker).await?;
    let claimed = sqlx::query(
        "UPDATE local_bootstrap_authority SET state = 'initializing', request_id = ? WHERE singleton = 1 AND state = 'empty'",
    )
    .bind(request_id)
    .execute(&mut *connection)
    .await?;
    if claimed.rows_affected() != 1 {
        return Err(bootstrap_repair_required());
    }
    sqlx::query(
        "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, ?)",
    )
    .bind(LOCAL_OPERATOR_USER_ID)
    .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
    .bind(serde_json::to_string(profile)?)
    .execute(&mut *connection)
    .await?;
    let completed_at = canonical_timestamp(Utc::now());
    let digest = bootstrap_digest(&BootstrapDigestContract {
        lineage_id: &marker.authority_lineage_id,
        request_id,
        user_id: LOCAL_OPERATOR_USER_ID,
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID,
        schema_revision: marker.schema_revision,
        server_id: &server_id,
        created_at: &marker.created_at,
        completed_at: &completed_at,
        profile,
    });
    let stored = LocalBootstrapCommit {
        status: LocalBootstrapStatus {
            phase: LocalBootstrapPhase::Complete,
            authority_lineage_id: marker.authority_lineage_id,
            server_id,
            profile: Some(profile.clone()),
        },
        deduplicated: false,
    };
    let completed = sqlx::query(
        "UPDATE local_bootstrap_authority SET state = 'complete', initialization_digest = ?, user_id = ?, participant_id = ?, result_json = ?, completed_at = ? WHERE singleton = 1 AND state = 'initializing' AND request_id = ?",
    )
    .bind(digest)
    .bind(LOCAL_OPERATOR_USER_ID)
    .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
    .bind(serde_json::to_string(&stored)?)
    .bind(completed_at)
    .bind(request_id)
    .execute(&mut *connection)
    .await?;
    if completed.rows_affected() != 1 {
        return Err(bootstrap_repair_required());
    }
    Ok(stored)
}

async fn inspect_bootstrap(
    connection: &mut SqliteConnection,
) -> Result<LocalBootstrapStatus, PersistenceError> {
    let marker = load_marker(connection).await?;
    let server_id = load_server_id(connection).await?;
    match marker.state.as_str() {
        "empty" if empty_marker_is_consistent(connection, &marker).await? => {
            Ok(LocalBootstrapStatus {
                phase: LocalBootstrapPhase::Empty,
                authority_lineage_id: marker.authority_lineage_id,
                server_id,
                profile: None,
            })
        }
        "complete" => match inspect_complete(connection, &marker, &server_id).await {
            Ok(status) => Ok(status),
            Err(PersistenceError::CommandRejected {
                code: "bootstrap_repair_required",
                ..
            }) => Ok(repair_status(marker.authority_lineage_id, server_id)),
            Err(error) => Err(error),
        },
        _ => Ok(LocalBootstrapStatus {
            phase: LocalBootstrapPhase::RepairRequired,
            authority_lineage_id: marker.authority_lineage_id,
            server_id,
            profile: None,
        }),
    }
}

pub(crate) async fn require_complete_bootstrap_in_transaction(
    connection: &mut SqliteConnection,
) -> Result<LocalBootstrapStatus, PersistenceError> {
    let marker = load_marker(connection).await?;
    let server_id = load_server_id(connection).await?;
    match marker.state.as_str() {
        "complete" => inspect_complete(connection, &marker, &server_id).await,
        "empty" => Err(rejected(
            "bootstrap_required",
            "Local identity bootstrap is not complete.",
        )),
        _ => Err(bootstrap_repair_required()),
    }
}

async fn inspect_complete(
    connection: &mut SqliteConnection,
    marker: &BootstrapMarker,
    server_id: &str,
) -> Result<LocalBootstrapStatus, PersistenceError> {
    let created_at = parse_canonical_timestamp(&marker.created_at)?;
    let completed_at = marker
        .completed_at
        .as_deref()
        .ok_or_else(bootstrap_repair_required)
        .and_then(parse_canonical_timestamp)?;
    if uuid::Uuid::parse_str(&marker.authority_lineage_id).is_err()
        || uuid::Uuid::parse_str(&marker.request_id).is_err()
        || marker.schema_revision != crate::schema_version::CURRENT_SCHEMA_VERSION
        || marker.user_id != LOCAL_OPERATOR_USER_ID
        || marker.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
        || !valid_digest(&marker.initialization_digest)
        || marker.result_json.is_empty()
        || created_at > completed_at
    {
        return Err(bootstrap_repair_required());
    }
    let stored: LocalBootstrapCommit =
        serde_json::from_str(&marker.result_json).map_err(|_| bootstrap_repair_required())?;
    let Some(initial_profile) = stored.status.profile.as_ref() else {
        return Err(bootstrap_repair_required());
    };
    if stored.deduplicated
        || stored.status.phase != LocalBootstrapPhase::Complete
        || stored.status.authority_lineage_id != marker.authority_lineage_id
        || stored.status.server_id != server_id
        || initial_profile.revision != 1
        || initial_profile.created_at != initial_profile.updated_at
        || initial_profile.created_at < created_at
        || initial_profile.created_at > completed_at
        || bootstrap_digest(&BootstrapDigestContract {
            lineage_id: &marker.authority_lineage_id,
            request_id: &marker.request_id,
            user_id: LOCAL_OPERATOR_USER_ID,
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID,
            schema_revision: marker.schema_revision,
            server_id,
            created_at: &marker.created_at,
            completed_at: marker.completed_at.as_deref().unwrap_or_default(),
            profile: initial_profile,
        }) != marker.initialization_digest
    {
        return Err(bootstrap_repair_required());
    }
    let row =
        sqlx::query("SELECT participant_id, profile_json FROM user_profiles WHERE user_id = ?")
            .bind(LOCAL_OPERATOR_USER_ID)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(bootstrap_repair_required)?;
    if row.get::<String, _>("participant_id") != LOCAL_OPERATOR_PARTICIPANT_ID {
        return Err(bootstrap_repair_required());
    }
    let profile: UserProfile = serde_json::from_str(row.get::<String, _>("profile_json").as_str())
        .map_err(|_| bootstrap_repair_required())?;
    if profile.revision < 1 {
        return Err(bootstrap_repair_required());
    }
    Ok(LocalBootstrapStatus {
        phase: LocalBootstrapPhase::Complete,
        authority_lineage_id: marker.authority_lineage_id.clone(),
        server_id: server_id.to_owned(),
        profile: Some(profile),
    })
}

fn repair_status(authority_lineage_id: String, server_id: String) -> LocalBootstrapStatus {
    LocalBootstrapStatus {
        phase: LocalBootstrapPhase::RepairRequired,
        authority_lineage_id,
        server_id,
        profile: None,
    }
}

async fn require_empty_marker(
    connection: &mut SqliteConnection,
    marker: &BootstrapMarker,
) -> Result<(), PersistenceError> {
    if empty_marker_is_consistent(connection, marker).await? {
        Ok(())
    } else {
        Err(bootstrap_repair_required())
    }
}

async fn empty_marker_is_consistent(
    connection: &mut SqliteConnection,
    marker: &BootstrapMarker,
) -> Result<bool, PersistenceError> {
    if uuid::Uuid::parse_str(&marker.authority_lineage_id).is_err()
        || parse_canonical_timestamp(&marker.created_at).is_err()
        || marker.schema_revision != crate::schema_version::CURRENT_SCHEMA_VERSION
        || !marker.request_id.is_empty()
        || !marker.initialization_digest.is_empty()
        || !marker.user_id.is_empty()
        || !marker.participant_id.is_empty()
        || !marker.result_json.is_empty()
        || marker.completed_at.is_some()
    {
        return Ok(false);
    }
    for table in crate::schema::product_tables() {
        let has_rows_sql = format!("SELECT EXISTS(SELECT 1 FROM {} LIMIT 1)", table.name);
        if sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(has_rows_sql))
            .fetch_one(&mut *connection)
            .await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn load_server_id(connection: &mut SqliteConnection) -> Result<String, PersistenceError> {
    let server_id = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'server_id'",
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PersistenceError::InvalidServerId)?;
    uuid::Uuid::parse_str(&server_id).map_err(|_| PersistenceError::InvalidServerId)?;
    Ok(server_id)
}

async fn load_marker(
    connection: &mut SqliteConnection,
) -> Result<BootstrapMarker, PersistenceError> {
    let row = sqlx::query(
        "SELECT authority_lineage_id, state, request_id, schema_revision, initialization_digest, user_id, participant_id, result_json, created_at, completed_at FROM local_bootstrap_authority WHERE singleton = 1",
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(bootstrap_repair_required)?;
    Ok(BootstrapMarker {
        authority_lineage_id: row.get("authority_lineage_id"),
        state: row.get("state"),
        request_id: row.get("request_id"),
        schema_revision: row.get("schema_revision"),
        initialization_digest: row.get("initialization_digest"),
        user_id: row.get("user_id"),
        participant_id: row.get("participant_id"),
        result_json: row.get("result_json"),
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
    })
}

struct BootstrapDigestContract<'a> {
    lineage_id: &'a str,
    request_id: &'a str,
    user_id: &'a str,
    participant_id: &'a str,
    schema_revision: i64,
    server_id: &'a str,
    created_at: &'a str,
    completed_at: &'a str,
    profile: &'a UserProfile,
}

fn bootstrap_digest(contract: &BootstrapDigestContract<'_>) -> String {
    let profile = contract.profile;
    let mut digest = Sha256::new();
    digest.update(BOOTSTRAP_DIGEST_CONTEXT);
    digest.update(contract.schema_revision.to_le_bytes());
    for field in [
        contract.lineage_id.as_bytes(),
        contract.request_id.as_bytes(),
        contract.user_id.as_bytes(),
        contract.participant_id.as_bytes(),
        contract.server_id.as_bytes(),
        contract.created_at.as_bytes(),
        contract.completed_at.as_bytes(),
        &profile.revision.to_le_bytes(),
        profile.display_name.as_bytes(),
        profile.handle.as_bytes(),
        profile.status.as_bytes(),
        profile.custom_status.as_bytes(),
        profile.avatar_label.as_bytes(),
        profile.avatar_image_url.as_bytes(),
        profile.banner_preset.as_bytes(),
        profile.accent_color.as_bytes(),
        &[u8::from(profile.mic_muted)],
        &[u8::from(profile.deafened)],
        canonical_timestamp(profile.created_at).as_bytes(),
        canonical_timestamp(profile.updated_at).as_bytes(),
    ] {
        digest.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_le_bytes());
        digest.update(field);
    }
    format!("bootstrap-v1-{:x}", digest.finalize())
}

fn canonical_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_canonical_timestamp(value: &str) -> Result<DateTime<Utc>, PersistenceError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| bootstrap_repair_required())?
        .with_timezone(&Utc);
    if canonical_timestamp(parsed) != value {
        return Err(bootstrap_repair_required());
    }
    Ok(parsed)
}

fn valid_digest(value: &str) -> bool {
    value.len() == "bootstrap-v1-".len() + 64
        && value
            .strip_prefix("bootstrap-v1-")
            .is_some_and(|digest| digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn bootstrap_repair_required() -> PersistenceError {
    rejected(
        "bootstrap_repair_required",
        "Local bootstrap authority is inconsistent and requires explicit repair.",
    )
}

fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

struct BootstrapMarker {
    authority_lineage_id: String,
    state: String,
    request_id: String,
    schema_revision: i64,
    initialization_digest: String,
    user_id: String,
    participant_id: String,
    result_json: String,
    created_at: String,
    completed_at: Option<String>,
}
