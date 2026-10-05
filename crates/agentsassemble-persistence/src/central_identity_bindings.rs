//! C4a refusal boundary, retained for all non-member identity entry points.
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, account_identity::rejected};

pub(crate) const MEMBER_SCHEMA_VERSION: i64 = 81;

// C4b must execute these exact statements; SQLite stores them without a semicolon.
pub(crate) const TABLE_DDL: &str = "CREATE TABLE central_identity_bindings (
    binding_id TEXT PRIMARY KEY NOT NULL,
    issuer TEXT NOT NULL,
    person_id TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES user_profiles(user_id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    UNIQUE(issuer, person_id),
    UNIQUE(issuer, user_id)
) STRICT";
pub(crate) const USER_INDEX_DDL: &str =
    "CREATE INDEX central_identity_bindings_user ON central_identity_bindings(user_id)";

/// Check inside the credential/admission owner's transaction, including exact replay.
/// An unreadable v81 binding is an error, never anonymous authority.
pub(crate) async fn require_unbound_user(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
) -> Result<(), PersistenceError> {
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut **tx)
            .await?;
    match version.parse::<i64>() {
        Ok(70..=80) => Ok(()),
        Ok(MEMBER_SCHEMA_VERSION | 82) => {
            let bound: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM central_identity_bindings WHERE user_id = ?)",
            )
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await?;
            if bound {
                return Err(rejected(
                    "central_member_unsupported",
                    "This host version does not support central member authority.",
                ));
            }
            Ok(())
        }
        _ => Err(PersistenceError::InvalidSchemaVersion(version)),
    }
}
