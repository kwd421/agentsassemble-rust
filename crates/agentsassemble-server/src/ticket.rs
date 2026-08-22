use std::{collections::HashMap, sync::Arc, time::Duration};

use agentsassemble_domain::AuthenticatedPrincipal;
use thiserror::Error;
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct StoredTicketGrant {
    principal: AuthenticatedPrincipal,
    proof_key: String,
    expires_at: Instant,
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
                principal,
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
        let grant = self
            .grants
            .lock()
            .await
            .remove(ticket)
            .ok_or(TicketError::Invalid)?;
        if grant.expires_at <= Instant::now() {
            return Err(TicketError::Invalid);
        }
        Ok(ConsumedTicket {
            principal: grant.principal,
            proof_key: grant.proof_key,
        })
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
}
