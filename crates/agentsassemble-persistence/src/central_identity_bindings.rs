//! Read-only C4a boundary. C4b owns creation and migration of member authority.
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, account_identity::rejected};

pub(crate) const MEMBER_SCHEMA_VERSION: i64 = 81;

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
        Ok(crate::CURRENT_SCHEMA_VERSION) => Ok(()),
        Ok(MEMBER_SCHEMA_VERSION) => {
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
