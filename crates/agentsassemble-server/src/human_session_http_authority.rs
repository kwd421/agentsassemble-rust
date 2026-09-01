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
) -> Result<HumanSessionBearerResolution, HumanSessionBearerError> {
    let fingerprint = match classify_presented_bearer(bearer) {
        PresentedHumanSessionBearer::Other => return Ok(HumanSessionBearerResolution::Other),
        PresentedHumanSessionBearer::Invalid => return Err(HumanSessionBearerError::Invalid),
        PresentedHumanSessionBearer::Fingerprint(fingerprint) => fingerprint,
    };
    state
        .store
        .authorize_human_session(&fingerprint)
        .await
        .map(HumanSessionBearerResolution::Authorized)
        .map_err(HumanSessionBearerError::Persistence)
}
