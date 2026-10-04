use agentsassemble_persistence::{
    CentralOwnerSessionRequest, OWNER_SESSION_PREFIX, OwnerSessionAuthorization, PersistenceError,
    ServerOwnerAuthority,
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
    central_directory::CentralDirectoryError,
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, PRIVATE_NO_STORE, decode_json_body,
        exact_tauri_cors,
    },
    operator_pairing_web::{device_fingerprint, fingerprint_token, require_ready_origin},
};

const MAX_BODY: usize = 4096;
const GRANT_PREFIX: &str = "aacg1.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryRequest {
    session_token: String,
    generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryRequest {
    grant_token: String,
    generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomRequest {
    session_token: String,
    generation: i64,
    room_id: String,
    room_uid: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    session_token: String,
    generation: i64,
    request_id: String,
    room_id: String,
    label: String,
}

registered_routes! {
    fn central_owner_routes<AppState>() {
        same_origin_public "/api/central-owner/session" => post(exchange),
        same_origin_public "/api/central-owner/directory" => post(directory),
        same_origin_public "/api/central-owner/events" => post(directory_events),
        same_origin_public "/api/central-owner/room" => post(room),
        same_origin_public "/api/central-owner/rooms" => post(create),
    }
}

async fn exchange(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<agentsassemble_protocol::CentralOwnerSessionGrant>, CentralOwnerHttpError> {
    let origin = require_ready_origin(&state, request.headers())
        .map_err(|_| CentralOwnerHttpError::unauthorized())?;
    let device =
        device_fingerprint(request.headers()).ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let body: EntryRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(CentralOwnerHttpError::body)?;
    if fingerprint_token(&body.grant_token, GRANT_PREFIX).is_none() || body.generation < 1 {
        return Err(CentralOwnerHttpError::unauthorized());
    }
    let lease = state
        .central_directory
        .owner_connection(
            &state.central_host_identity,
            "grant_token",
            &body.grant_token,
            &origin,
            body.generation,
            &device,
        )
        .await
        .map_err(|error| CentralOwnerHttpError::central(&error))?;
    let session = state.store.create_owner_session(&lease).await?;
    Ok(Json(agentsassemble_protocol::CentralOwnerSessionGrant {
        session_token: session.session_bearer,
        server_id: session.authorization.binding().server_id.clone(),
        generation: session.authorization.binding().generation,
        expires_at: session.authorization.expires_at(),
        session_expires_at: session.authorization.binding().session_expires_at,
    }))
}

async fn directory_events(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, CentralOwnerHttpError> {
    let transport = request
        .extensions()
        .get::<crate::http_admission::HttpConnectionAdmission>()
        .cloned()
        .ok_or_else(|| {
            CentralOwnerHttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "http_connection_unavailable",
                "HTTP connection is unavailable.",
            )
        })?;
    let origin = require_ready_origin(&state, request.headers())
        .map_err(|_| CentralOwnerHttpError::unauthorized())?;
    let device =
        device_fingerprint(request.headers()).ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let body: DirectoryRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(CentralOwnerHttpError::body)?;
    let changes = state.store.subscribe_room_directory();
    let owner = authorize_directory_owner(
        &state,
        &body.session_token,
        &origin,
        body.generation,
        device,
    )
    .await?;
    let owner_lease = state.owner_sessions.retain(&state, owner.clone())?;
    let lease = state
        .connection_admission
        .acquire_directory()
        .map_err(|_| {
            CentralOwnerHttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "central_connect_capacity",
                "Server directory connection capacity is unavailable.",
            )
        })?;
    Ok(crate::room_directory_stream::directory_stream(
        state,
        changes,
        crate::room_directory_stream::DirectoryStreamAuthority::Central {
            owner: Box::new(owner),
            lease: owner_lease,
        },
        lease,
        transport.retain_authenticated_wait(),
    ))
}

pub(crate) async fn authorize_directory_owner(
    state: &AppState,
    token: &str,
    origin: &str,
    generation: i64,
    device: [u8; 32],
) -> Result<OwnerSessionAuthorization, CentralOwnerHttpError> {
    // The configured ingress origin may have changed since HTTP admission.
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::ORIGIN,
        origin
            .parse()
            .map_err(|_| CentralOwnerHttpError::unauthorized())?,
    );
    require_ready_origin(state, &headers).map_err(|_| CentralOwnerHttpError::unauthorized())?;
    let fingerprint = fingerprint_token(token, OWNER_SESSION_PREFIX)
        .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let owner = state
        .store
        .authorize_owner_session(&fingerprint, &device, origin)
        .await?;
    if owner.binding().generation != generation {
        return Err(CentralOwnerHttpError::unauthorized());
    }
    state.owner_sessions.require_live(&owner)?;
    Ok(owner)
}

/// Server-wide profile/friend access uses the same parent as an empty directory.
/// Credential-domain dispatch is explicit; rejection never tries a local ticket.
pub(crate) async fn owner_from_session_headers(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<ServerOwnerAuthority, CentralOwnerHttpError> {
    let token = crate::http_api::bearer_credential(headers)
        .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let generation = crate::ingress_trust::single_header(
        headers,
        header::HeaderName::from_static("x-central-generation"),
    )
    .and_then(|value| {
        value
            .parse::<i64>()
            .ok()
            .filter(|parsed| parsed.to_string() == value)
    })
    .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let origin = origin
        .ok_or_else(CentralOwnerHttpError::unauthorized)?
        .as_str();
    let device = device_fingerprint(headers).ok_or_else(CentralOwnerHttpError::unauthorized)?;
    authorize_directory_owner(state, token, origin, generation, device)
        .await
        .map(ServerOwnerAuthority::CentralSession)
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
    let device =
        device_fingerprint(request.headers()).ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let body: DirectoryRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(CentralOwnerHttpError::body)?;
    let owner = authorize_directory_owner(
        &state,
        &body.session_token,
        &origin,
        body.generation,
        device,
    )
    .await?;
    let authority = ServerOwnerAuthority::CentralSession(owner);
    let (bootstrap, rooms, profile_revision) = state
        .store
        .list_room_directory_for_owner(&authority, true)
        .await?;
    let rooms = rooms
        .iter()
        .map(|room| crate::room_directory_web::room_payload(room, "agent_session"))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CentralOwnerHttpError::unavailable())?;
    Ok(Json(json!({
        "server_id": bootstrap.server_id,
        "authority_lineage_id": bootstrap.authority_lineage_id,
        "server_product_surface": state.server_product_surface,
        "rooms": rooms,
        "profile_revision": profile_revision,
    })))
}

async fn create(State(state): State<AppState>, request: Request) -> Response {
    async fn execute(
        state: &AppState,
        request: Request,
    ) -> Result<Response, CentralOwnerHttpError> {
        let origin = require_ready_origin(state, request.headers())
            .map_err(|_| CentralOwnerHttpError::unauthorized())?;
        let device = device_fingerprint(request.headers())
            .ok_or_else(CentralOwnerHttpError::unauthorized)?;
        let body: CreateRequest = decode_json_body(request, MAX_BODY)
            .await
            .map_err(CentralOwnerHttpError::body)?;
        let owner =
            authorize_directory_owner(state, &body.session_token, &origin, body.generation, device)
                .await?;
        let authority = ServerOwnerAuthority::CentralSession(owner);
        Ok(crate::room_directory_web::create_room_for_owner(
            state,
            &authority,
            crate::room_directory_web::CreateRoomRequest {
                request_id: body.request_id,
                room_id: body.room_id,
                label: body.label,
            },
        )
        .await
        .into_response())
    }
    execute(&state, request).await.into_response()
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
    let owner = authorize_directory_owner(
        &state,
        &body.session_token,
        &origin,
        body.generation,
        device,
    )
    .await?;
    let expires_at = DateTime::from_timestamp(owner.binding().session_expires_at, 0)
        .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    let room_incarnation = Uuid::parse_str(&body.room_uid)
        .ok()
        .filter(|value| value.to_string() == body.room_uid)
        .ok_or_else(CentralOwnerHttpError::unauthorized)?;
    // Each room has distinct custody; the stable parent preserves exact retries.
    let mut hash = Sha256::new();
    hash.update(b"agentsassemble.central-owner-room.v1\0");
    hash.update(owner.fingerprint());
    hash.update(room_incarnation.as_bytes());
    let fingerprint: [u8; 32] = hash.finalize().into();
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
        .create_central_owner_session(
            &ServerOwnerAuthority::CentralSession(owner),
            &session_request,
        )
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
        "room_uid": snapshot.room.room_uid,
        "owner_id": principal.principal_id, "stable_identity": true, "operator": true,
        "central_owner": redemption.authorization.is_central_owner(),
        "server_id": bootstrap.server_id, "authority_lineage_id": bootstrap.authority_lineage_id,
        "server_product_surface": state.server_product_surface.as_ref(),
    })))
}

pub(crate) struct CentralOwnerHttpError {
    pub(crate) status: StatusCode,
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
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
            PersistenceError::RoomMissing => Self::new(
                StatusCode::CONFLICT,
                "room_missing",
                "The selected room no longer exists.",
            ),
            PersistenceError::CommandRejected { code, .. }
                if matches!(
                    code.as_bytes(),
                    b"room_inactive" | b"room_incarnation_changed"
                ) =>
            {
                Self::new(
                    StatusCode::CONFLICT,
                    "room_inactive",
                    "The selected room is no longer open.",
                )
            }
            PersistenceError::CommandRejected { .. } | PersistenceError::ParticipantMissing => {
                Self::unauthorized()
            }
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
