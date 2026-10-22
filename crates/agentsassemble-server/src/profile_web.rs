use agentsassemble_domain::{InviteScope, LOCAL_OPERATOR_USER_ID, UserProfilePatch};
use agentsassemble_persistence::{HumanSessionAuthorization, PersistenceError};
use axum::{
    Json, Router, body,
    extract::{Path, Query, RawQuery, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer};

use crate::{
    AppState, ConsumedProfileTicket,
    http_api::{
        BodyDecodeError, DEVICE_CREDENTIAL_HEADER, MAX_BASE64_UPLOAD_BODY_BYTES, PRIVATE_NO_STORE,
        bearer_credential, decode_json_body, decode_upload_body, ensure_empty_body,
        exact_tauri_cors,
    },
    room_session_http_authority::{
        RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
    },
    ticket::{
        ConsumedAppearanceReadTicket, ConsumedAttachmentUploadTicket, RoomSessionHttpAuthority,
    },
};

mod agent_avatar;
mod avatar_admission;
#[cfg(test)]
mod avatar_admission_tests;
mod google_profile;
#[path = "message_upload_http.rs"]
mod transfer;

const MAX_PROFILE_BODY_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct AttachmentUpload {
    purpose: String,
    filename: String,
    content_type: String,
    data_base64: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EncodedAttachmentUpload {
    pub(crate) filename: String,
    pub(crate) content_type: String,
    pub(crate) data_base64: String,
}

#[derive(Deserialize)]
struct ProfileUpdateRequest {
    expected_revision: i64,
    initial_google_profile: Option<google_profile::InitialGoogleProfile>,
    #[serde(flatten)]
    patch: UserProfilePatch,
}

pub(crate) fn routes() -> Router<AppState> {
    profile_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-agentsassemble-upload-transfer"),
            HeaderValue::from_static("1"),
        ))
        .layer(profile_cors())
}

registered_routes! {
    fn profile_routes<AppState>() {
        secure_remote "/api/agent-avatars/upload/{session_id}" => post(agent_avatar::upload),
        secure_remote "/api/agent-avatars/{asset_id}" => get(agent_avatar::read),
        secure_remote "/api/user-profile" => get(read_profile).post(update_profile),
        secure_remote "/api/attachments" => post(upload_attachment),
        secure_remote "/api/message-attachment-transfers" => post(transfer::begin),
        secure_remote "/api/message-attachment-transfers/{handle}" => get(transfer::status).delete(transfer::cancel),
        secure_remote "/api/message-attachment-transfers/{handle}/data" => post(transfer::data),
        secure_remote "/api/message-attachment-transfers/{handle}/finish" => post(transfer::finish),
        secure_remote "/api/message-attachments" => post(upload_message_attachment),
        secure_remote "/api/attachments/{attachment_id}" => get(read_attachment),
    }
}

fn profile_cors() -> CorsLayer {
    exact_tauri_cors([Method::GET, Method::POST, Method::DELETE]).allow_headers([
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        DEVICE_CREDENTIAL_HEADER,
    ])
}

async fn read_profile(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let authority =
        resolve_profile_authority(&state, request.headers(), request.extensions().get()).await?;
    ensure_empty_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let profile = match authority {
        ProfileAuthority::HumanSession(authorization) => {
            state.store.human_session_profile(&authorization).await?
        }
        ProfileAuthority::LocalOperator => state.store.local_operator_profile().await?,
        ProfileAuthority::CentralOwner(session) => {
            state.store.server_owner_profile(&session).await?
        }
    };
    Ok(Json(json!({"profile": profile})))
}

async fn update_profile(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let authority =
        resolve_profile_authority(&state, request.headers(), request.extensions().get()).await?;
    let update: ProfileUpdateRequest = decode_json_body(request, MAX_PROFILE_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    if let Some(profile) = update.initial_google_profile {
        if !matches!(authority, ProfileAuthority::LocalOperator) || update.expected_revision != 1 {
            return Err(ProfileHttpError::unauthorized());
        }
        return google_profile::initialize(&state, profile).await;
    }
    let outcome = match authority {
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
        ProfileAuthority::CentralOwner(session) => {
            state
                .store
                .update_server_owner_profile(&session, update.expected_revision, update.patch)
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
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let authority =
        resolve_attachment_upload_authority(&state, request.headers(), request.extensions().get())
            .await?;
    if matches!(
        &authority,
        AttachmentUploadAuthority::Profile(ProfileAuthority::HumanSession(authorization))
            if authorization.principal().invite_scope == InviteScope::ReadOnly
    ) {
        return Err(ProfileHttpError::new(
            StatusCode::FORBIDDEN,
            "session_read_only",
            "Read-only room sessions cannot upload profile avatars.",
        ));
    }
    let permit = avatar_admission::acquire(&state.avatar_uploads)?;
    let principal = match &authority {
        AttachmentUploadAuthority::Profile(ProfileAuthority::HumanSession(session)) => {
            session.principal().principal_id.as_str()
        }
        AttachmentUploadAuthority::Operator(session) => session.principal().principal_id.as_str(),
        AttachmentUploadAuthority::Appearance(
            agentsassemble_persistence::RoomManagerAuthority::Local(binding),
        ) => &binding.manager.user_id,
        AttachmentUploadAuthority::Appearance(
            agentsassemble_persistence::RoomManagerAuthority::Operator(session),
        ) => &session.principal().principal_id,
        AttachmentUploadAuthority::Profile(
            ProfileAuthority::CentralOwner(_) | ProfileAuthority::LocalOperator,
        ) => agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
    };
    let (payload, reservation): (AttachmentUpload, _) = decode_upload_body(
        request,
        MAX_BASE64_UPLOAD_BODY_BYTES,
        &policy,
        &state.upload_bodies,
        principal,
    )
    .await?;
    let authority = match authority {
        AttachmentUploadAuthority::Operator(session)
            if payload.purpose.trim() == "profile_avatar" && session.is_central_owner() =>
        {
            AttachmentUploadAuthority::Profile(ProfileAuthority::CentralOwner(
                agentsassemble_persistence::ServerOwnerAuthority::CentralOwner(session),
            ))
        }
        AttachmentUploadAuthority::Operator(session) => AttachmentUploadAuthority::Appearance(
            agentsassemble_persistence::RoomManagerAuthority::Operator(session),
        ),
        other => other,
    };
    match (&authority, payload.purpose.trim()) {
        (AttachmentUploadAuthority::Appearance(_), "room_appearance")
        | (AttachmentUploadAuthority::Profile(_), "profile_avatar") => {}
        (_, _) => {
            return Err(ProfileHttpError::bad_request(
                "Attachment purpose does not match its authority.",
            ));
        }
    }
    let attachment = state
        .connections
        .clone()
        .spawn(async move {
            let _reservation = reservation;
            let _permit = permit;
            let content = decode_attachment_content(&payload.data_base64)?;
            store_uploaded_attachment(&state, authority, &payload, content).await
        })
        .await
        .map_err(|_| ProfileHttpError::internal())??;
    Ok(Json(json!({"attachment": attachment})))
}

async fn upload_message_attachment(
    State(state): State<AppState>,
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let (authority, file_limit) = resolve_message_attachment_upload_authority(
        &state,
        request.headers(),
        request.extensions().get(),
    )
    .await?;
    let principal = match &authority {
        RoomSessionHttpAuthority::LocalTicket(grant) => &grant.principal_id,
        RoomSessionHttpAuthority::Session(session) => &session.principal().principal_id,
    };
    let (payload, reservation): (EncodedAttachmentUpload, _) = decode_upload_body(
        request,
        crate::http_api::base64_upload_body_limit(file_limit),
        &policy,
        &state.upload_bodies,
        principal,
    )
    .await?;
    let attachment = state
        .connections
        .clone()
        .spawn(async move {
            let _reservation = reservation;
            if payload.data_base64.trim().len() > file_limit.div_ceil(3) * 4 {
                return Err(ProfileHttpError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "attachment_too_large",
                    format!("Message attachment exceeds the room limit of {file_limit} bytes."),
                ));
            }
            let content = decode_attachment_content(&payload.data_base64)?;
            if content.is_empty() || content.len() > file_limit {
                return Err(ProfileHttpError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "attachment_too_large",
                    format!("Message attachment exceeds the room limit of {file_limit} bytes."),
                ));
            }
            let attachment = match authority {
                RoomSessionHttpAuthority::LocalTicket(grant) => {
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
                RoomSessionHttpAuthority::Session(authorization) => {
                    state
                        .store
                        .store_authorized_message_attachment(
                            authorization.mutation_authority(),
                            &payload.filename,
                            &payload.content_type,
                            content,
                        )
                        .await?
                }
            };
            Ok::<_, ProfileHttpError>(attachment)
        })
        .await
        .map_err(|_| ProfileHttpError::internal())??;
    Ok(Json(json!({"attachment": attachment})))
}

fn decode_attachment_content(encoded: &str) -> Result<Vec<u8>, ProfileHttpError> {
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Err(ProfileHttpError::bad_request("data_base64 is required."));
    }
    STANDARD
        .decode(encoded)
        .map_err(|_| ProfileHttpError::bad_request("data_base64 is invalid."))
}

async fn store_uploaded_attachment(
    state: &AppState,
    authority: AttachmentUploadAuthority,
    payload: &AttachmentUpload,
    content: Vec<u8>,
) -> Result<serde_json::Value, ProfileHttpError> {
    Ok(match authority {
        AttachmentUploadAuthority::Operator(_) => return Err(ProfileHttpError::unauthorized()),
        AttachmentUploadAuthority::Appearance(manager) => json!(
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
        AttachmentUploadAuthority::Profile(profile) => {
            json!(store_profile_attachment(state, profile, payload, content).await?)
        }
    })
}

async fn store_profile_attachment(
    state: &AppState,
    authority: ProfileAuthority,
    payload: &AttachmentUpload,
    content: Vec<u8>,
) -> Result<agentsassemble_persistence::ProfileAttachmentMetadata, ProfileHttpError> {
    Ok(match authority {
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
        ProfileAuthority::CentralOwner(session) => {
            state
                .store
                .store_server_owner_profile_attachment(
                    &session,
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

async fn read_attachment(
    State(state): State<AppState>,
    Path(attachment_id): Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    RawQuery(raw_query): RawQuery,
    request: Request,
) -> Result<Response, ProfileHttpError> {
    if attachment_id.starts_with(agentsassemble_domain::MESSAGE_ATTACHMENT_ID_PREFIX) {
        use agentsassemble_persistence::MessageAttachmentReadAuthority;
        let inline_requested = attachment_inline_query(raw_query.as_deref())?;
        let preview_requested = raw_query.as_deref() == Some("preview=1");
        let authority = resolve_message_attachment_read_authority(
            &state,
            request.headers(),
            request.extensions().get(),
            &attachment_id,
        )
        .await?;
        let authority = match authority {
            RoomSessionHttpAuthority::LocalTicket(grant) => MessageAttachmentReadAuthority::Local {
                room_id: grant.room_id,
                user_id: grant.principal_id,
                participant_id: grant.participant_id,
            },
            RoomSessionHttpAuthority::Session(session) => {
                MessageAttachmentReadAuthority::Session(session)
            }
        };
        let (reader, lease) = if preview_requested {
            state
                .store
                .open_message_attachment_preview_reader(
                    authority,
                    &attachment_id,
                    |principal| state.downloads.reserve_stream(principal),
                    |principal, size| state.downloads.reserve_preview_generation(principal, size),
                )
                .await?
        } else {
            state
                .store
                .open_message_attachment_reader(authority, &attachment_id, |principal| {
                    state.downloads.reserve_stream(principal)
                })
                .await?
        };
        let metadata = reader.metadata().clone();
        let (body, progress) =
            crate::attachment_download::body(reader, lease, request.extensions().get());
        let mut response = attachment_response(
            &metadata.filename,
            &metadata.content_type,
            body,
            inline_requested && metadata.is_image,
        )?;
        response.headers_mut().insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&metadata.size.to_string())
                .map_err(|_| ProfileHttpError::internal())?,
        );
        response.extensions_mut().insert(progress);
        return Ok(response);
    }

    if attachment_id.starts_with(agentsassemble_domain::ROOM_APPEARANCE_ASSET_PREFIX) {
        if raw_query.as_deref() != Some(agentsassemble_domain::ROOM_APPEARANCE_REFERENCE_QUERY) {
            return Err(ProfileHttpError::bad_request(
                "Room appearance assets require the exact view query.",
            ));
        }
        let authority = resolve_appearance_read_authority(
            &state,
            request.headers(),
            request.extensions().get(),
            &attachment_id,
        )
        .await?;
        let (attachment, lease) = match authority {
            AppearanceReadAuthority::Local(ConsumedAppearanceReadTicket::Pending(grant)) => {
                state
                    .store
                    .pending_room_appearance_asset(&grant, &attachment_id, |principal, size| {
                        state.downloads.reserve(Some(principal), size)
                    })
                    .await?
            }
            AppearanceReadAuthority::Local(ConsumedAppearanceReadTicket::Bound(grant)) => {
                state
                    .store
                    .bound_room_appearance_asset(
                        &grant.room_id,
                        &grant.principal_id,
                        &grant.participant_id,
                        &attachment_id,
                        |principal, size| state.downloads.reserve(Some(principal), size),
                    )
                    .await?
            }
            AppearanceReadAuthority::Session(authorization) => {
                state
                    .store
                    .bound_room_session_room_appearance_asset(
                        &authorization,
                        &attachment_id,
                        |principal, size| state.downloads.reserve(Some(principal), size),
                    )
                    .await?
            }
        };
        return attachment_response(
            &attachment.metadata.filename,
            &attachment.metadata.content_type,
            lease.retain(attachment.content),
            true,
        );
    }
    let permit = avatar_admission::acquire(&state.avatar_reads)?;
    let (attachment, lease) = state
        .store
        .profile_attachment(&attachment_id, |size| state.downloads.reserve(None, size))
        .await?;
    avatar_admission::response(
        &attachment.metadata.filename,
        &attachment.metadata.content_type,
        lease.retain(attachment.content),
        query.contains_key("view") && !query.contains_key("download"),
        permit,
    )
}

fn attachment_inline_query(query: Option<&str>) -> Result<bool, ProfileHttpError> {
    match query {
        Some("view=1" | "preview=1") => Ok(true),
        Some("download=1") => Ok(false),
        _ => Err(ProfileHttpError::bad_request(
            "Message attachments require the exact view, preview or download query.",
        )),
    }
}

fn attachment_response(
    filename: &str,
    content_type: &str,
    content: impl Into<body::Body>,
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
    let mut response = Response::new(content.into());
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
    CentralOwner(agentsassemble_persistence::ServerOwnerAuthority),
    HumanSession(HumanSessionAuthorization),
    LocalOperator,
}

enum AttachmentUploadAuthority {
    Operator(Box<agentsassemble_persistence::OperatorSessionAuthorization>),
    Profile(ProfileAuthority),
    Appearance(agentsassemble_persistence::RoomManagerAuthority),
}

enum AppearanceReadAuthority {
    Local(ConsumedAppearanceReadTicket),
    Session(Box<agentsassemble_persistence::RoomSessionAuthorization>),
}

async fn resolve_appearance_read_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
    asset_id: &str,
) -> Result<AppearanceReadAuthority, ProfileHttpError> {
    let credential = bearer_credential(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(AppearanceReadAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_appearance_read(credential, asset_id)
            .await
            .map(AppearanceReadAuthority::Local)
            .map_err(|_| ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn resolve_message_attachment_upload_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<(RoomSessionHttpAuthority, usize), ProfileHttpError> {
    let credential = bearer_credential(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            let limit = state
                .store
                .authorize_room_session_message_attachment_upload(&authorization)
                .await?;
            Ok((RoomSessionHttpAuthority::Session(authorization), limit))
        }
        Ok(RoomSessionBearerResolution::Other) => {
            let grant = state
                .tickets
                .consume_message_attachment_upload(credential)
                .await
                .map_err(|_| ProfileHttpError::unauthorized())?;
            let limit = state
                .store
                .authorize_local_message_attachment_upload(
                    &grant.room_id,
                    &grant.principal_id,
                    &grant.participant_id,
                )
                .await?;
            Ok((RoomSessionHttpAuthority::LocalTicket(grant), limit))
        }
        Err(RoomSessionBearerError::Invalid) => Err(ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn resolve_message_attachment_read_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
    attachment_id: &str,
) -> Result<RoomSessionHttpAuthority, ProfileHttpError> {
    let credential = bearer_credential(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomSessionHttpAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_message_attachment_read(credential, attachment_id)
            .await
            .map(RoomSessionHttpAuthority::LocalTicket)
            .map_err(|_| ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn resolve_attachment_upload_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<AttachmentUploadAuthority, ProfileHttpError> {
    let credential = bearer_credential(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    if credential.starts_with(agentsassemble_persistence::OWNER_SESSION_PREFIX) {
        let owner = crate::central::owner_web::owner_from_session_headers(state, headers, origin)
            .await
            .map_err(|error| ProfileHttpError::new(error.status, error.code, error.message))?;
        return Ok(AttachmentUploadAuthority::Profile(
            ProfileAuthority::CentralOwner(owner),
        ));
    }
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            return Ok(match *authorization {
                agentsassemble_persistence::RoomSessionAuthorization::Human(session) => {
                    AttachmentUploadAuthority::Profile(ProfileAuthority::HumanSession(session))
                }
                agentsassemble_persistence::RoomSessionAuthorization::Operator(session) => {
                    AttachmentUploadAuthority::Operator(Box::new(session))
                }
            });
        }
        Ok(RoomSessionBearerResolution::Other) => {}
        Err(RoomSessionBearerError::Invalid) => return Err(ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => return Err(error.into()),
    }
    match state
        .tickets
        .consume_attachment_upload(credential)
        .await
        .map_err(|_| ProfileHttpError::unauthorized())?
    {
        ConsumedAttachmentUploadTicket::Profile(profile) => {
            profile_authority(profile).map(AttachmentUploadAuthority::Profile)
        }
        ConsumedAttachmentUploadTicket::Appearance(authority) => {
            Ok(AttachmentUploadAuthority::Appearance(
                agentsassemble_persistence::RoomManagerAuthority::Local(authority),
            ))
        }
    }
}

async fn resolve_profile_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<ProfileAuthority, ProfileHttpError> {
    let credential = bearer_credential(headers).ok_or_else(ProfileHttpError::unauthorized)?;
    if credential.starts_with(agentsassemble_persistence::OWNER_SESSION_PREFIX) {
        let owner = crate::central::owner_web::owner_from_session_headers(state, headers, origin)
            .await
            .map_err(|error| ProfileHttpError::new(error.status, error.code, error.message))?;
        return Ok(ProfileAuthority::CentralOwner(owner));
    }
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            return match *authorization {
                agentsassemble_persistence::RoomSessionAuthorization::Human(session) => {
                    Ok(ProfileAuthority::HumanSession(session))
                }
                agentsassemble_persistence::RoomSessionAuthorization::Operator(session)
                    if session.is_central_owner() =>
                {
                    Ok(ProfileAuthority::CentralOwner(
                        agentsassemble_persistence::ServerOwnerAuthority::CentralOwner(Box::new(
                            session,
                        )),
                    ))
                }
                agentsassemble_persistence::RoomSessionAuthorization::Operator(_) => {
                    Err(ProfileHttpError::unauthorized())
                }
            };
        }
        Ok(RoomSessionBearerResolution::Other) => {}
        Err(RoomSessionBearerError::Invalid) => return Err(ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => return Err(error.into()),
    }
    profile_authority(
        state
            .tickets
            .consume_profile(credential)
            .await
            .map_err(|_| ProfileHttpError::unauthorized())?,
    )
}

fn profile_authority(ticket: ConsumedProfileTicket) -> Result<ProfileAuthority, ProfileHttpError> {
    match ticket {
        ConsumedProfileTicket::ServerOperator { principal_id }
            if principal_id == LOCAL_OPERATOR_USER_ID =>
        {
            Ok(ProfileAuthority::LocalOperator)
        }
        ConsumedProfileTicket::ServerOperator { .. } => Err(ProfileHttpError::unauthorized()),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub(crate) struct ProfileHttpError {
    // Boxed: most errors carry no extra fields, and the error rides in every Result.
    extra: Box<serde_json::Map<String, serde_json::Value>>,
    status: StatusCode,
    pub(crate) code: std::borrow::Cow<'static, str>,
    pub(crate) message: String,
}

impl ProfileHttpError {
    pub(crate) fn new(
        status: StatusCode,
        code: impl Into<std::borrow::Cow<'static, str>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            extra: Box::default(),
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub(crate) fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                extra: Box::default(),
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout".into(),
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
            extra: Box::default(),
            status: StatusCode::BAD_REQUEST,
            code: "bad_request".into(),
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            extra: Box::default(),
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized".into(),
            message: "Valid request authority is required.".to_owned(),
        }
    }

    fn payload_too_large() -> Self {
        Self {
            extra: Box::default(),
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large".into(),
            message: "Request body exceeds the route limit.".to_owned(),
        }
    }

    pub(crate) fn with_defer(mut self, milliseconds: u64) -> Self {
        self.extra.insert("disposition".into(), json!("defer"));
        self.extra
            .insert("retry_after_ms".into(), json!(milliseconds));
        self
    }
    pub(crate) fn with_offset(mut self, offset: usize) -> Self {
        self.extra.insert("accepted_offset".into(), json!(offset));
        self
    }
    fn internal() -> Self {
        Self {
            extra: Box::default(),
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "persistence_failed".into(),
            message: "Persistence operation failed.".to_owned(),
        }
    }

    /// Moves the wire rejection triple to another transport owner without new fallbacks.
    pub(crate) fn into_http_parts(self) -> (StatusCode, std::borrow::Cow<'static, str>, String) {
        (self.status, self.code, self.message)
    }
}

impl From<PersistenceError> for ProfileHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected { code, message } => {
                let status = match code.as_bytes() {
                    b"session_revoked" | b"central_connect_invalid" => StatusCode::UNAUTHORIZED,
                    b"attachment_missing"
                    | b"attachment_not_found"
                    | b"appearance_asset_missing"
                    | b"message_attachment_missing"
                    | b"user_profile_missing" => StatusCode::NOT_FOUND,
                    b"invite_invalid"
                    | b"invite_revoked"
                    | b"token_expired"
                    | b"invite_use_limit_reached"
                    | b"session_read_only"
                    | b"room_authority_changed"
                    | b"permission_denied" => StatusCode::FORBIDDEN,
                    b"room_unavailable" | b"room_inactive" => StatusCode::GONE,
                    b"attachment_owner_mismatch" | b"profile_authority_mismatch" => {
                        StatusCode::FORBIDDEN
                    }
                    b"pairing_capacity"
                    | b"attachment_quota_reached"
                    | b"room_attachment_quota_reached"
                    | b"attachment_download_budget_exhausted" => StatusCode::TOO_MANY_REQUESTS,
                    b"profile_revision_conflict" => StatusCode::CONFLICT,
                    b"attachment_too_large" => StatusCode::PAYLOAD_TOO_LARGE,
                    b"attachment_type_unsupported"
                    | b"attachment_type_mismatch"
                    | b"attachment_invalid_image"
                    | b"attachment_image_limits" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    b"invalid_state" | b"agent_avatar_custody_invalid" => {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                    _ => StatusCode::BAD_REQUEST,
                };
                Self {
                    extra: Box::default(),
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
        let mut payload = serde_json::Map::new();
        payload.insert("error".into(), self.message.into());
        payload.insert("code".into(), self.code.into());
        payload.extend(*self.extra);
        (self.status, Json(payload)).into_response()
    }
}

impl From<crate::upload_body_budget::Error> for ProfileHttpError {
    fn from(error: crate::upload_body_budget::Error) -> Self {
        match error {
            crate::upload_body_budget::Error::Body(error) => Self::from_body(error),
            crate::upload_body_budget::Error::Exhausted => Self {
                extra: Box::default(),
                status: StatusCode::TOO_MANY_REQUESTS,
                code: crate::upload_body_budget::ERROR_CODE.into(),
                message: "Upload body capacity is busy; retry later.".into(),
            },
        }
    }
}
