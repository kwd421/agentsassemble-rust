use crate::{
    AppState,
    http_api::{BodyDecodeError, PRIVATE_NO_STORE, bearer_credential, decode_json_body},
    room_command_result::CommandFailure,
};
use agentsassemble_persistence::{
    CONNECTOR_INVITE_PREFIX, CONNECTOR_SESSION_PREFIX, PersistenceError,
};
use agentsassemble_protocol::{CommandResolution, RoomAction};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

#[path = "connector_read_web.rs"]
mod read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinRequest {
    request_id: Uuid,
    client_secret: String,
    display_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    request_id: String,
    action: RoomAction,
    payload: Value,
}

registered_routes! {
    fn connector_routes<AppState>() {
        same_origin_public "/api/room-connector/read" => get(read::snapshot),
        same_origin_public "/api/room-connector/wait" => get(read::wait),
        same_origin_public "/api/room-connector/search" => get(read::search),
        same_origin_public "/api/room-connector/context" => get(read::context),
        same_origin_public "/api/room-connector/vote" => get(read::vote),
        same_origin_public "/api/room-connector/join" => post(join),
        same_origin_public "/api/room-connector/command" => post(command),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    connector_routes().layer(SetResponseHeaderLayer::overriding(
        CACHE_CONTROL,
        PRIVATE_NO_STORE.clone(),
    ))
}

async fn join(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let invite = credential(&request, CONNECTOR_INVITE_PREFIX)?;
    let body: JoinRequest = decode_json_body(request, 8192).await?;
    let decoded = URL_SAFE_NO_PAD
        .decode(&body.client_secret)
        .map_err(|_| ConnectorHttpError::invalid())?;
    if decoded.len() != 32 {
        return Err(ConnectorHttpError::invalid());
    }
    let admitted = state
        .rooms
        .admit_connector(
            invite,
            Sha256::digest(body.client_secret.as_bytes()).into(),
            body.request_id,
            body.display_name,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let authority = &admitted.authorization;
    Ok(Json(
        json!({"session_bearer":admitted.session_bearer,"room_id":authority.principal().room_id,"room_uid":authority.room_uid(),"participant_id":authority.principal().participant_id,"expires_at":authority.expires_at(),"deduplicated":admitted.deduplicated}),
    ))
}

async fn command(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = credential(&request, CONNECTOR_SESSION_PREFIX)?;
    let authority = state
        .store
        .authorize_connector_session(&fingerprint, chrono::Utc::now())
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let body: CommandRequest = decode_json_body(request, 65536).await?;
    let outcome = state
        .rooms
        .execute_connector(&authority, body.request_id, body.action, body.payload)
        .await
        .map_err(|failure| ConnectorHttpError::from_failure(&failure))?;
    Ok(Json(
        json!({"resolution":"committed","result":outcome.result,"deduplicated":outcome.deduplicated}),
    ))
}

fn credential(request: &Request, prefix: &str) -> Result<[u8; 32], ConnectorHttpError> {
    let value =
        bearer_credential(request.headers()).ok_or_else(ConnectorHttpError::unauthorized)?;
    if value.len() != prefix.len() + 43 || !value.starts_with(prefix) {
        return Err(ConnectorHttpError::unauthorized());
    }
    // The credential owner matches the exact stored fingerprint; decoding it adds no authority.
    Ok(Sha256::digest(value.as_bytes()).into())
}

pub(crate) struct ConnectorHttpError {
    status: StatusCode,
    code: &'static str,
    resolution: CommandResolution,
}

impl ConnectorHttpError {
    pub(crate) fn invalid() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_connector_request",
            resolution: CommandResolution::Rejected,
        }
    }
    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "connector_credential_required",
            resolution: CommandResolution::Rejected,
        }
    }
    pub(crate) fn from_persistence(error: PersistenceError) -> Self {
        Self::from_failure(&CommandFailure::transactional(error))
    }
    fn from_failure(failure: &CommandFailure) -> Self {
        let (status, code) = match failure.error {
            PersistenceError::InvalidCursor { .. }
            | PersistenceError::SubscriptionCatchUpExceeded { .. }
            | PersistenceError::SubscriptionSequenceGap { .. } => {
                (StatusCode::CONFLICT, "connector_resync_required")
            }
            PersistenceError::CommandConflict => (StatusCode::CONFLICT, "command_conflict"),
            PersistenceError::CommandRejected { code, .. } => (StatusCode::FORBIDDEN, code),
            PersistenceError::RoomMissing | PersistenceError::ParticipantMissing => {
                (StatusCode::GONE, "room_membership_unavailable")
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "connector_operation_failed",
            ),
        };
        Self {
            status,
            code,
            resolution: failure.resolution,
        }
    }
}

impl From<BodyDecodeError> for ConnectorHttpError {
    fn from(error: BodyDecodeError) -> Self {
        let mut result = Self::invalid();
        result.status = match error {
            BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
        };
        result
    }
}

impl IntoResponse for ConnectorHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error":{"code":self.code},"resolution":self.resolution})),
        )
            .into_response()
    }
}
