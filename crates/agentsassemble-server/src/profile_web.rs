use agentsassemble_domain::{AuthenticatedPrincipal, UserProfilePatch};
use agentsassemble_persistence::{PersistenceError, ProfileAttachment};
use axum::{
    Json, Router, body,
    extract::{Path, Query, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, bearer_ticket, decode_json_body, ensure_empty_body, exact_tauri_cors,
    },
};

const MAX_PROFILE_BODY_BYTES: usize = 16 * 1024;
const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_ATTACHMENT_BODY_BYTES: usize = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 + (64 * 1024);

#[derive(Debug, Deserialize)]
struct AttachmentUpload {
    purpose: String,
    filename: String,
    content_type: String,
    data_base64: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user-profile", get(read_profile).post(update_profile))
        .route("/api/attachments", post(upload_attachment))
        .route("/api/attachments/{attachment_id}", get(read_attachment))
        .layer(profile_cors())
}

fn profile_cors() -> CorsLayer {
    exact_tauri_cors([Method::GET, Method::POST])
}

async fn read_profile(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let principal = consume_principal(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let profile = state.store.user_profile(&principal).await?;
    Ok(Json(json!({"profile": profile})))
}

async fn update_profile(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let principal = consume_principal(&state, request.headers()).await?;
    let patch: UserProfilePatch = decode_json_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let outcome = state.store.update_user_profile(&principal, patch).await?;
    state.rooms.notify_committed_events(&outcome.events).await;
    Ok(Json(json!({"profile": outcome.profile})))
}

async fn upload_attachment(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let principal = consume_principal(&state, request.headers()).await?;
    let payload: AttachmentUpload = decode_json_body(request, MAX_ATTACHMENT_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    if payload.purpose.trim() != "profile_avatar" {
        return Err(ProfileHttpError::bad_request(
            "Only profile_avatar attachments are available in this runtime.",
        ));
    }
    let encoded = payload.data_base64.trim();
    if encoded.is_empty() {
        return Err(ProfileHttpError::bad_request("data_base64 is required."));
    }
    let content = STANDARD
        .decode(encoded)
        .map_err(|_| ProfileHttpError::bad_request("data_base64 is invalid."))?;
    let attachment = state
        .store
        .store_profile_attachment(
            &principal,
            &payload.filename,
            &payload.content_type,
            content,
        )
        .await?;
    Ok(Json(json!({"attachment": attachment})))
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

async fn consume_principal(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<AuthenticatedPrincipal, ProfileHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    state
        .tickets
        .consume(ticket)
        .await
        .map(|grant| grant.principal)
        .map_err(|_| ProfileHttpError::unauthorized())
}

#[derive(Debug)]
struct ProfileHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ProfileHttpError {
    fn from_body(error: BodyDecodeError) -> Self {
        match error {
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
                    "attachment_owner_mismatch" | "profile_authority_mismatch" => {
                        StatusCode::FORBIDDEN
                    }
                    "attachment_quota_reached" => StatusCode::TOO_MANY_REQUESTS,
                    "attachment_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
                    "attachment_type_unsupported"
                    | "attachment_type_mismatch"
                    | "attachment_invalid_image"
                    | "attachment_image_limits" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
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
