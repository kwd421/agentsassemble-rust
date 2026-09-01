use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{HumanSessionAuthorization, LocalRoomManagerAuthority};
use chrono::Utc;
use tokio::time::Instant;

use super::{
    ConsumedTicket, IssuedTicket, LocalRoomManagerPurpose, StoredTicketGrant, TicketAuthority,
    TicketError, TicketStore, insert_grant, resolve_local_room_manager_authority,
};

pub(super) struct HumanSessionSocketGrant {
    pub(super) authorization: HumanSessionAuthorization,
}

pub struct ConsumedHumanSessionSocketTicket {
    authorization: HumanSessionAuthorization,
}

pub(crate) enum SocketTicketHint {
    Local,
    HumanSession { room_id: String },
}

pub(crate) enum ConsumedSocketTicket {
    Local(ConsumedTicket),
    HumanSession(ConsumedHumanSessionSocketTicket),
}

impl ConsumedSocketTicket {
    #[must_use]
    pub(crate) fn principal(&self) -> &AuthenticatedPrincipal {
        match self {
            Self::Local(grant) => &grant.principal,
            Self::HumanSession(grant) => grant.authorization.principal(),
        }
    }
}

impl ConsumedHumanSessionSocketTicket {
    #[must_use]
    pub fn into_authorization(self) -> HumanSessionAuthorization {
        self.authorization
    }
}

pub enum ConsumedProfileTicket {
    ServerOperator { principal_id: String },
}

pub(crate) enum ConsumedAttachmentUploadTicket {
    Profile(ConsumedProfileTicket),
    Appearance(LocalRoomManagerAuthority),
}

const PUBLIC_SOCKET_TICKET_CAPACITY: usize = 1_792;
const PUBLIC_SOCKET_TICKETS_PER_SESSION: usize = 8;
const LOCAL_PRIVATE_GRANT_RESERVE: usize = 2_304;

impl TicketStore {
    /// Issues an exact WebSocket-connect grant from current durable human-session authority.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the session has expired or a global, public, or per-session
    /// grant bound is exhausted.
    pub async fn issue_human_session_socket(
        &self,
        authorization: HumanSessionAuthorization,
    ) -> Result<IssuedTicket, TicketError> {
        let session_fingerprint = *authorization.session_fingerprint();
        let mut grants = self.grants.lock().await;
        let now = Instant::now();
        let session_remaining = authorization
            .expires_at()
            .signed_duration_since(Utc::now())
            .to_std()
            .map_err(|_| TicketError::Invalid)?;
        let mut public_count = 0;
        let mut same_session_count = 0;
        grants.retain(|_, grant| {
            if grant.expires_at <= now {
                return false;
            }
            if let TicketAuthority::HumanSessionSocket(public) = &grant.authority {
                public_count += 1;
                if public.authorization.session_fingerprint() == &session_fingerprint {
                    same_session_count += 1;
                }
            }
            true
        });
        let public_capacity = self.public_socket_capacity();
        if grants.len() >= self.capacity
            || public_count >= public_capacity
            || same_session_count >= PUBLIC_SOCKET_TICKETS_PER_SESSION
        {
            return Err(TicketError::Invalid);
        }
        Ok(insert_grant(
            &mut grants,
            TicketAuthority::HumanSessionSocket(HumanSessionSocketGrant { authorization }),
            now + self.ttl.min(session_remaining),
        ))
    }

    /// Inspects only enough socket authority to subscribe to human-session revocation before
    /// consuming the one-use grant. Wrong-purpose and expired grants are consumed immediately.
    pub(crate) async fn socket_ticket_hint(
        &self,
        ticket: &str,
    ) -> Result<SocketTicketHint, TicketError> {
        let now = Instant::now();
        let mut grants = self.grants.lock().await;
        let Some(grant) = grants.get(ticket) else {
            return Err(TicketError::Invalid);
        };
        if grant.expires_at <= now {
            grants.remove(ticket);
            return Err(TicketError::Invalid);
        }
        match &grant.authority {
            TicketAuthority::Room(_) => Ok(SocketTicketHint::Local),
            TicketAuthority::HumanSessionSocket(session) => Ok(SocketTicketHint::HumanSession {
                room_id: session.authorization.principal().room_id.clone(),
            }),
            TicketAuthority::RoomHttp(_)
            | TicketAuthority::LocalRoomManager(_)
            | TicketAuthority::SettingsDirectoryRead { .. }
            | TicketAuthority::ServerOperator { .. }
            | TicketAuthority::CentralRegistration { .. } => {
                grants.remove(ticket);
                Err(TicketError::Invalid)
            }
        }
    }

    /// Removes and resolves either current socket credential exactly once without trying one
    /// authority kind and falling back to another.
    pub(crate) async fn consume_socket(
        &self,
        ticket: &str,
    ) -> Result<ConsumedSocketTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        match grant.authority {
            TicketAuthority::Room(principal) => {
                Ok(ConsumedSocketTicket::Local(ConsumedTicket { principal }))
            }
            TicketAuthority::HumanSessionSocket(public) => {
                let authorization =
                    Self::resolve_human_session_socket_authority(public, Utc::now())?;
                Ok(ConsumedSocketTicket::HumanSession(
                    ConsumedHumanSessionSocketTicket { authorization },
                ))
            }
            TicketAuthority::RoomHttp(_)
            | TicketAuthority::LocalRoomManager(_)
            | TicketAuthority::SettingsDirectoryRead { .. }
            | TicketAuthority::ServerOperator { .. }
            | TicketAuthority::CentralRegistration { .. } => Err(TicketError::Invalid),
        }
    }

    /// Consumes only an exact human-session WebSocket credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_human_session_socket(
        &self,
        ticket: &str,
    ) -> Result<ConsumedHumanSessionSocketTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        let authorization = Self::resolve_human_session_at(grant, Utc::now())?;
        Ok(ConsumedHumanSessionSocketTicket { authorization })
    }

    #[cfg(test)]
    pub(crate) async fn consume_human_session_socket_at(
        &self,
        ticket: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        Self::resolve_human_session_at(grant, now)
    }

    fn resolve_human_session_at(
        grant: StoredTicketGrant,
        now: chrono::DateTime<Utc>,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        let TicketAuthority::HumanSessionSocket(public) = grant.authority else {
            return Err(TicketError::Invalid);
        };
        Self::resolve_human_session_socket_authority(public, now)
    }

    fn resolve_human_session_socket_authority(
        public: HumanSessionSocketGrant,
        now: chrono::DateTime<Utc>,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        if public.authorization.expires_at() <= now {
            return Err(TicketError::Invalid);
        }
        Ok(public.authorization)
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
        match Self::resolve_attachment_upload_authority(grant.authority)? {
            ConsumedAttachmentUploadTicket::Profile(profile) => Ok(profile),
            ConsumedAttachmentUploadTicket::Appearance(_) => Err(TicketError::Invalid),
        }
    }

    /// Consumes one authenticated attachment-upload credential and dispatches its exact purpose.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub(crate) async fn consume_attachment_upload(
        &self,
        ticket: &str,
    ) -> Result<ConsumedAttachmentUploadTicket, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        Self::resolve_attachment_upload_authority(grant.authority)
    }

    fn resolve_attachment_upload_authority(
        authority: TicketAuthority,
    ) -> Result<ConsumedAttachmentUploadTicket, TicketError> {
        Ok(match authority {
            TicketAuthority::ServerOperator { principal_id, .. } => {
                ConsumedAttachmentUploadTicket::Profile(ConsumedProfileTicket::ServerOperator {
                    principal_id,
                })
            }
            TicketAuthority::LocalRoomManager(manager) => {
                ConsumedAttachmentUploadTicket::Appearance(resolve_local_room_manager_authority(
                    manager,
                    &LocalRoomManagerPurpose::AppearanceUpload,
                )?)
            }
            TicketAuthority::Room(_)
            | TicketAuthority::HumanSessionSocket(_)
            | TicketAuthority::RoomHttp(_)
            | TicketAuthority::SettingsDirectoryRead { .. }
            | TicketAuthority::CentralRegistration { .. } => return Err(TicketError::Invalid),
        })
    }

    pub(crate) fn public_socket_capacity(&self) -> usize {
        self.capacity
            .saturating_sub(LOCAL_PRIVATE_GRANT_RESERVE)
            .min(PUBLIC_SOCKET_TICKET_CAPACITY)
    }
}
