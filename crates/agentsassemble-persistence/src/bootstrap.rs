use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, UserProfile};
use chrono::Utc;
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
            sqlx::query(*statement).execute(&mut *transaction).await?;
        }
        install_metadata(&mut transaction).await?;
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
    sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('server_id', ?)")
        .bind(uuid::Uuid::new_v4().to_string())
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
    .bind(Utc::now().to_rfc3339())
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
        "complete" => {
            let status = inspect_complete(connection, &marker, server_id).await?;
            if marker.request_id != request_id {
                return Err(rejected(
                    "bootstrap_already_complete",
                    "Local authority was completed by a different bootstrap request.",
                ));
            }
            let requested_digest = bootstrap_digest(
                &marker.authority_lineage_id,
                request_id,
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                profile,
            );
            if requested_digest != marker.initialization_digest {
                return Err(rejected(
                    "bootstrap_request_conflict",
                    "Bootstrap request id was reused with a different profile.",
                ));
            }
            let stored: LocalBootstrapCommit = serde_json::from_str(&marker.result_json)?;
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
        "empty" => {
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
            let digest = bootstrap_digest(
                &marker.authority_lineage_id,
                request_id,
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                profile,
            );
            let status = LocalBootstrapStatus {
                phase: LocalBootstrapPhase::Complete,
                authority_lineage_id: marker.authority_lineage_id,
                server_id,
                profile: Some(profile.clone()),
            };
            let stored = LocalBootstrapCommit {
                status: status.clone(),
                deduplicated: false,
            };
            let completed = sqlx::query(
                "UPDATE local_bootstrap_authority SET state = 'complete', initialization_digest = ?, user_id = ?, participant_id = ?, result_json = ?, completed_at = ? WHERE singleton = 1 AND state = 'initializing' AND request_id = ?",
            )
            .bind(digest)
            .bind(LOCAL_OPERATOR_USER_ID)
            .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
            .bind(serde_json::to_string(&stored)?)
            .bind(Utc::now().to_rfc3339())
            .bind(request_id)
            .execute(&mut *connection)
            .await?;
            if completed.rows_affected() != 1 {
                return Err(bootstrap_repair_required());
            }
            Ok(stored)
        }
        _ => Err(bootstrap_repair_required()),
    }
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
        "complete" => match inspect_complete(connection, &marker, server_id.clone()).await {
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

async fn inspect_complete(
    connection: &mut SqliteConnection,
    marker: &BootstrapMarker,
    server_id: String,
) -> Result<LocalBootstrapStatus, PersistenceError> {
    if uuid::Uuid::parse_str(&marker.authority_lineage_id).is_err()
        || uuid::Uuid::parse_str(&marker.request_id).is_err()
        || marker.schema_revision != crate::schema_version::CURRENT_SCHEMA_VERSION
        || marker.user_id != LOCAL_OPERATOR_USER_ID
        || marker.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
        || !valid_digest(&marker.initialization_digest)
        || marker.result_json.is_empty()
        || marker.completed_at.is_none()
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
        || bootstrap_digest(
            &marker.authority_lineage_id,
            &marker.request_id,
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            initial_profile,
        ) != marker.initialization_digest
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
        server_id,
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
    let product_rows = sqlx::query_scalar::<_, i64>(
        "SELECT (SELECT COUNT(*) FROM rooms) + (SELECT COUNT(*) FROM participants) + (SELECT COUNT(*) FROM user_profiles)",
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(product_rows == 0)
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
        "SELECT authority_lineage_id, state, request_id, schema_revision, initialization_digest, user_id, participant_id, result_json, completed_at FROM local_bootstrap_authority WHERE singleton = 1",
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
        completed_at: row.get("completed_at"),
    })
}

fn bootstrap_digest(
    lineage_id: &str,
    request_id: &str,
    user_id: &str,
    participant_id: &str,
    profile: &UserProfile,
) -> String {
    let mut digest = Sha256::new();
    digest.update(BOOTSTRAP_DIGEST_CONTEXT);
    for field in [
        lineage_id.as_bytes(),
        request_id.as_bytes(),
        user_id.as_bytes(),
        participant_id.as_bytes(),
        profile.display_name.as_bytes(),
        profile.handle.as_bytes(),
        profile.avatar_label.as_bytes(),
    ] {
        digest.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_le_bytes());
        digest.update(field);
    }
    format!("bootstrap-v1-{:x}", digest.finalize())
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
    completed_at: Option<String>,
}
