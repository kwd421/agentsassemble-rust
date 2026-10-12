//! Thin authenticated adapters; virtual encrypted HTTP enters these exact handlers.
use super::{ProfileHttpError, resolve_message_attachment_upload_authority};
use crate::{AppState, ticket::RoomSessionHttpAuthority};
use agentsassemble_persistence::MessageUploadAuthority;
use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Begin {
    room_id: String,
    declared_size: usize,
    filename: String,
    content_type: String,
    operation_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Offset {
    offset: usize,
}

async fn authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<MessageUploadAuthority, ProfileHttpError> {
    let (authority, _) =
        resolve_message_attachment_upload_authority(state, headers, origin).await?;
    Ok(match authority {
        RoomSessionHttpAuthority::LocalTicket(grant) => MessageUploadAuthority::Local {
            room_id: grant.room_id,
            user_id: grant.principal_id,
            participant_id: grant.participant_id,
        },
        RoomSessionHttpAuthority::Session(session) => MessageUploadAuthority::Session(session),
    })
}
pub(super) async fn begin(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ProfileHttpError> {
    let authority = authority(&state, request.headers(), request.extensions().get()).await?;
    let input: Begin = crate::http_api::decode_json_body(request, 16 * 1024)
        .await
        .map_err(ProfileHttpError::from_body)?;
    if input.room_id != authority.identity().0 {
        return Err(ProfileHttpError::new(
            StatusCode::FORBIDDEN,
            "upload_room_mismatch",
            "Upload room differs from authenticated room.",
        ));
    }
    let service = state.uploads.clone();
    let status = state
        .connections
        .spawn(async move {
            service
                .begin(
                    authority,
                    input.operation_id,
                    input.filename,
                    input.content_type,
                    input.declared_size,
                )
                .await
        })
        .await
        .map_err(|_| ProfileHttpError::internal())??;
    Ok(Json(json!({"transfer":status})))
}
pub(super) async fn data(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    Query(offset): Query<Offset>,
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: Request,
) -> Result<Json<Value>, ProfileHttpError> {
    let authority = authority(&state, request.headers(), request.extensions().get()).await?;
    let _connection_wait = request
        .extensions()
        .get::<crate::http_admission::HttpConnectionAdmission>()
        .map(|connection| connection.retain_authenticated_wait());
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        != Some("application/octet-stream")
    {
        return Err(ProfileHttpError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "upload_binary_required",
            "Upload data requires application/octet-stream.",
        ));
    }
    let status = state
        .uploads
        .data(
            authority,
            handle,
            offset.offset,
            request.into_body(),
            policy.0,
        )
        .await?;
    Ok(Json(json!({"transfer":status})))
}
pub(super) async fn status(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    request: Request,
) -> Result<Json<Value>, ProfileHttpError> {
    let authority = authority(&state, request.headers(), request.extensions().get()).await?;
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let status = state.uploads.status(authority, handle).await?;
    Ok(Json(json!({"transfer":status})))
}
pub(super) async fn finish(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    request: Request,
) -> Result<Json<Value>, ProfileHttpError> {
    let authority = authority(&state, request.headers(), request.extensions().get()).await?;
    let _connection_wait = request
        .extensions()
        .get::<crate::http_admission::HttpConnectionAdmission>()
        .map(|connection| connection.retain_authenticated_wait());
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let service = state.uploads.clone();
    let status = state
        .connections
        .spawn(async move { service.finish(authority, handle).await })
        .await
        .map_err(|_| ProfileHttpError::internal())??;
    Ok(Json(json!({"transfer":status})))
}
pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    request: Request,
) -> Result<Json<Value>, ProfileHttpError> {
    let authority = authority(&state, request.headers(), request.extensions().get()).await?;
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let service = state.uploads.clone();
    state
        .connections
        .spawn(async move { service.cancel(authority, handle).await })
        .await
        .map_err(|_| ProfileHttpError::internal())??;
    Ok(Json(json!({"cancelled":true})))
}
