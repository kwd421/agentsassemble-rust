use agentsassemble_domain::{InviteScope, clean_single_line, validate_room_id};
use agentsassemble_persistence::{NewHumanInvite, PersistenceError};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_credential, decode_json_body, exact_tauri_cors,
    },
    human_invite_credentials::{HumanInviteCredentialDraft, format_invite_timestamp},
    ticket::ConsumedHumanInviteManagerTicket,
};

const MAX_MANAGER_BODY_BYTES: usize = 8 * 1024;
const DEFAULT_INVITE_TTL_SECONDS: i64 = 600;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInviteRequest {
    meeting_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default = "default_invite_scope")]
    invite_scope: String,
    #[serde(default = "default_invite_ttl")]
    ttl_seconds: i64,
    #[serde(default)]
    max_uses: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeInviteRequest {
    meeting_id: String,
    invite_id: String,
}

#[derive(Serialize)]
struct CreateInviteResponse {
    invite_id: String,
    invite_token: String,
    join_code: String,
    meeting_id: String,
    agent_id: String,
    display_name: String,
    invite_scope: &'static str,
    participant_type: &'static str,
    client_type: &'static str,
    provider_kind: &'static str,
    permission_mode: &'static str,
    max_uses: i64,
    expires_at: String,
    room_url: String,
    join_url: String,
}

registered_routes! {
    fn manager_routes<AppState>() {
        private "/api/room-invite/create" => post(create_invite),
        private "/api/room-invite/revoke" => post(revoke_invite),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    manager_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]))
}

async fn create_invite(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<CreateInviteResponse>, InviteManagerHttpError> {
    let grant = consume_create_ticket(&state, request.headers()).await?;
    let payload: CreateInviteRequest = decode_json_body(request, MAX_MANAGER_BODY_BYTES)
        .await
        .map_err(InviteManagerHttpError::from_body)?;
    let room_id = bound_room_id(&grant, &payload.meeting_id)?;
    let display_name = clean_single_line(&payload.display_name, 128);
    let display_name = if display_name.is_empty() {
        "Guest".to_owned()
    } else {
        display_name
    };
    let invite_scope = normalize_invite_scope(&payload.invite_scope);
    let max_uses = payload.max_uses.max(0);
    let (issued_at, expires_at) = invite_window(payload.ttl_seconds)?;
    let ingress = state
        .public_ingress
        .ready_snapshot()
        .ok_or_else(InviteManagerHttpError::ingress_not_ready)?;
    let base_participant_id = format!("guest-{}", Uuid::new_v4().simple());
    let credentials = state
        .human_invite_credentials
        .issue(&HumanInviteCredentialDraft {
            room_url: ingress.local_url.clone(),
            public_room_url: ingress.public_url.clone(),
            room_id: room_id.clone(),
            base_participant_id: base_participant_id.clone(),
            display_name: display_name.clone(),
            invite_scope,
            issued_at,
            expires_at,
        })?;
    let invite = state
        .store
        .create_human_invite_for_local_manager(
            &grant.authority,
            NewHumanInvite {
                signed_token_fingerprint: *credentials.signed_token_fingerprint(),
                join_code_fingerprint: *credentials.join_code_fingerprint(),
                base_participant_id: base_participant_id.clone(),
                display_name: display_name.clone(),
                invite_scope,
                max_uses,
                expires_at,
                created_at: issued_at,
            },
        )
        .await?;
    let (invite_scope, permission_mode) = scope_wire(invite_scope);
    Ok(Json(CreateInviteResponse {
        invite_id: invite.invite_id,
        invite_token: credentials.invite_token().to_owned(),
        join_code: credentials.join_code().to_owned(),
        meeting_id: room_id,
        agent_id: base_participant_id,
        display_name,
        invite_scope,
        participant_type: "human",
        client_type: "browser",
        provider_kind: "manual",
        permission_mode,
        max_uses,
        expires_at: format_invite_timestamp(expires_at),
        room_url: ingress.local_url,
        join_url: format!(
            "{}/join?token={}",
            ingress.public_url,
            credentials.join_code()
        ),
    }))
}

async fn revoke_invite(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, InviteManagerHttpError> {
    let grant = consume_revoke_ticket(&state, request.headers()).await?;
    let payload: RevokeInviteRequest = decode_json_body(request, MAX_MANAGER_BODY_BYTES)
        .await
        .map_err(InviteManagerHttpError::from_body)?;
    bound_room_id(&grant, &payload.meeting_id)?;
    if !state
        .store
        .revoke_human_invite_for_local_manager(&grant.authority, &payload.invite_id)
        .await?
    {
        return Err(InviteManagerHttpError::not_found());
    }
    Ok(Json(
        json!({"status": "revoked", "invite_id": payload.invite_id}),
    ))
}

async fn consume_create_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<ConsumedHumanInviteManagerTicket, InviteManagerHttpError> {
    let ticket = bearer_credential(headers).ok_or_else(InviteManagerHttpError::unauthorized)?;
    state
        .tickets
        .consume_human_invite_create(ticket)
        .await
        .map_err(|_| InviteManagerHttpError::unauthorized())
}

async fn consume_revoke_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<ConsumedHumanInviteManagerTicket, InviteManagerHttpError> {
    let ticket = bearer_credential(headers).ok_or_else(InviteManagerHttpError::unauthorized)?;
    state
        .tickets
        .consume_human_invite_revoke(ticket)
        .await
        .map_err(|_| InviteManagerHttpError::unauthorized())
}

fn bound_room_id(
    grant: &ConsumedHumanInviteManagerTicket,
    requested: &str,
) -> Result<String, InviteManagerHttpError> {
    let room_id = validate_room_id(requested)
        .map_err(|error| InviteManagerHttpError::bad_request(error.message))?;
    if room_id != grant.authority.manager.room_id {
        return Err(InviteManagerHttpError::unauthorized());
    }
    Ok(room_id)
}

fn invite_window(
    ttl_seconds: i64,
) -> Result<(DateTime<Utc>, DateTime<Utc>), InviteManagerHttpError> {
    let duration = (ttl_seconds > 0)
        .then(|| TimeDelta::try_seconds(ttl_seconds))
        .flatten()
        .ok_or_else(|| InviteManagerHttpError::bad_request("ttl_seconds must be positive."))?;
    let issued_at = DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or_else(InviteManagerHttpError::internal)?;
    let expires_at = issued_at
        .checked_add_signed(duration)
        .ok_or_else(|| InviteManagerHttpError::bad_request("ttl_seconds is out of range."))?;
    Ok((issued_at, expires_at))
}

fn normalize_invite_scope(value: &str) -> InviteScope {
    if clean_single_line(value, 32) == "read_only" {
        InviteScope::ReadOnly
    } else {
        InviteScope::ReadWrite
    }
}

const fn scope_wire(scope: InviteScope) -> (&'static str, &'static str) {
    match scope {
        InviteScope::ReadWrite => ("room", "participant"),
        InviteScope::ReadOnly => ("read_only", "meeting_read_only"),
    }
}

fn default_invite_scope() -> String {
    "room".to_owned()
}

const fn default_invite_ttl() -> i64 {
    DEFAULT_INVITE_TTL_SECONDS
}

#[derive(Debug)]
struct InviteManagerHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl InviteManagerHttpError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use room invite management ticket is required.".to_owned(),
        }
    }

    fn ingress_not_ready() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "public_ingress_not_ready",
            message: "Public ingress must be ready before creating an invite.".to_owned(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "invite_not_found",
            message: "Invite was not found in this room.".to_owned(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "invite_management_failed",
            message: "Invite management failed.".to_owned(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Request body exceeds the route limit.".to_owned(),
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => {
                Self::bad_request("Request body must be one supported JSON object.")
            }
        }
    }
}

impl From<PersistenceError> for InviteManagerHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::RoomMissing => Self {
                status: StatusCode::NOT_FOUND,
                code: "room_not_found",
                message: "Room does not exist.".to_owned(),
            },
            PersistenceError::ParticipantMissing
            | PersistenceError::CommandRejected {
                code:
                    "session_revoked"
                    | "user_profile_missing"
                    | "profile_authority_mismatch"
                    | "permission_denied"
                    | "room_authority_changed",
                ..
            } => Self {
                status: StatusCode::FORBIDDEN,
                code: "room_manager_required",
                message: "Current room-manager authority is required.".to_owned(),
            },
            PersistenceError::CommandRejected {
                code: "room_inactive",
                ..
            } => Self {
                status: StatusCode::GONE,
                code: "room_closed",
                message: "Room is closed.".to_owned(),
            },
            PersistenceError::CommandRejected {
                code: "invalid_human_invite" | "invalid_human_invite_id",
                message,
            } => Self::bad_request(message),
            _ => Self::internal(),
        }
    }
}

impl From<crate::HumanInviteCredentialError> for InviteManagerHttpError {
    fn from(_error: crate::HumanInviteCredentialError) -> Self {
        Self::internal()
    }
}

impl IntoResponse for InviteManagerHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}
