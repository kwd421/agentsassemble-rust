use agentsassemble_persistence::{
    OPERATOR_SESSION_BEARER_PREFIX, PersistenceError, RoomSessionAuthorization,
};
use axum::http::HeaderMap;

use crate::{
    AppState,
    human_session_http_authority::{
        HumanSessionBearerError, HumanSessionBearerResolution, resolve_human_session_bearer,
    },
    ingress_trust::TrustedIngressOrigin,
    operator_pairing_web::{device_fingerprint, fingerprint_token},
};

pub(crate) enum RoomSessionBearerResolution {
    Other,
    Authorized(Box<RoomSessionAuthorization>),
}

pub(crate) enum RoomSessionBearerError {
    Invalid,
    Persistence(PersistenceError),
}

pub(crate) async fn resolve_room_session_bearer(
    state: &AppState,
    headers: &HeaderMap,
    origin: Option<&TrustedIngressOrigin>,
    bearer: &str,
) -> Result<RoomSessionBearerResolution, RoomSessionBearerError> {
    // Credential domain selects one authority owner. A malformed or revoked paired
    // bearer cannot enter human-session or private-ticket resolution.
    if bearer.starts_with(OPERATOR_SESSION_BEARER_PREFIX) {
        let fingerprint = fingerprint_token(bearer, OPERATOR_SESSION_BEARER_PREFIX)
            .ok_or(RoomSessionBearerError::Invalid)?;
        let device = device_fingerprint(headers).ok_or(RoomSessionBearerError::Invalid)?;
        let origin = origin.ok_or(RoomSessionBearerError::Invalid)?.as_str();
        let ready = state
            .public_ingress
            .ready_snapshot()
            .ok_or(RoomSessionBearerError::Invalid)?;
        if origin != ready.public_url {
            return Err(RoomSessionBearerError::Invalid);
        }
        let session = state
            .store
            .authorize_operator_session(&fingerprint, &device, origin)
            .await
            .map_err(RoomSessionBearerError::Persistence)?;
        return Ok(RoomSessionBearerResolution::Authorized(Box::new(
            RoomSessionAuthorization::Operator(session),
        )));
    }
    match resolve_human_session_bearer(state, bearer).await {
        Ok(HumanSessionBearerResolution::Other) => Ok(RoomSessionBearerResolution::Other),
        Ok(HumanSessionBearerResolution::Authorized(session)) => {
            Ok(RoomSessionBearerResolution::Authorized(Box::new(
                RoomSessionAuthorization::Human(session),
            )))
        }
        Err(HumanSessionBearerError::Invalid) => Err(RoomSessionBearerError::Invalid),
        Err(HumanSessionBearerError::Persistence(error)) => {
            Err(RoomSessionBearerError::Persistence(error))
        }
    }
}
