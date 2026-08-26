use agentsassemble_domain::{AuthenticatedPrincipal, InviteScope};
use agentsassemble_persistence::HumanSessionAuthorization;
use chrono::Utc;
use tokio::time::Instant;

use super::{
    ConsumedTicket, IssuedTicket, StoredTicketGrant, TicketAuthority, TicketError, TicketStore,
    insert_grant,
};

pub(super) struct HumanSessionGrant {
    pub(super) authorization: HumanSessionAuthorization,
    pub(super) purpose: HumanSessionGrantPurpose,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HumanSessionGrantPurpose {
    WebSocketConnect,
    OwnProfile,
    PreferencesRead,
    PreferencesWrite,
}

pub struct ConsumedHumanSessionSocketTicket {
    authorization: HumanSessionAuthorization,
    proof_key: String,
    connection_nonce: String,
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
    pub fn into_parts(self) -> (HumanSessionAuthorization, String, String) {
        (self.authorization, self.proof_key, self.connection_nonce)
    }
}

pub enum ConsumedProfileTicket {
    Room(AuthenticatedPrincipal),
    HumanSession(HumanSessionAuthorization),
    ServerOperator { principal_id: String },
}

const PUBLIC_SESSION_GRANT_CAPACITY: usize = 1_792;
const PUBLIC_SESSION_GRANTS_PER_SESSION: usize = 8;
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
        self.issue_human_session(authorization, HumanSessionGrantPurpose::WebSocketConnect)
            .await
    }

    /// Issues an exact own-profile grant from current durable human-session authority.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the session has expired or a global, public, or per-session
    /// grant bound is exhausted.
    pub async fn issue_human_session_profile(
        &self,
        authorization: HumanSessionAuthorization,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_human_session(authorization, HumanSessionGrantPurpose::OwnProfile)
            .await
    }

    /// Issues an exact preference-read grant from current durable human-session authority.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the session has expired or a global, public, or per-session
    /// grant bound is exhausted.
    pub async fn issue_human_session_preferences_read(
        &self,
        authorization: HumanSessionAuthorization,
    ) -> Result<IssuedTicket, TicketError> {
        self.issue_human_session(authorization, HumanSessionGrantPurpose::PreferencesRead)
            .await
    }

    /// Issues an exact preference-write grant from current durable human-session authority.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` when the session has expired or a global, public, or per-session
    /// grant bound is exhausted.
    pub async fn issue_human_session_preferences_write(
        &self,
        authorization: HumanSessionAuthorization,
    ) -> Result<IssuedTicket, TicketError> {
        if authorization.principal().invite_scope != InviteScope::ReadWrite {
            return Err(TicketError::Invalid);
        }
        self.issue_human_session(authorization, HumanSessionGrantPurpose::PreferencesWrite)
            .await
    }

    async fn issue_human_session(
        &self,
        authorization: HumanSessionAuthorization,
        purpose: HumanSessionGrantPurpose,
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
            if let TicketAuthority::HumanSession(public) = &grant.authority {
                public_count += 1;
                if public.authorization.session_fingerprint() == &session_fingerprint {
                    same_session_count += 1;
                }
            }
            true
        });
        let public_capacity = self.public_session_capacity();
        if grants.len() >= self.capacity
            || public_count >= public_capacity
            || same_session_count >= PUBLIC_SESSION_GRANTS_PER_SESSION
        {
            return Err(TicketError::Invalid);
        }
        Ok(insert_grant(
            &mut grants,
            TicketAuthority::HumanSession(HumanSessionGrant {
                authorization,
                purpose,
            }),
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
            TicketAuthority::HumanSession(session)
                if session.purpose == HumanSessionGrantPurpose::WebSocketConnect =>
            {
                Ok(SocketTicketHint::HumanSession {
                    room_id: session.authorization.principal().room_id.clone(),
                })
            }
            TicketAuthority::RoomHttp(_)
            | TicketAuthority::HumanSession(_)
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
        let connection_nonce = crate::server_proof::derive_connection_nonce(ticket);
        match grant.authority {
            TicketAuthority::Room(principal) => Ok(ConsumedSocketTicket::Local(ConsumedTicket {
                principal,
                proof_key: grant.proof_key,
                connection_nonce,
            })),
            TicketAuthority::HumanSession(public) => {
                let authorization = Self::resolve_human_session_authority(
                    public,
                    HumanSessionGrantPurpose::WebSocketConnect,
                    Utc::now(),
                )?;
                Ok(ConsumedSocketTicket::HumanSession(
                    ConsumedHumanSessionSocketTicket {
                        authorization,
                        proof_key: grant.proof_key,
                        connection_nonce,
                    },
                ))
            }
            TicketAuthority::RoomHttp(_)
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
        let grant = self
            .consume_human_session(ticket, HumanSessionGrantPurpose::WebSocketConnect)
            .await?;
        Ok(ConsumedHumanSessionSocketTicket {
            authorization: grant.authorization,
            proof_key: grant.proof_key,
            connection_nonce: crate::server_proof::derive_connection_nonce(ticket),
        })
    }

    /// Consumes only an exact human-session own-profile credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_human_session_profile(
        &self,
        ticket: &str,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        Ok(self
            .consume_human_session(ticket, HumanSessionGrantPurpose::OwnProfile)
            .await?
            .authorization)
    }

    /// Consumes only an exact human-session preference-read credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_human_session_preferences_read(
        &self,
        ticket: &str,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        Ok(self
            .consume_human_session(ticket, HumanSessionGrantPurpose::PreferencesRead)
            .await?
            .authorization)
    }

    /// Consumes only an exact human-session preference-write credential.
    ///
    /// # Errors
    ///
    /// Returns `Invalid` after consuming a wrong-purpose, expired, unknown, or reused ticket.
    pub async fn consume_human_session_preferences_write(
        &self,
        ticket: &str,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        Ok(self
            .consume_human_session(ticket, HumanSessionGrantPurpose::PreferencesWrite)
            .await?
            .authorization)
    }

    async fn consume_human_session(
        &self,
        ticket: &str,
        expected: HumanSessionGrantPurpose,
    ) -> Result<ConsumedHumanSessionGrant, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        Self::resolve_human_session_at(grant, expected, Utc::now())
    }

    fn resolve_human_session_at(
        grant: StoredTicketGrant,
        expected: HumanSessionGrantPurpose,
        now: chrono::DateTime<Utc>,
    ) -> Result<ConsumedHumanSessionGrant, TicketError> {
        let TicketAuthority::HumanSession(public) = grant.authority else {
            return Err(TicketError::Invalid);
        };
        let authorization = Self::resolve_human_session_authority(public, expected, now)?;
        Ok(ConsumedHumanSessionGrant {
            authorization,
            proof_key: grant.proof_key,
        })
    }

    fn resolve_human_session_authority(
        public: HumanSessionGrant,
        expected: HumanSessionGrantPurpose,
        now: chrono::DateTime<Utc>,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        if public.purpose != expected || public.authorization.expires_at() <= now {
            return Err(TicketError::Invalid);
        }
        Ok(public.authorization)
    }

    #[cfg(test)]
    pub(crate) async fn consume_human_session_profile_at(
        &self,
        ticket: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<HumanSessionAuthorization, TicketError> {
        let grant = self.consume_grant(ticket).await?;
        Ok(
            Self::resolve_human_session_at(grant, HumanSessionGrantPurpose::OwnProfile, now)?
                .authorization,
        )
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
            TicketAuthority::HumanSession(public) => {
                ConsumedProfileTicket::HumanSession(Self::resolve_human_session_authority(
                    public,
                    HumanSessionGrantPurpose::OwnProfile,
                    Utc::now(),
                )?)
            }
            TicketAuthority::ServerOperator { principal_id } => {
                ConsumedProfileTicket::ServerOperator { principal_id }
            }
            TicketAuthority::RoomHttp(_)
            | TicketAuthority::SettingsDirectoryRead { .. }
            | TicketAuthority::CentralRegistration { .. } => return Err(TicketError::Invalid),
        })
    }

    pub(crate) fn public_session_capacity(&self) -> usize {
        self.capacity
            .saturating_sub(LOCAL_PRIVATE_GRANT_RESERVE)
            .min(PUBLIC_SESSION_GRANT_CAPACITY)
    }
}

struct ConsumedHumanSessionGrant {
    authorization: HumanSessionAuthorization,
    proof_key: String,
}
