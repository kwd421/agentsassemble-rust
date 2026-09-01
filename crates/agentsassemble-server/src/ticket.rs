use std::{
    collections::HashMap,
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use agentsassemble_domain::{AuthenticatedPrincipal, is_room_appearance_asset_id};
use agentsassemble_persistence::{HumanSessionAuthorization, LocalRoomManagerAuthority};
use chrono::Utc;
use thiserror::Error;
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

mod human_session;
mod message_attachments;

pub(crate) use human_session::{
    ConsumedAttachmentUploadTicket, ConsumedSocketTicket, SocketTicketHint,
};
pub use human_session::{ConsumedHumanSessionSocketTicket, ConsumedProfileTicket};
use human_session::{HumanSessionGrant, HumanSessionGrantPurpose};
pub(crate) use message_attachments::ConsumedMessageAttachmentReadTicket;

struct StoredTicketGrant {
    authority: TicketAuthority,
    expires_at: Instant,
}

enum TicketAuthority {
    Room(AuthenticatedPrincipal),
    RoomHttp(RoomHttpGrant),
    LocalRoomManager(LocalRoomManagerGrant),
    HumanSession(HumanSessionGrant),
    SettingsDirectoryRead {
        principal_id: String,
    },
    ServerOperator {
        principal_id: String,
        issue_sequence: NonZeroU64,
    },
    CentralRegistration {
        principal_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomHttpGrant {
    room_id: String,
    principal_id: String,
    participant_id: String,
    purpose: RoomHttpPurpose,
}

pub(super) struct LocalRoomManagerGrant {
    authority: LocalRoomManagerAuthority,
    purpose: LocalRoomManagerPurpose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalRoomManagerPurpose {
    HumanInviteCreate,
    HumanInviteRevoke,
    AppearanceUpload,
    PendingPreviewRead { asset_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomHttpPurpose {
    PreferencesRead,
    PreferencesWrite,
    MessagePinsRead,
    MessagePinsWrite,
    MessageSearchRead,
    MessageAttachmentUpload,
    BoundMessageAttachmentRead { attachment_id: String },
    BoundAppearanceRead { asset_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedTicket {
    pub ticket: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedTicket {
    pub principal: AuthenticatedPrincipal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedServerOperatorTicket {
    pub principal_id: String,
    pub issue_sequence: NonZeroU64,
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

pub(crate) struct ConsumedHumanInviteManagerTicket {
    pub authority: LocalRoomManagerAuthority,
}

pub(crate) enum RoomHumanHttpAuthority {
    LocalTicket(ConsumedRoomHttpTicket),
    HumanSession(HumanSessionAuthorization),
}

pub(crate) enum ConsumedAppearanceReadTicket {
    Pending(LocalRoomManagerAuthority),
    Bound(ConsumedRoomHttpTicket),
    HumanSession(HumanSessionAuthorization),
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
    next_server_operator_sequence: Arc<AtomicU64>,
    ttl: Duration,
    capacity: usize,
}

impl TicketStore {
    #[must_use]
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            grants: Arc::new(Mutex::new(HashMap::new())),
            next_server_operator_sequence: Arc::new(AtomicU64::new(0)),
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
        let previous = self
            .next_server_operator_sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| TicketError::Invalid)?;
        let issue_sequence = previous
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(TicketError::Invalid)?;
        self.issue_authority(TicketAuthority::ServerOperator {
            principal_id,
            issue_sequence,
        })
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

    /// Issues one exact message-pin read credential for a resolved room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_message_pins_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::MessagePinsRead,
        )
        .await
    }

    /// Issues one exact message-pin write credential for a resolved room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_message_pins_write(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::MessagePinsWrite,
        )
        .await
    }

    /// Issues one exact lobby-message-search credential for a resolved room human.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_message_search_read(
        &self,
        room_id: String,
        principal_id: String,
        participant_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_room_http(
            room_id,
            principal_id,
            participant_id,
            RoomHttpPurpose::MessageSearchRead,
        )
        .await
    }

    /// Issues one exact human-invite creation credential for a resolved room manager.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_human_invite_create(
        &self,
        authority: LocalRoomManagerAuthority,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_local_room_manager(authority, LocalRoomManagerPurpose::HumanInviteCreate)
            .await
    }

    /// Issues one exact human-invite revocation credential for a resolved room manager.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_human_invite_revoke(
        &self,
        authority: LocalRoomManagerAuthority,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_local_room_manager(authority, LocalRoomManagerPurpose::HumanInviteRevoke)
            .await
    }

    async fn issue_local_room_manager(
        &self,
        authority: LocalRoomManagerAuthority,
        purpose: LocalRoomManagerPurpose,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_authority(TicketAuthority::LocalRoomManager(LocalRoomManagerGrant {
            authority,
            purpose,
        }))
        .await
    }

    /// Issues one exact room-appearance upload credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity fields or exhausted ticket capacity.
    pub async fn issue_appearance_upload(
        &self,
        authority: LocalRoomManagerAuthority,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_local_room_manager(authority, LocalRoomManagerPurpose::AppearanceUpload)
            .await
    }

    /// Issues one pending-preview credential bound to an exact asset.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` for empty identity or asset fields, or exhausted capacity.
    pub async fn issue_pending_preview_read(
        &self,
        authority: LocalRoomManagerAuthority,
        asset_id: String,
    ) -> Result<IssuedTicket, TicketError> {
        if !is_room_appearance_asset_id(&asset_id) {
            return Err(TicketError::Invalid);
        }
        self.issue_local_room_manager(
            authority,
            LocalRoomManagerPurpose::PendingPreviewRead { asset_id },
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
        if !is_room_appearance_asset_id(&asset_id) {
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
        Ok(ConsumedTicket { principal })
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
        let TicketAuthority::ServerOperator {
            principal_id,
            issue_sequence,
        } = grant.authority
        else {
            return Err(TicketError::Invalid);
        };
        Ok(ConsumedServerOperatorTicket {
            principal_id,
            issue_sequence,
        })
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
    pub(crate) async fn consume_preferences_read(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::PreferencesRead)
            .await
    }

    /// Consumes only an exact preference-write credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_preferences_write(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::PreferencesWrite)
            .await
    }

    /// Consumes only an exact message-pin read credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_message_pins_read(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::MessagePinsRead)
            .await
    }

    /// Consumes only an exact message-pin write credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_message_pins_write(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::MessagePinsWrite)
            .await
    }

    /// Consumes only an exact lobby-message-search credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_message_search_read(
        &self,
        ticket: &str,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        self.consume_room_http(ticket, &RoomHttpPurpose::MessageSearchRead)
            .await
    }

    /// Consumes only an exact human-invite creation credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_human_invite_create(
        &self,
        ticket: &str,
    ) -> Result<ConsumedHumanInviteManagerTicket, TicketError> {
        self.consume_local_room_manager(ticket, &LocalRoomManagerPurpose::HumanInviteCreate)
            .await
    }

    /// Consumes only an exact human-invite revocation credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_human_invite_revoke(
        &self,
        ticket: &str,
    ) -> Result<ConsumedHumanInviteManagerTicket, TicketError> {
        self.consume_local_room_manager(ticket, &LocalRoomManagerPurpose::HumanInviteRevoke)
            .await
    }

    /// Consumes one exact appearance read credential without trying a second purpose.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a mismatched, expired, unknown, or reused ticket.
    pub(crate) async fn consume_appearance_read(
        &self,
        ticket: &str,
        asset_id: &str,
    ) -> Result<ConsumedAppearanceReadTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        match grant.authority {
            TicketAuthority::LocalRoomManager(manager) => resolve_local_room_manager_authority(
                manager,
                &LocalRoomManagerPurpose::PendingPreviewRead {
                    asset_id: asset_id.to_owned(),
                },
            )
            .map(ConsumedAppearanceReadTicket::Pending),
            TicketAuthority::RoomHttp(room) => match room.purpose.clone() {
                RoomHttpPurpose::BoundAppearanceRead { asset_id: expected }
                    if expected == asset_id =>
                {
                    resolve_room_http_authority(
                        room,
                        &RoomHttpPurpose::BoundAppearanceRead { asset_id: expected },
                    )
                    .map(ConsumedAppearanceReadTicket::Bound)
                }
                _ => Err(TicketError::Invalid),
            },
            TicketAuthority::HumanSession(session) => Self::resolve_human_session_authority(
                session,
                &HumanSessionGrantPurpose::BoundAppearanceRead {
                    asset_id: asset_id.to_owned(),
                },
                Utc::now(),
            )
            .map(ConsumedAppearanceReadTicket::HumanSession),
            TicketAuthority::Room(_)
            | TicketAuthority::SettingsDirectoryRead { .. }
            | TicketAuthority::ServerOperator { .. }
            | TicketAuthority::CentralRegistration { .. } => Err(TicketError::Invalid),
        }
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

    async fn consume_local_room_manager(
        &self,
        ticket: &str,
        expected: &LocalRoomManagerPurpose,
    ) -> Result<ConsumedHumanInviteManagerTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::LocalRoomManager(manager) = grant.authority else {
            return Err(TicketError::Invalid);
        };
        Ok(ConsumedHumanInviteManagerTicket {
            authority: resolve_local_room_manager_authority(manager, expected)?,
        })
    }

    async fn consume_room_http(
        &self,
        ticket: &str,
        expected: &RoomHttpPurpose,
    ) -> Result<ConsumedRoomHttpTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let TicketAuthority::RoomHttp(room) = grant.authority else {
            return Err(TicketError::Invalid);
        };
        resolve_room_http_authority(room, expected)
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

pub(super) fn resolve_local_room_manager_authority(
    manager: LocalRoomManagerGrant,
    expected: &LocalRoomManagerPurpose,
) -> Result<LocalRoomManagerAuthority, TicketError> {
    if &manager.purpose != expected {
        return Err(TicketError::Invalid);
    }
    Ok(manager.authority)
}

fn resolve_room_http_authority(
    room: RoomHttpGrant,
    expected: &RoomHttpPurpose,
) -> Result<ConsumedRoomHttpTicket, TicketError> {
    if &room.purpose != expected {
        return Err(TicketError::Invalid);
    }
    Ok(ConsumedRoomHttpTicket {
        room_id: room.room_id,
        principal_id: room.principal_id,
        participant_id: room.participant_id,
    })
}

fn insert_grant(
    grants: &mut HashMap<String, StoredTicketGrant>,
    authority: TicketAuthority,
    expires_at: Instant,
) -> IssuedTicket {
    let ticket = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    grants.insert(
        ticket.clone(),
        StoredTicketGrant {
            authority,
            expires_at,
        },
    );
    IssuedTicket { ticket }
}
