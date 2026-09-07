use agentsassemble_persistence::{LocalRoomManagerAuthority, PersistenceError};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, PRIVATE_NO_STORE, consume_local_operator,
        decode_json_body, exact_tauri_cors,
    },
    human_browser_credential::fingerprint_browser_credential,
    ingress_trust::single_header,
    public_ingress::CanonicalPublicOrigin,
};

const MAX_BODY: usize = 4096;
const PAIRING_PREFIX: &str = "aap1.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    server_id: String,
    authority_lineage_id: String,
    room_id: String,
    room_uid: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeRequest {
    authority: CreateRequest,
    pairing_id: uuid::Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedeemRequest {
    pairing_token: String,
}

registered_routes! {
    fn pairing_routes<AppState>() {
        private "/api/operator-pairing/create" => post(create),
        private "/api/operator-pairing/revoke" => post(revoke),
        same_origin_public "/api/operator-pairing/redeem" => post(redeem),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    pairing_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]).allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            DEVICE_CREDENTIAL_HEADER,
        ]))
}

async fn create(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, PairingHttpError> {
    consume_local_operator(&state, request.headers())
        .await
        .ok_or_else(PairingHttpError::unauthorized)?;
    let body: CreateRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(PairingHttpError::body)?;
    let manager = resolve_manager(&state, body).await?;
    let origin = ready_origin(&state)?;
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| PairingHttpError::internal())?;
    let token = format!("{PAIRING_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    let fingerprint = Sha256::digest(token.as_bytes()).into();
    let grant = state
        .store
        .create_operator_pairing(&manager, &fingerprint, &origin, Utc::now())
        .await?;
    Ok(Json(json!({
        "pairing_id": grant.pairing_id,
        "expires_at": grant.expires_at,
        "pairing_url": format!("{origin}/pair?token={token}"),
    })))
}

async fn revoke(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, PairingHttpError> {
    consume_local_operator(&state, request.headers())
        .await
        .ok_or_else(PairingHttpError::unauthorized)?;
    let body: RevokeRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(PairingHttpError::body)?;
    let manager = resolve_manager(&state, body.authority).await?;
    state
        .rooms
        .revoke_operator_pairing(&manager, body.pairing_id)
        .await?;
    Ok(Json(
        json!({"status": "revoked", "pairing_id": body.pairing_id}),
    ))
}

async fn redeem(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, PairingHttpError> {
    let origin = require_ready_origin(&state, request.headers())?;
    let device =
        device_fingerprint(request.headers()).ok_or_else(PairingHttpError::unauthorized)?;
    let body: RedeemRequest = decode_json_body(request, MAX_BODY)
        .await
        .map_err(PairingHttpError::body)?;
    let fingerprint = fingerprint_token(&body.pairing_token, PAIRING_PREFIX)
        .ok_or_else(PairingHttpError::unauthorized)?;
    let redemption = state
        .store
        .redeem_operator_pairing(&fingerprint, &device, &origin, Utc::now())
        .await?;
    let principal = redemption.authorization.principal();
    let snapshot = state.store.snapshot_for(principal, 0, 0).await?;
    let bootstrap = state.store.local_bootstrap_status().await?;
    state
        .store
        .revalidate_room_session_authorization(
            &agentsassemble_persistence::RoomSessionAuthorization::Operator(
                redemption.authorization.clone(),
            ),
        )
        .await?;
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

async fn resolve_manager(
    state: &AppState,
    body: CreateRequest,
) -> Result<LocalRoomManagerAuthority, PairingHttpError> {
    crate::ticket_issuer::resolve_local_room_manager(
        state,
        &crate::ManagerRoomAuthorityRequest {
            server_id: body.server_id,
            authority_lineage_id: body.authority_lineage_id,
            room_id: body.room_id,
            room_uid: body.room_uid,
        },
    )
    .await
    .map_err(|error| match error {
        crate::TicketIssueError::Persistence(error) => error.into(),
        _ => PairingHttpError::unauthorized(),
    })
}

pub(crate) fn ready_origin(state: &AppState) -> Result<String, PairingHttpError> {
    state
        .public_ingress
        .ready_snapshot()
        .map(|ready| ready.public_url)
        .ok_or_else(|| {
            PairingHttpError::new(
                StatusCode::CONFLICT,
                "public_ingress_not_ready",
                "Public ingress is not ready.",
            )
        })
}

pub(crate) fn require_ready_origin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, PairingHttpError> {
    let origin = ready_origin(state)?;
    if single_header(headers, header::ORIGIN)
        .and_then(|value| CanonicalPublicOrigin::parse(value).ok())
        .is_none_or(|presented| presented.value != origin)
    {
        return Err(PairingHttpError::unauthorized());
    }
    Ok(origin)
}

pub(crate) fn device_fingerprint(headers: &HeaderMap) -> Option<[u8; 32]> {
    fingerprint_browser_credential(single_header(headers, DEVICE_CREDENTIAL_HEADER)?)
}

pub(crate) fn fingerprint_token(token: &str, prefix: &str) -> Option<[u8; 32]> {
    if token.len() != prefix.len() + 43 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(token.strip_prefix(prefix)?).ok()?;
    if decoded.len() != 32 {
        return None;
    }
    Some(Sha256::digest(token.as_bytes()).into())
}

pub(crate) struct PairingHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl PairingHttpError {
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
            "pairing_invalid",
            "Current operator pairing authority is required.",
        )
    }
    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "pairing_failed",
            "Operator pairing failed.",
        )
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

impl From<PersistenceError> for PairingHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected {
                code: "pairing_capacity",
                ..
            } => Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "pairing_capacity",
                "Operator pairing capacity is unavailable.",
            ),
            PersistenceError::CommandRejected { .. }
            | PersistenceError::ParticipantMissing
            | PersistenceError::RoomMissing => Self::unauthorized(),
            _ => Self::internal(),
        }
    }
}

impl IntoResponse for PairingHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
