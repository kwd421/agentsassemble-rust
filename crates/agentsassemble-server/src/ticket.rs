use std::{collections::HashMap, sync::Arc, time::Duration};

use agentsassemble_domain::AuthenticatedPrincipal;
use thiserror::Error;
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct StoredTicketGrant {
    authority: TicketAuthority,
    proof_key: String,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
enum TicketAuthority {
    Room(AuthenticatedPrincipal),
    ServerOperator { principal_id: String },
    CentralRegistration { principal_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedTicket {
    pub ticket: String,
    pub proof_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedTicket {
    pub principal: AuthenticatedPrincipal,
    pub proof_key: String,
    pub connection_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedServerOperatorTicket {
    pub principal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedCentralRegistrationTicket {
    pub principal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumedProfileTicket {
    Room(AuthenticatedPrincipal),
    ServerOperator { principal_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TicketError {
    #[error("ticket is invalid, expired, or already used")]
    Invalid,
}

#[derive(Clone)]
pub struct TicketStore {
    grants: Arc<Mutex<HashMap<String, StoredTicketGrant>>>,
    ttl: Duration,
    capacity: usize,
}

impl TicketStore {
    #[must_use]
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            grants: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            capacity: capacity.max(1),
        }
    }

    /// Issues one bounded, short-lived credential for a resolved principal.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the bounded grant store is full.
    pub async fn issue(
        &self,
        principal: AuthenticatedPrincipal,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_authority(TicketAuthority::Room(principal)).await
    }

    /// Issues one server-operator HTTP credential that cannot authenticate a room socket.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the bounded grant store is full or the identity is empty.
    pub async fn issue_server_operator(
        &self,
        principal_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if principal_id.is_empty() {
            return Err(TicketError::Invalid);
        }
        self.issue_authority(TicketAuthority::ServerOperator { principal_id })
            .await
    }

    /// Issues one central-registration-only HTTP credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the bounded grant store is full or the identity is empty.
    pub async fn issue_central_registration(
        &self,
        principal_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if principal_id.is_empty() {
            return Err(TicketError::Invalid);
        }
        self.issue_authority(TicketAuthority::CentralRegistration { principal_id })
            .await
    }

    async fn issue_authority(
        &self,
        authority: TicketAuthority,
    ) -> Result<IssuedTicket, TicketError> {
        let now = Instant::now();
        let mut grants = self.grants.lock().await;
        grants.retain(|_, grant| grant.expires_at > now);
        if grants.len() >= self.capacity {
            return Err(TicketError::Invalid);
        }
        let ticket = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let proof_key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        grants.insert(
            ticket.clone(),
            StoredTicketGrant {
                authority,
                proof_key: proof_key.clone(),
                expires_at: now + self.ttl,
            },
        );
        Ok(IssuedTicket { ticket, proof_key })
    }

    /// Removes and resolves a credential exactly once.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for unknown, expired, or previously consumed tickets.
    pub async fn consume(&self, ticket: &str) -> Result<ConsumedTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::Room(principal) = grant.authority else {
            return Err(TicketError::Invalid);
        };
        Ok(ConsumedTicket {
            principal,
            proof_key: grant.proof_key,
            connection_nonce: crate::server_proof::derive_connection_nonce(ticket),
        })
    }

    /// Removes and resolves a server-operator HTTP credential exactly once.
    ///
    /// A room credential is consumed and rejected instead of being accepted across scopes.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for the wrong purpose, unknown, expired, or reused tickets.
    pub async fn consume_server_operator(
        &self,
        ticket: &str,
    ) -> Result<ConsumedServerOperatorTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::ServerOperator { principal_id } = grant.authority else {
            return Err(TicketError::Invalid);
        };
        Ok(ConsumedServerOperatorTicket { principal_id })
    }

    /// Removes and resolves a central-registration credential exactly once.
    ///
    /// Every wrong-purpose credential is consumed and rejected.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for the wrong purpose, unknown, expired, or reused tickets.
    pub async fn consume_central_registration(
        &self,
        ticket: &str,
    ) -> Result<ConsumedCentralRegistrationTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::CentralRegistration { principal_id } = grant.authority else {
            return Err(TicketError::Invalid);
        };
        Ok(ConsumedCentralRegistrationTicket { principal_id })
    }

    /// Removes and resolves a one-use credential accepted by the server-wide profile surface.
    ///
    /// Room participants retain their own profile authority, while the private-control-derived
    /// server operator may access only the canonical local human profile selected by the route.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for an unknown, expired, or reused ticket.
    pub async fn consume_profile(
        &self,
        ticket: &str,
    ) -> Result<ConsumedProfileTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        Ok(match grant.authority {
            TicketAuthority::Room(principal) => ConsumedProfileTicket::Room(principal),
            TicketAuthority::ServerOperator { principal_id } => {
                ConsumedProfileTicket::ServerOperator { principal_id }
            }
            TicketAuthority::CentralRegistration { .. } => return Err(TicketError::Invalid),
        })
    }

    async fn consume_grant(&self, ticket: &str) -> Result<StoredTicketGrant, TicketError> {
        let grant = self
            .grants
            .lock()
            .await
            .remove(ticket)
            .ok_or(TicketError::Invalid)?;
        if grant.expires_at <= Instant::now() {
            return Err(TicketError::Invalid);
        }
        Ok(grant)
    }

    #[must_use]
    pub fn ttl_seconds(&self) -> u64 {
        self.ttl.as_secs()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};

    use super::{TicketError, TicketStore};

    fn principal() -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: "operator".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "Host".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        }
    }

    #[tokio::test]
    async fn ticket_is_consumed_once() {
        let store = TicketStore::new(Duration::from_secs(30), 8);
        let ticket = store
            .issue(principal())
            .await
            .unwrap_or_else(|error| panic!("issue ticket: {error}"));
        assert!(store.consume(&ticket.ticket).await.is_ok());
        assert_eq!(
            store.consume(&ticket.ticket).await,
            Err(TicketError::Invalid)
        );
    }

    #[tokio::test]
    async fn expired_ticket_fails_closed() {
        let store = TicketStore::new(Duration::ZERO, 8);
        let ticket = store
            .issue(principal())
            .await
            .unwrap_or_else(|error| panic!("issue ticket: {error}"));
        assert_eq!(
            store.consume(&ticket.ticket).await,
            Err(TicketError::Invalid)
        );
    }

    #[tokio::test]
    async fn ticket_purposes_are_one_use_and_never_interchangeable() {
        let store = TicketStore::new(Duration::from_secs(30), 8);
        let operator = store
            .issue_server_operator("operator-local-user".to_owned())
            .await
            .unwrap_or_else(|error| panic!("issue operator ticket: {error}"));
        assert_eq!(
            store.consume(&operator.ticket).await,
            Err(TicketError::Invalid)
        );
        assert_eq!(
            store.consume_server_operator(&operator.ticket).await,
            Err(TicketError::Invalid)
        );

        let room = store
            .issue(principal())
            .await
            .unwrap_or_else(|error| panic!("issue room ticket: {error}"));
        assert_eq!(
            store.consume_server_operator(&room.ticket).await,
            Err(TicketError::Invalid)
        );
        assert_eq!(store.consume(&room.ticket).await, Err(TicketError::Invalid));
    }
}
