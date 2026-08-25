use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

const HOST_PRIVATE_KEY_BYTES: usize = 32;

/// Persistent private signing identity bound to one server authority.
///
/// This type deliberately implements neither `Debug` nor serialization so the
/// private seed cannot enter generic diagnostics or wire projections.
pub struct PersistentHostIdentity {
    server_id: String,
    private_key_seed: [u8; HOST_PRIVATE_KEY_BYTES],
}

impl PersistentHostIdentity {
    #[must_use]
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    #[must_use]
    pub const fn private_key_seed(&self) -> &[u8; HOST_PRIVATE_KEY_BYTES] {
        &self.private_key_seed
    }
}

impl SqliteStore {
    /// Loads the exact Ed25519 seed bound to this server ID.
    ///
    /// # Errors
    ///
    /// Rejects missing, malformed, or differently bound host identity state.
    pub async fn host_identity(&self) -> Result<PersistentHostIdentity, PersistenceError> {
        let row = sqlx::query(
            "SELECT host.server_id, host.private_key_seed, metadata.value AS expected_server_id
             FROM runtime_host_identity AS host
             JOIN runtime_metadata AS metadata ON metadata.key = 'server_id'
             WHERE host.singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PersistenceError::InvalidHostIdentity)?;
        let server_id = row.get::<String, _>("server_id");
        let expected_server_id = row.get::<String, _>("expected_server_id");
        if server_id != expected_server_id || uuid::Uuid::parse_str(&server_id).is_err() {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        let private_key_seed = row
            .get::<Vec<u8>, _>("private_key_seed")
            .try_into()
            .map_err(|_| PersistenceError::InvalidHostIdentity)?;
        Ok(PersistentHostIdentity {
            server_id,
            private_key_seed,
        })
    }
}
