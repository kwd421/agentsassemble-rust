use sqlx::{Row, Sqlite, Transaction};

use crate::{OperatorSessionAuthorization, PersistenceError};

/// Transport-authenticated server owner; remote provenance is checked again in storage.
/// Local callers must first consume their native one-use operator ticket.
pub enum ServerOwnerAuthority {
    LocalOperator,
    CentralOwner(Box<OperatorSessionAuthorization>),
    CentralGrant(CentralOwnerGrant),
}

/// A live grant already redeemed by the host against its configured central authority.
/// No wire deserializer or client-chosen local-operator conversion is exposed.
pub struct CentralOwnerGrant {
    server_id: String,
    generation: i64,
    expires_at: chrono::DateTime<chrono::Utc>,
    fingerprint: [u8; 32],
    device: [u8; 32],
    origin: String,
}

impl CentralOwnerGrant {
    /// Captures the verified central response for transactional server/lease checks.
    ///
    /// # Errors
    /// Rejects malformed server identity, nonpositive generation or excessive expiry.
    pub fn verified(
        server_id: String,
        generation: i64,
        expires_at: i64,
        fingerprint: [u8; 32],
        device: [u8; 32],
        origin: String,
    ) -> Result<Self, PersistenceError> {
        let now = chrono::Utc::now();
        let expires_at = chrono::DateTime::from_timestamp(expires_at, 0).ok_or_else(unavailable)?;
        if uuid::Uuid::parse_str(&server_id).is_err()
            || generation < 1
            || expires_at <= now
            || expires_at > now + chrono::Duration::minutes(5)
        {
            return Err(unavailable());
        }
        Ok(Self {
            server_id,
            generation,
            expires_at,
            fingerprint,
            device,
            origin,
        })
    }
}

fn unavailable() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "central_connect_invalid".into(),
        message: "The central owner grant is no longer valid.".to_owned(),
    }
}

impl ServerOwnerAuthority {
    pub(crate) async fn revalidate(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<crate::LocalBootstrapStatus, PersistenceError> {
        if let Self::CentralOwner(session) = self {
            crate::operator_pairing::revalidate_central_owner_session(transaction, session).await?;
        }
        let bootstrap =
            crate::bootstrap::require_complete_bootstrap_in_transaction(transaction).await?;
        if let Self::CentralGrant(grant) = self {
            let generation: Option<String> = sqlx::query_scalar(
                "SELECT value FROM runtime_metadata WHERE key = 'central_endpoint_generation'",
            )
            .fetch_optional(&mut **transaction)
            .await?;
            if bootstrap.server_id != grant.server_id
                || generation.as_deref() != Some(grant.generation.to_string().as_str())
                || grant.expires_at <= chrono::Utc::now()
            {
                return Err(unavailable());
            }
            grant.bind_device(transaction).await?;
        }
        Ok(bootstrap)
    }
}

impl crate::SqliteStore {
    /// Revalidates server ownership without reading a room or its directory.
    ///
    /// # Errors
    /// Rejects incomplete bootstrap and expired, revoked or changed owner custody.
    pub async fn validate_server_owner(
        &self,
        owner: &ServerOwnerAuthority,
    ) -> Result<crate::LocalBootstrapStatus, PersistenceError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let bootstrap = owner.revalidate(&mut transaction).await?;
        transaction.commit().await?;
        Ok(bootstrap)
    }
}

// A short central grant is shared by directory/create/room admission, but only by
// its first browser. Store fingerprints only; bounded cleanup follows grant expiry.
pub(crate) const GRANT_DDL: &str = "CREATE TABLE central_owner_grants (
    fingerprint BLOB PRIMARY KEY CHECK(length(fingerprint) = 32),
    device BLOB NOT NULL CHECK(length(device) = 32), origin TEXT NOT NULL,
    generation INTEGER NOT NULL, expires_at INTEGER NOT NULL) STRICT";

impl CentralOwnerGrant {
    async fn bind_device(&self, tx: &mut Transaction<'_, Sqlite>) -> Result<(), PersistenceError> {
        if let Some(row) = sqlx::query("SELECT device, origin, generation, expires_at FROM central_owner_grants WHERE fingerprint = ?")
            .bind(self.fingerprint.as_slice()).fetch_optional(&mut **tx).await? {
            if row.get::<Vec<u8>, _>("device") != self.device
                || row.get::<String, _>("origin") != self.origin
                || row.get::<i64, _>("generation") != self.generation
                || row.get::<i64, _>("expires_at") != self.expires_at.timestamp() {
                return Err(unavailable());
            }
            return Ok(());
        }
        sqlx::query("DELETE FROM central_owner_grants WHERE expires_at <= ?")
            .bind(chrono::Utc::now().timestamp())
            .execute(&mut **tx)
            .await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM central_owner_grants")
            .fetch_one(&mut **tx)
            .await?;
        if count >= 128 {
            return Err(PersistenceError::CommandRejected {
                code: "pairing_capacity".into(),
                message: "Wait for an existing central grant to expire.".into(),
            });
        }
        sqlx::query("INSERT INTO central_owner_grants(fingerprint, device, origin, generation, expires_at) VALUES (?, ?, ?, ?, ?)")
            .bind(self.fingerprint.as_slice()).bind(self.device.as_slice()).bind(&self.origin)
            .bind(self.generation).bind(self.expires_at.timestamp()).execute(&mut **tx).await?;
        Ok(())
    }
}

/// Server-derived lifecycle identity retaining its exact server-owner provenance.
/// It is never a room admission or a caller-selected trusted principal.
pub struct ServerOwnerLifecycleAuthorization {
    owner: ServerOwnerAuthority,
    principal: agentsassemble_domain::AuthenticatedPrincipal,
}

impl ServerOwnerLifecycleAuthorization {
    #[must_use]
    pub const fn principal(&self) -> &agentsassemble_domain::AuthenticatedPrincipal {
        &self.principal
    }

    pub(crate) async fn revalidate(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<agentsassemble_domain::AuthenticatedPrincipal, PersistenceError> {
        self.owner.revalidate(tx).await?;
        crate::room_lifecycle::resolve_local_owner(tx, &self.principal).await
    }
}

impl crate::SqliteStore {
    /// Derives host lifecycle identity from authenticated server ownership.
    /// Mutation and receipt reads must revalidate the returned provenance again.
    ///
    /// # Errors
    /// Rejects expired or differently bound grants and incomplete host identity.
    pub async fn authorize_server_owner_lifecycle(
        &self,
        owner: ServerOwnerAuthority,
        room_id: String,
    ) -> Result<ServerOwnerLifecycleAuthorization, PersistenceError> {
        use agentsassemble_domain::{
            AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
            LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        owner.revalidate(&mut tx).await?;
        let credential = AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: String::new(),
            room_id,
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        let principal = crate::room_lifecycle::resolve_local_owner(&mut tx, &credential).await?;
        tx.commit().await?;
        Ok(ServerOwnerLifecycleAuthorization { owner, principal })
    }
}
