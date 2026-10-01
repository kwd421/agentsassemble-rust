use agentsassemble_persistence::{
    CentralOwnerSessionRequest, LocalBootstrapPhase, PersistenceError,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use crate::{
    AppState,
    central_directory::{CentralDirectoryError, RedeemedConnectGrant},
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, PRIVATE_NO_STORE, decode_json_body,
        exact_tauri_cors,
    },
    operator_pairing_web::{device_fingerprint, require_ready_origin},
};

const MAX_BODY: usize = 4096;
const GRANT_PREFIX: &str = "aacg1.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryRequest {
    grant_token: String,
    generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomRequest {
    grant_token: String,
    generation: i64,
    room_id: String,
    room_uid: String,
}

registered_routes! {
    fn central_owner_routes<AppState>() {
        same_origin_public "/api/central-owner/directory" => post(directory),
        same_origin_public "/api/central-owner/room" => post(room),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    central_owner_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(
            exact_tauri_cors([Method::POST])
                .allow_headers([header::CONTENT_TYPE, DEVICE_CREDENTIAL_HEADER]),
        )
}

async fn directory(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, CentralOwnerHttpError> {
    let origin = require_ready_origin(&state, request.headers())
        .map_err(|_| CentralOwnerHttpError::unauthorized())?;
    device_fingerprint(request.headers()).ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let body: DirectoryRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(CentralOwnerHttpError::body)?;
    let grant = redeem(&state, &body.grant_token, &origin, body.generation).await?;
    let bootstrap = state.store.local_bootstrap_status().await?;
    if bootstrap.phase != LocalBootstrapPhase::Complete {
        return Err(CentralOwnerHttpError::unavailable());
    }
    let rooms = state.store.list_room_directory(false).await?;
    Ok(Json(json!({
        "server_id": bootstrap.server_id,
        "authority_lineage_id": bootstrap.authority_lineage_id,
        "expires_at": grant.expires_at,
        "rooms": rooms.into_iter().map(|summary| json!({
            "room_id": summary.room.room_id,
            "room_uid": summary.room.room_uid,
            "label": summary.room.label,
            "status": summary.room.status,
            "created_at": summary.room.created_at,
            "updated_at": summary.room.updated_at,
            "topic": summary.settings.topic,
        })).collect::<Vec<_>>(),
    })))
}

async fn room(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, CentralOwnerHttpError> {
    let origin = require_ready_origin(&state, request.headers())
        .map_err(|_| CentralOwnerHttpError::unauthorized())?;
    let device =
        device_fingerprint(request.headers()).ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let body: RoomRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(CentralOwnerHttpError::body)?;
    let grant = redeem(&state, &body.grant_token, &origin, body.generation).await?;
    let expires_at = DateTime::from_timestamp(grant.expires_at, 0)
        .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let fingerprint: [u8; 32] = Sha256::digest(body.grant_token.as_bytes()).into();
    let room_incarnation = Uuid::parse_str(&body.room_uid)
        .ok()
        .filter(|value| value.to_string() == body.room_uid)
        .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let session_request = CentralOwnerSessionRequest::new(
        &body.room_id,
        room_incarnation,
        &fingerprint,
        &device,
        &origin,
        expires_at,
        Utc::now(),
    );
    let redemption = state
        .store
        .create_central_owner_session(&session_request)
        .await?;
    let principal = redemption.authorization.principal();
    let snapshot = state
        .store
        .snapshot_for(
            agentsassemble_persistence::RoomMutationAuthority::OperatorSession(
                &redemption.authorization,
            ),
            0,
            0,
        )
        .await?;
    let bootstrap = state.store.local_bootstrap_status().await?;
    Ok(Json(json!({
        "status": "admitted", "session_token": redemption.session_bearer,
        "agent_id": principal.participant_id, "display_name": principal.display_name,
        "meeting_id": principal.room_id, "invite_scope": "room", "participant_type": "human",
        "client_type": "browser", "provider_kind": "manual", "connection_kind": "browser",
        "expires_at": redemption.authorization.expires_at(), "room_label": snapshot.room.label,
        "room_topic": snapshot.settings.topic, "room_created_at": snapshot.room.created_at,
        "owner_id": principal.principal_id, "stable_identity": true, "operator": true,
        "server_id": bootstrap.server_id, "authority_lineage_id": bootstrap.authority_lineage_id,
        "server_product_surface": state.server_product_surface.as_ref(),
    })))
}

async fn redeem(
    state: &AppState,
    token: &str,
    origin: &str,
    generation: i64,
) -> Result<RedeemedConnectGrant, CentralOwnerHttpError> {
    if token.len() != GRANT_PREFIX.len() + 43
        || !token.starts_with(GRANT_PREFIX)
        || !token[GRANT_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || generation < 1
    {
        return Err(CentralOwnerHttpError::unauthorized());
    }
    state
        .central_directory
        .redeem_connect_grant(&state.central_host_identity, token, origin, generation)
        .await
        .map_err(|error| CentralOwnerHttpError::central(&error))
}

struct CentralOwnerHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl CentralOwnerHttpError {
    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }
    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "central_connect_invalid",
            "The central owner connection is no longer valid.",
        )
    }
    fn unavailable() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "central_connect_unavailable",
            "The selected server is not ready.",
        )
    }
    fn central(error: &CentralDirectoryError) -> Self {
        match error {
            CentralDirectoryError::Rejected | CentralDirectoryError::InvalidResponse => {
                Self::unauthorized()
            }
            _ => Self::new(
                StatusCode::BAD_GATEWAY,
                "central_connect_unavailable",
                "Central owner verification is temporarily unavailable.",
            ),
        }
    }
    fn body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
            ),
            BodyDecodeError::PayloadTooLarge => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Request body exceeds the route limit.",
            ),
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "A supported JSON object is required.",
            ),
        }
    }
}

impl From<PersistenceError> for CentralOwnerHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected { code, .. }
                if code.as_bytes() == b"pairing_capacity" =>
            {
                Self::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "central_connect_capacity",
                    "Central owner session capacity is unavailable.",
                )
            }
            PersistenceError::CommandRejected { .. }
            | PersistenceError::ParticipantMissing
            | PersistenceError::RoomMissing => Self::unauthorized(),
            _ => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "central_connect_failed",
                "Central owner connection failed.",
            ),
        }
    }
}

impl IntoResponse for CentralOwnerHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
