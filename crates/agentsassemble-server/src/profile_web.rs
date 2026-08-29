use agentsassemble_domain::{
    AuthenticatedPrincipal, InviteScope, LOCAL_OPERATOR_USER_ID, UserProfilePatch,
};
use agentsassemble_persistence::{
    HumanPrejoinAvatarAuthorization, HumanSessionAuthorization, PersistenceError,
};
use axum::{
    Json, Router, body,
    extract::{Path, Query, RawQuery, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer};

use crate::{
    AppState, ConsumedProfileTicket,
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, INVITE_CREDENTIAL_HEADER,
        MAX_BASE64_UPLOAD_BODY_BYTES, PRIVATE_NO_STORE, bearer_ticket, decode_json_body,
        ensure_empty_body, exact_tauri_cors,
    },
    human_browser_credential::fingerprint_browser_credential,
    human_invite_preflight::authenticated_invite_evidence,
    ingress_trust::single_header,
    ticket::{
        ConsumedAppearanceReadTicket, ConsumedAttachmentUploadTicket,
        ConsumedMessageAttachmentReadTicket, ConsumedRoomHumanTicket,
    },
};

const MAX_PROFILE_BODY_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct AttachmentUpload {
    purpose: String,
    filename: String,
    content_type: String,
    data_base64: String,
}

#[derive(Deserialize)]
struct ProfileUpdateRequest {
    expected_revision: i64,
    #[serde(flatten)]
    patch: UserProfilePatch,
}

pub(crate) fn routes() -> Router<AppState> {
    profile_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(profile_cors())
}

registered_routes! {
    fn profile_routes<AppState>() {
        same_origin_public "/api/user-profile" => get(read_profile).post(update_profile),
        same_origin_public "/api/attachments" => post(upload_attachment),
        same_origin_public "/api/attachments/{attachment_id}" => get(read_attachment),
    }
}

fn profile_cors() -> CorsLayer {
    exact_tauri_cors([Method::GET, Method::POST]).allow_headers([
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        DEVICE_CREDENTIAL_HEADER,
        INVITE_CREDENTIAL_HEADER,
    ])
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
    let update: ProfileUpdateRequest = decode_json_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let outcome = match authority {
        ProfileAuthority::Room(principal) => {
            state
                .store
                .update_user_profile(&principal, update.expected_revision, update.patch)
                .await?
        }
        ProfileAuthority::HumanSession(authorization) => {
            state
                .store
                .update_human_session_profile(
                    &authorization,
                    update.expected_revision,
                    update.patch,
                )
                .await?
        }
        ProfileAuthority::LocalOperator => {
            state
                .store
                .update_local_operator_profile(update.expected_revision, update.patch)
                .await?
        }
    };
    state.rooms.notify_committed_events(&outcome.events).await;
    Ok(Json(json!({"profile": outcome.profile})))
}

async fn upload_attachment(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let (authority, prejoin_authority) = if request.headers().contains_key(header::AUTHORIZATION) {
        (
            Some(consume_attachment_upload_authority(&state, request.headers()).await?),
            None,
        )
    } else {
        (
            None,
            Some(authorize_prejoin_avatar_upload(&state, request.headers()).await?),
        )
    };
    if authority.as_ref().is_some_and(|authority| {
        matches!(
            authority,
            AttachmentUploadAuthority::Profile(ProfileAuthority::HumanSession(authorization))
                if authorization.principal().invite_scope == InviteScope::ReadOnly
        )
    }) {
        return Err(ProfileHttpError::new(
            StatusCode::FORBIDDEN,
            "session_read_only",
            "Read-only room sessions cannot upload profile avatars.",
        ));
    }
    let payload: AttachmentUpload = decode_json_body(request, MAX_BASE64_UPLOAD_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    match (&authority, payload.purpose.trim()) {
        (Some(AttachmentUploadAuthority::Appearance(_)), "room_appearance")
        | (Some(AttachmentUploadAuthority::Message(_)), "room_attachment")
        | (Some(AttachmentUploadAuthority::Profile(_)) | None, "profile_avatar") => {}
        (Some(_), _) => {
            return Err(ProfileHttpError::bad_request(
                "Attachment purpose does not match its one-use authority.",
            ));
        }
        (None, _) => {
            return Err(ProfileHttpError::new(
                StatusCode::UNAUTHORIZED,
                "profile_authority_required",
                "A profile upload authority is required.",
            ));
        }
    }
    let encoded = payload.data_base64.trim();
    if encoded.is_empty() {
        return Err(ProfileHttpError::bad_request("data_base64 is required."));
    }
    let content = STANDARD
        .decode(encoded)
        .map_err(|_| ProfileHttpError::bad_request("data_base64 is invalid."))?;
    let attachment =
        store_uploaded_attachment(&state, authority, prejoin_authority, &payload, content).await?;
    Ok(Json(json!({"attachment": attachment})))
}

async fn store_uploaded_attachment(
    state: &AppState,
    authority: Option<AttachmentUploadAuthority>,
    prejoin_authority: Option<HumanPrejoinAvatarAuthorization>,
    payload: &AttachmentUpload,
    content: Vec<u8>,
) -> Result<serde_json::Value, ProfileHttpError> {
    Ok(match (authority, prejoin_authority) {
        (Some(AttachmentUploadAuthority::Appearance(manager)), None) => json!(
            state
                .store
                .store_pending_room_appearance_asset(
                    &manager,
                    &payload.filename,
                    &payload.content_type,
                    content,
                )
                .await?
        ),
        (Some(AttachmentUploadAuthority::Profile(profile)), None) => {
            json!(store_profile_attachment(state, profile, payload, content).await?)
        }
        (Some(AttachmentUploadAuthority::Message(message)), None) => json!(match message {
            ConsumedRoomHumanTicket::Local(grant) => {
                state
                    .store
                    .store_local_message_attachment(
                        &grant.room_id,
                        &grant.principal_id,
                        &grant.participant_id,
                        &payload.filename,
                        &payload.content_type,
                        content,
                    )
                    .await?
            }
            ConsumedRoomHumanTicket::HumanSession(authorization) => {
                state
                    .store
                    .store_human_session_message_attachment(
                        &authorization,
                        &payload.filename,
                        &payload.content_type,
                        content,
                    )
                    .await?
            }
        }),
        (None, Some(prejoin_authorization)) => {
            json!(
                state
                    .store
                    .store_human_prejoin_avatar(
                        &prejoin_authorization,
                        &payload.filename,
                        &payload.content_type,
                        content,
                    )
                    .await?
            )
        }
        _ => return Err(ProfileHttpError::internal()),
    })
}

async fn store_profile_attachment(
    state: &AppState,
    authority: ProfileAuthority,
    payload: &AttachmentUpload,
    content: Vec<u8>,
) -> Result<agentsassemble_persistence::ProfileAttachmentMetadata, ProfileHttpError> {
    Ok(match authority {
        ProfileAuthority::Room(principal) => {
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
        ProfileAuthority::HumanSession(authorization) => {
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
        ProfileAuthority::LocalOperator => {
            state
                .store
                .store_local_operator_profile_attachment(
                    &payload.filename,
                    &payload.content_type,
                    content,
                )
                .await?
        }
    })
}

async fn authorize_prejoin_avatar_upload(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<HumanPrejoinAvatarAuthorization, ProfileHttpError> {
    let invite = required_prejoin_header(headers, &INVITE_CREDENTIAL_HEADER).ok_or_else(|| {
        ProfileHttpError::new(
            StatusCode::UNAUTHORIZED,
            "invite_token_required",
            "x-invite-token is required for a pre-join profile upload.",
        )
    })?;
    let credential = authenticated_invite_evidence(&state.human_invite_credentials, invite)
        .map_err(|_| {
            ProfileHttpError::new(
                StatusCode::FORBIDDEN,
                "invite_invalid",
                "Invite is invalid.",
            )
        })?;
    let device = required_prejoin_header(headers, &DEVICE_CREDENTIAL_HEADER).ok_or_else(|| {
        ProfileHttpError::new(
            StatusCode::BAD_REQUEST,
            "browser_credential_invalid",
            "A canonical browser credential is required.",
        )
    })?;
    let browser_credential_fingerprint =
        fingerprint_browser_credential(device).ok_or_else(|| {
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

fn required_prejoin_header<'a>(
    headers: &'a HeaderMap,
    name: &header::HeaderName,
) -> Option<&'a str> {
    single_header(headers, name.clone())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn read_attachment(
    State(state): State<AppState>,
    Path(attachment_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    RawQuery(raw_query): RawQuery,
    request: Request,
) -> Result<Response, ProfileHttpError> {
    if attachment_id.starts_with(agentsassemble_domain::MESSAGE_ATTACHMENT_ID_PREFIX) {
        let inline_requested = match raw_query.as_deref() {
            Some("view=1") => true,
            Some("download=1") => false,
            _ => {
                return Err(ProfileHttpError::bad_request(
                    "Message attachments require the exact view or download query.",
                ));
            }
        };
        let ticket = bearer_ticket(request.headers()).ok_or_else(ProfileHttpError::unauthorized)?;
        let grant = state
            .tickets
            .consume_message_attachment_read(ticket, &attachment_id)
            .await
            .map_err(|_| ProfileHttpError::unauthorized())?;
        let attachment = match grant {
            ConsumedMessageAttachmentReadTicket::Local(grant) => {
                state
                    .store
                    .bound_message_attachment(
                        &grant.room_id,
                        &grant.principal_id,
                        &grant.participant_id,
                        &attachment_id,
                    )
                    .await?
            }
            ConsumedMessageAttachmentReadTicket::HumanSession(authorization) => {
                state
                    .store
                    .bound_human_session_message_attachment(&authorization, &attachment_id)
                    .await?
            }
        };
        let inline = inline_requested && attachment.metadata.is_image;
        return attachment_response(
            &attachment.metadata.filename,
            &attachment.metadata.content_type,
            attachment.content,
            inline,
        );
    }
    if attachment_id.starts_with(agentsassemble_domain::ROOM_APPEARANCE_ASSET_PREFIX) {
        if raw_query.as_deref() != Some(agentsassemble_domain::ROOM_APPEARANCE_REFERENCE_QUERY) {
            return Err(ProfileHttpError::bad_request(
                "Room appearance assets require the exact view query.",
            ));
        }
        let ticket = bearer_ticket(request.headers()).ok_or_else(ProfileHttpError::unauthorized)?;
        let grant = state
            .tickets
            .consume_appearance_read(ticket, &attachment_id)
            .await
            .map_err(|_| ProfileHttpError::unauthorized())?;
        let attachment = match grant {
            ConsumedAppearanceReadTicket::Pending(grant) => {
                state
                    .store
                    .pending_room_appearance_asset(&grant, &attachment_id)
                    .await?
            }
            ConsumedAppearanceReadTicket::Bound(grant) => {
                state
                    .store
                    .bound_room_appearance_asset(
                        &grant.room_id,
                        &grant.principal_id,
                        &grant.participant_id,
                        &attachment_id,
                    )
                    .await?
            }
            ConsumedAppearanceReadTicket::HumanSession(authorization) => {
                state
                    .store
                    .bound_human_session_room_appearance_asset(&authorization, &attachment_id)
                    .await?
            }
        };
        return attachment_response(
            &attachment.metadata.filename,
            &attachment.metadata.content_type,
            attachment.content,
            true,
        );
    }
    let attachment = state.store.profile_attachment(&attachment_id).await?;
    attachment_response(
        &attachment.metadata.filename,
        &attachment.metadata.content_type,
        attachment.content,
        query.contains_key("view") && !query.contains_key("download"),
    )
}

fn attachment_response(
    filename: &str,
    content_type: &str,
    content: Vec<u8>,
    inline: bool,
) -> Result<Response, ProfileHttpError> {
    let disposition = if inline { "inline" } else { "attachment" };
    let fallback_name: String = filename
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
    let mut response = Response::new(body::Body::from(content));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).map_err(|_| ProfileHttpError::internal())?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disposition}; filename=\"{fallback_name}\""))
            .map_err(|_| ProfileHttpError::internal())?,
    );
    Ok(response)
}

enum ProfileAuthority {
    Room(AuthenticatedPrincipal),
    HumanSession(HumanSessionAuthorization),
    LocalOperator,
}

enum AttachmentUploadAuthority {
    Profile(ProfileAuthority),
    Appearance(agentsassemble_persistence::LocalRoomManagerAuthority),
    Message(ConsumedRoomHumanTicket),
}

async fn consume_attachment_upload_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<AttachmentUploadAuthority, ProfileHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    match state
        .tickets
        .consume_attachment_upload(ticket)
        .await
        .map_err(|_| ProfileHttpError::unauthorized())?
    {
        ConsumedAttachmentUploadTicket::Profile(profile) => {
            profile_authority(profile).map(AttachmentUploadAuthority::Profile)
        }
        ConsumedAttachmentUploadTicket::Appearance(authority) => {
            Ok(AttachmentUploadAuthority::Appearance(authority))
        }
        ConsumedAttachmentUploadTicket::Message(authority) => {
            Ok(AttachmentUploadAuthority::Message(authority))
        }
    }
}

async fn consume_profile_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<ProfileAuthority, ProfileHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    profile_authority(
        state
            .tickets
            .consume_profile(ticket)
            .await
            .map_err(|_| ProfileHttpError::unauthorized())?,
    )
}

fn profile_authority(ticket: ConsumedProfileTicket) -> Result<ProfileAuthority, ProfileHttpError> {
    match ticket {
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
                    "attachment_missing"
                    | "appearance_asset_missing"
                    | "message_attachment_missing"
                    | "user_profile_missing" => StatusCode::NOT_FOUND,
                    "invite_invalid"
                    | "invite_revoked"
                    | "token_expired"
                    | "invite_use_limit_reached"
                    | "session_read_only"
                    | "room_authority_changed"
                    | "permission_denied" => StatusCode::FORBIDDEN,
                    "room_unavailable" | "room_inactive" => StatusCode::GONE,
                    "attachment_owner_mismatch" | "profile_authority_mismatch" => {
                        StatusCode::FORBIDDEN
                    }
                    "attachment_quota_reached" => StatusCode::TOO_MANY_REQUESTS,
                    "profile_revision_conflict" => StatusCode::CONFLICT,
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
