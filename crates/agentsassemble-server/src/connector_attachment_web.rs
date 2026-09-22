//! Connector attachments share the browser's storage, validation and pending custody.
use super::{AppState, ConnectorHttpError, credential};
use crate::http_api::{MAX_BASE64_ENCODED_BYTES, MAX_BASE64_UPLOAD_BODY_BYTES, decode_json_body};
use agentsassemble_persistence::{CONNECTOR_SESSION_PREFIX, RoomMutationAuthority};
use axum::{
    Json,
    extract::{Query, Request, State},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttachmentQuery {
    attachment_id: String,
}

pub(super) async fn read(
    State(state): State<AppState>,
    Query(query): Query<AttachmentQuery>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = super::read::authorize_read(&state, request, 1).await?;
    let attachment = state
        .store
        .bound_authorized_message_attachment(
            RoomMutationAuthority::ConnectorSession(&authorization),
            &query.attachment_id,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    Ok(Json(json!(
        crate::provider_attachment_runtime::into_provider_attachment(attachment)
    )))
}

pub(super) async fn upload(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = credential(&request, CONNECTOR_SESSION_PREFIX)?;
    let authorization = state
        .store
        .authorize_connector_session(&fingerprint, chrono::Utc::now())
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    if !authorization.principal().capabilities.message_send {
        return Err(ConnectorHttpError::unauthorized());
    }
    let payload: crate::profile_web::EncodedAttachmentUpload =
        decode_json_body(request, MAX_BASE64_UPLOAD_BODY_BYTES).await?;
    if payload.data_base64.len() > MAX_BASE64_ENCODED_BYTES {
        return Err(ConnectorHttpError::invalid());
    }
    let content = STANDARD
        .decode(payload.data_base64.trim())
        .map_err(|_| ConnectorHttpError::invalid())?;
    let attachment = state
        .store
        .store_authorized_message_attachment(
            RoomMutationAuthority::ConnectorSession(&authorization),
            &payload.filename,
            &payload.content_type,
            content,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    Ok(Json(json!({"attachment":attachment})))
}
