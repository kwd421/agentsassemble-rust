use agentsassemble_persistence::{HumanSessionAuthorization, PersistenceError};

use crate::{
    AppState,
    human_session_bearer::{PresentedHumanSessionBearer, classify_presented_bearer},
};

pub(crate) enum HumanSessionBearerResolution {
    Other,
    Authorized(HumanSessionAuthorization),
}

pub(crate) enum HumanSessionBearerError {
    Invalid,
    Persistence(PersistenceError),
}

pub(crate) async fn resolve_human_session_bearer(
    state: &AppState,
    bearer: &str,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<HumanSessionBearerResolution, HumanSessionBearerError> {
    let fingerprint = match classify_presented_bearer(bearer) {
        PresentedHumanSessionBearer::Other => return Ok(HumanSessionBearerResolution::Other),
        PresentedHumanSessionBearer::Invalid => return Err(HumanSessionBearerError::Invalid),
        PresentedHumanSessionBearer::Fingerprint(fingerprint) => fingerprint,
    };
    let session = state
        .store
        .authorize_human_session(&fingerprint)
        .await
        .map_err(HumanSessionBearerError::Persistence)?;
    state
        .store
        .require_secure_human_transport(
            &fingerprint,
            origin.and_then(|value| value.secure.as_ref()),
        )
        .await
        .map_err(HumanSessionBearerError::Persistence)?;
    Ok(HumanSessionBearerResolution::Authorized(session))
}
