use agentsassemble_domain::AuthenticatedPrincipal;
use chrono::{DateTime, Utc};

use crate::{
    HumanSessionAuthorization, OperatorSessionAuthorization, PersistenceError,
    RoomMutationAuthority, SqliteStore,
};

/// Room-scoped browser provenance; neither variant grants durable account authority.
#[derive(Clone)]
pub enum RoomSessionAuthorization {
    Human(HumanSessionAuthorization),
    Operator(OperatorSessionAuthorization),
}

impl RoomSessionAuthorization {
    #[must_use]
    pub const fn principal(&self) -> &AuthenticatedPrincipal {
        match self {
            Self::Human(session) => session.principal(),
            Self::Operator(session) => session.principal(),
        }
    }

    #[must_use]
    pub const fn session_fingerprint(&self) -> &[u8; 32] {
        match self {
            Self::Human(session) => session.session_fingerprint(),
            Self::Operator(session) => session.session_fingerprint(),
        }
    }

    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        match self {
            Self::Human(session) => session.expires_at(),
            Self::Operator(session) => session.expires_at(),
        }
    }

    #[must_use]
    pub const fn mutation_authority(&self) -> RoomMutationAuthority<'_> {
        match self {
            Self::Human(session) => RoomMutationAuthority::HumanSession(session),
            Self::Operator(session) => RoomMutationAuthority::OperatorSession(session),
        }
    }
}

impl SqliteStore {
    /// Revalidates exact room-session provenance at a new request or stream boundary.
    ///
    /// # Errors
    /// Rejects changed, expired or revoked authority and propagates storage failures.
    pub async fn revalidate_room_session_authorization(
        &self,
        expected: &RoomSessionAuthorization,
    ) -> Result<RoomSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let current = match expected {
            RoomSessionAuthorization::Human(session) => {
                let (current, _) = crate::human_session_authority::revalidate_human_session(
                    &mut tx,
                    session,
                    Utc::now(),
                )
                .await?;
                RoomSessionAuthorization::Human(current)
            }
            RoomSessionAuthorization::Operator(session) => RoomSessionAuthorization::Operator(
                crate::operator_pairing::revalidate_operator_session(&mut tx, session, Utc::now())
                    .await?,
            ),
        };
        tx.commit().await?;
        Ok(current)
    }
}
