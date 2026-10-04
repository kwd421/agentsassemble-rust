use sqlx::{Sqlite, Transaction};

use crate::{OperatorSessionAuthorization, PersistenceError};

/// Transport-authenticated server owner; remote provenance is checked again in storage.
/// Local callers must first consume their native one-use operator ticket.
pub enum ServerOwnerAuthority {
    LocalOperator,
    CentralOwner(Box<OperatorSessionAuthorization>),
    CentralSession(Box<crate::OwnerSessionAuthorization>),
}

impl ServerOwnerAuthority {
    pub(crate) async fn revalidate(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<crate::LocalBootstrapStatus, PersistenceError> {
        if let Self::CentralOwner(session) = self {
            crate::operator_pairing::revalidate_central_owner_session(transaction, session).await?;
        }
        if let Self::CentralSession(session) = self {
            session.revalidate(transaction).await?;
        }
        let bootstrap =
            crate::bootstrap::require_complete_bootstrap_in_transaction(transaction).await?;
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

// Preserve the historical table for additive schema upgrades. No live authority
// reads or writes it; host-owned admission now lives in host_owner_sessions.
pub(crate) const GRANT_DDL: &str = "CREATE TABLE central_owner_grants (
    fingerprint BLOB PRIMARY KEY CHECK(length(fingerprint) = 32),
    device BLOB NOT NULL CHECK(length(device) = 32), origin TEXT NOT NULL,
    generation INTEGER NOT NULL, expires_at INTEGER NOT NULL) STRICT";

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
