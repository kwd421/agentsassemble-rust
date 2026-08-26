use agentsassemble_domain::{
    AuthenticatedPrincipal, InviteScope, LOCAL_OPERATOR_USER_ID, UserProfilePatch,
};
use agentsassemble_persistence::{
    HumanPrejoinAvatarAuthorization, HumanSessionAuthorization, MAX_RASTER_BYTES, PersistenceError,
    ProfileAttachment,
};
use axum::{
    Json, Router, body,
    extract::{Path, Query, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;

use crate::{
    AppState, ConsumedProfileTicket,
    http_api::{
        BodyDecodeError, bearer_ticket, decode_json_body, ensure_empty_body, exact_tauri_cors,
    },
    human_browser_credential::fingerprint_browser_credential,
    human_invite_preflight::authenticated_invite_evidence,
};

const MAX_PROFILE_BODY_BYTES: usize = 16 * 1024;
const MAX_ATTACHMENT_BODY_BYTES: usize = MAX_RASTER_BYTES.div_ceil(3) * 4 + (64 * 1024);

#[derive(Deserialize)]
struct AttachmentUpload {
    purpose: String,
    #[serde(default)]
    invite_token: String,
    #[serde(default)]
    device_token: String,
    filename: String,
    content_type: String,
    data_base64: String,
}

pub(crate) fn routes() -> Router<AppState> {
    profile_routes().layer(profile_cors())
}

registered_routes! {
    fn profile_routes<AppState>() {
        "/api/user-profile" => get(read_profile).post(update_profile),
        "/api/attachments" => post(upload_attachment),
        "/api/attachments/{attachment_id}" => get(read_attachment),
    }
}

fn profile_cors() -> CorsLayer {
    exact_tauri_cors([Method::GET, Method::POST])
}

async fn read_profile(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let authority = consume_profile_authority(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let profile = match authority {
        ProfileAuthority::Room(principal) => state.store.user_profile(&principal).await?,
        ProfileAuthority::HumanSession(authorization) => {
            state.store.human_session_profile(&authorization).await?
        }
        ProfileAuthority::LocalOperator => state.store.local_operator_profile().await?,
    };
    Ok(Json(json!({"profile": profile})))
}

async fn update_profile(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let authority = consume_profile_authority(&state, request.headers()).await?;
    let patch: UserProfilePatch = decode_json_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let outcome = match authority {
        ProfileAuthority::Room(principal) => {
            state.store.update_user_profile(&principal, patch).await?
        }
        ProfileAuthority::HumanSession(authorization) => {
            state
                .store
                .update_human_session_profile(&authorization, patch)
                .await?
        }
        ProfileAuthority::LocalOperator => state.store.update_local_operator_profile(patch).await?,
    };
    state.rooms.notify_committed_events(&outcome.events).await;
    Ok(Json(json!({"profile": outcome.profile})))
}

async fn upload_attachment(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let authority = if request.headers().contains_key(header::AUTHORIZATION) {
        Some(consume_profile_authority(&state, request.headers()).await?)
    } else {
        None
    };
    if authority.as_ref().is_some_and(|authority| {
        matches!(
            authority,
            ProfileAuthority::HumanSession(authorization)
                if authorization.principal().invite_scope == InviteScope::ReadOnly
        )
    }) {
        return Err(ProfileHttpError::new(
            StatusCode::FORBIDDEN,
            "session_read_only",
            "Read-only room sessions cannot upload profile avatars.",
        ));
    }
    let payload: AttachmentUpload = decode_json_body(request, MAX_ATTACHMENT_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    if payload.purpose.trim() != "profile_avatar" {
        return Err(match authority {
            Some(_) => ProfileHttpError::bad_request(
                "Only profile_avatar attachments are available in this runtime.",
            ),
            None => ProfileHttpError::new(
                StatusCode::UNAUTHORIZED,
                "profile_authority_required",
                "A profile upload authority is required.",
            ),
        });
    }
    let prejoin_authority = if authority.is_none() {
        Some(authorize_prejoin_avatar_upload(&state, &payload).await?)
    } else {
        None
    };
    let encoded = payload.data_base64.trim();
    if encoded.is_empty() {
        return Err(ProfileHttpError::bad_request("data_base64 is required."));
    }
    let content = STANDARD
        .decode(encoded)
        .map_err(|_| ProfileHttpError::bad_request("data_base64 is invalid."))?;
    let attachment = match (authority, prejoin_authority) {
        (Some(ProfileAuthority::Room(principal)), None) => {
            state
                .store
                .store_profile_attachment(
                    &principal,
                    &payload.filename,
                    &payload.content_type,
                    content,
                )
                .await?
        }
        (Some(ProfileAuthority::HumanSession(authorization)), None) => {
            state
                .store
                .store_human_session_profile_attachment(
                    &authorization,
                    &payload.filename,
                    &payload.content_type,
                    content,
                )
                .await?
        }
        (Some(ProfileAuthority::LocalOperator), None) => {
            state
                .store
                .store_local_operator_profile_attachment(
                    &payload.filename,
                    &payload.content_type,
                    content,
                )
                .await?
        }
        (None, Some(prejoin_authorization)) => {
            state
                .store
                .store_human_prejoin_avatar(
                    &prejoin_authorization,
                    &payload.filename,
                    &payload.content_type,
                    content,
                )
                .await?
        }
        _ => return Err(ProfileHttpError::internal()),
    };
    Ok(Json(json!({"attachment": attachment})))
}

async fn authorize_prejoin_avatar_upload(
    state: &AppState,
    payload: &AttachmentUpload,
) -> Result<HumanPrejoinAvatarAuthorization, ProfileHttpError> {
    if payload.invite_token.trim().is_empty() {
        return Err(ProfileHttpError::new(
            StatusCode::UNAUTHORIZED,
            "invite_token_required",
            "invite_token is required for a pre-join profile upload.",
        ));
    }
    let credential =
        authenticated_invite_evidence(&state.human_invite_credentials, payload.invite_token.trim())
            .map_err(|_| {
                ProfileHttpError::new(
                    StatusCode::FORBIDDEN,
                    "invite_invalid",
                    "Invite is invalid.",
                )
            })?;
    let browser_credential_fingerprint =
        fingerprint_browser_credential(payload.device_token.trim()).ok_or_else(|| {
            ProfileHttpError::new(
                StatusCode::BAD_REQUEST,
                "browser_credential_invalid",
                "A canonical browser credential is required.",
            )
        })?;
    state
        .store
        .authorize_human_prejoin_avatar(&credential, &browser_credential_fingerprint)
        .await
        .map_err(ProfileHttpError::from)
}

async fn read_attachment(
    State(state): State<AppState>,
    Path(attachment_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Response, ProfileHttpError> {
    let attachment = state.store.profile_attachment(&attachment_id).await?;
    attachment_response(
        attachment,
        query.contains_key("view") && !query.contains_key("download"),
    )
}

fn attachment_response(
    attachment: ProfileAttachment,
    inline: bool,
) -> Result<Response, ProfileHttpError> {
    let disposition = if inline { "inline" } else { "attachment" };
    let fallback_name: String = attachment
        .metadata
        .filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let fallback_name = if fallback_name.trim_matches(['.', '_']).is_empty() {
        "attachment.bin"
    } else {
        fallback_name.as_str()
    };
    let mut response = Response::new(body::Body::from(attachment.content));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&attachment.metadata.content_type)
            .map_err(|_| ProfileHttpError::internal())?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disposition}; filename=\"{fallback_name}\""))
            .map_err(|_| ProfileHttpError::internal())?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

enum ProfileAuthority {
    Room(AuthenticatedPrincipal),
    HumanSession(HumanSessionAuthorization),
    LocalOperator,
}

async fn consume_profile_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<ProfileAuthority, ProfileHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    match state
        .tickets
        .consume_profile(ticket)
        .await
        .map_err(|_| ProfileHttpError::unauthorized())?
    {
        ConsumedProfileTicket::Room(principal) => Ok(ProfileAuthority::Room(principal)),
        ConsumedProfileTicket::HumanSession(authorization) => {
            Ok(ProfileAuthority::HumanSession(authorization))
        }
        ConsumedProfileTicket::ServerOperator { principal_id }
            if principal_id == LOCAL_OPERATOR_USER_ID =>
        {
            Ok(ProfileAuthority::LocalOperator)
        }
        ConsumedProfileTicket::ServerOperator { .. } => Err(ProfileHttpError::unauthorized()),
    }
}

#[derive(Debug)]
struct ProfileHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ProfileHttpError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => Self::payload_too_large(),
            BodyDecodeError::InvalidJson => Self::bad_request("Request JSON is invalid."),
            BodyDecodeError::NonEmpty => {
                Self::bad_request("GET user-profile requests must not contain a body.")
            }
        }
    }
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
            message: "A valid one-use user-profile ticket is required.".to_owned(),
        }
    }

    fn payload_too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large",
            message: "Request body exceeds the route limit.".to_owned(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "persistence_failed",
            message: "Persistence operation failed.".to_owned(),
        }
    }
}

impl From<PersistenceError> for ProfileHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected { code, message } => {
                let status = match code {
                    "session_revoked" => StatusCode::UNAUTHORIZED,
                    "attachment_missing" | "user_profile_missing" => StatusCode::NOT_FOUND,
                    "invite_invalid"
                    | "invite_revoked"
                    | "token_expired"
                    | "invite_use_limit_reached"
                    | "session_read_only" => StatusCode::FORBIDDEN,
                    "room_unavailable" => StatusCode::GONE,
                    "attachment_owner_mismatch" | "profile_authority_mismatch" => {
                        StatusCode::FORBIDDEN
                    }
                    "attachment_quota_reached" => StatusCode::TOO_MANY_REQUESTS,
                    "attachment_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
                    "attachment_type_unsupported"
                    | "attachment_type_mismatch"
                    | "attachment_invalid_image"
                    | "attachment_image_limits" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "invalid_state" => StatusCode::SERVICE_UNAVAILABLE,
                    _ => StatusCode::BAD_REQUEST,
                };
                Self {
                    status,
                    code,
                    message,
                }
            }
            PersistenceError::ParticipantMissing | PersistenceError::RoomMissing => {
                Self::unauthorized()
            }
            internal => {
                tracing::error!(error = ?internal, "profile HTTP persistence operation failed");
                Self::internal()
            }
        }
    }
}

impl IntoResponse for ProfileHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
