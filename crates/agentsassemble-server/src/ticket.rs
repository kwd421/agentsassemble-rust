use std::{collections::HashMap, sync::Arc, time::Duration};

use agentsassemble_domain::AuthenticatedPrincipal;
use thiserror::Error;
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

mod human_session;

use human_session::HumanSessionGrant;
pub use human_session::{ConsumedHumanSessionSocketTicket, ConsumedProfileTicket};
pub(crate) use human_session::{ConsumedSocketTicket, SocketTicketHint};

struct StoredTicketGrant {
    authority: TicketAuthority,
    proof_key: String,
    expires_at: Instant,
}

enum TicketAuthority {
    Room(AuthenticatedPrincipal),
    RoomHttp(RoomHttpGrant),
    HumanSession(HumanSessionGrant),
    SettingsDirectoryRead { principal_id: String },
    ServerOperator { principal_id: String },
    CentralRegistration { principal_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomHttpGrant {
    room_id: String,
    principal_id: String,
    participant_id: String,
    purpose: RoomHttpPurpose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomHttpPurpose {
    PreferencesRead,
    PreferencesWrite,
    AppearanceUpload,
    PendingPreviewRead { asset_id: String },
    BoundAppearanceRead { asset_id: String },
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
pub struct ConsumedRoomHttpTicket {
    pub room_id: String,
    pub principal_id: String,
    pub participant_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedSettingsDirectoryReadTicket {
    pub principal_id: String,
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

    /// Issues one exact preference-read credential for a resolved room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_preferences_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::PreferencesRead,
        )
        .await
    }

    /// Issues one exact preference-write credential for a resolved room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_preferences_write(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::PreferencesWrite,
        )
        .await
    }

    /// Issues one exact room-appearance upload credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_appearance_upload(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::AppearanceUpload,
        )
        .await
    }

    /// Issues one pending-preview credential bound to an exact asset.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity or asset fields, or exhausted capacity.
    pub async fn issue_pending_preview_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
        asset_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if asset_id.is_empty() {
            return Err(TicketError::Invalid);
        }
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::PendingPreviewRead { asset_id },
        )
        .await
    }

    /// Issues one bound-appearance credential bound to an exact asset.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity or asset fields, or exhausted capacity.
    pub async fn issue_bound_appearance_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
        asset_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if asset_id.is_empty() {
            return Err(TicketError::Invalid);
        }
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::BoundAppearanceRead { asset_id },
        )
        .await
    }

    /// Issues the server-wide local-operator settings-directory read credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for an empty principal or exhausted ticket capacity.
    pub async fn issue_settings_directory_read(
        &self,
        principal_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if principal_id.is_empty() {
            return Err(TicketError::Invalid);
        }
        self.issue_authority(TicketAuthority::SettingsDirectoryRead { principal_id })
            .await
    }

    async fn issue_room_http(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
        purpose: RoomHttpPurpose,
    ) -> Result<IssuedTicket, TicketError> {
        if room_id.is_empty() || principal_id.is_empty() || participant_id.is_empty() {
            return Err(TicketError::Invalid);
        }
        self.issue_authority(TicketAuthority::RoomHttp(RoomHttpGrant {
            room_id,
            principal_id,
            participant_id,
            purpose,
        }))
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
        Ok(insert_grant(&mut grants, authority, now + self.ttl))
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

    /// Consumes only an exact preference-read credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_preferences_read(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, RoomHttpPurpose::PreferencesRead)
            .await
    }

    /// Consumes only an exact preference-write credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_preferences_write(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, RoomHttpPurpose::PreferencesWrite)
            .await
    }

    /// Consumes only an exact room-appearance upload credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_appearance_upload(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, RoomHttpPurpose::AppearanceUpload)
            .await
    }

    /// Consumes only a pending-preview credential for the requested asset.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a mismatched, expired, unknown, or reused ticket.
    pub async fn consume_pending_preview_read(
        &self,
        ticket: &str,
        asset_id: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(
            ticket,
            RoomHttpPurpose::PendingPreviewRead {
                asset_id: asset_id.to_owned(),
            },
        )
        .await
    }

    /// Consumes only a bound-appearance credential for the requested asset.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a mismatched, expired, unknown, or reused ticket.
    pub async fn consume_bound_appearance_read(
        &self,
        ticket: &str,
        asset_id: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(
            ticket,
            RoomHttpPurpose::BoundAppearanceRead {
                asset_id: asset_id.to_owned(),
            },
        )
        .await
    }

    /// Consumes only the server-wide settings-directory read credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_settings_directory_read(
        &self,
        ticket: &str,
    ) -> Result<ConsumedSettingsDirectoryReadTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::SettingsDirectoryRead { principal_id } = grant.authority else {
            return Err(TicketError::Invalid);
        };
        Ok(ConsumedSettingsDirectoryReadTicket { principal_id })
    }

    async fn consume_room_http(
        &self,
        ticket: &str,
        expected: RoomHttpPurpose,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::RoomHttp(room) = grant.authority else {
            return Err(TicketError::Invalid);
        };
        if room.purpose != expected {
            return Err(TicketError::Invalid);
        }
        Ok(ConsumedRoomHttpTicket {
            room_id: room.room_id,
            principal_id: room.principal_id,
            participant_id: room.participant_id,
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

fn insert_grant(
    grants: &mut HashMap<String, StoredTicketGrant>,
    authority: TicketAuthority,
    expires_at: Instant,
) -> IssuedTicket {
    let ticket = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let proof_key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    grants.insert(
        ticket.clone(),
        StoredTicketGrant {
            authority,
            proof_key: proof_key.clone(),
            expires_at,
        },
    );
    IssuedTicket { ticket, proof_key }
}
