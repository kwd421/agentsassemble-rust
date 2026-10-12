//! Connector attachments share the browser's storage, validation and pending custody.
use super::{AppState, ConnectorHttpError, credential};
use crate::http_api::{MAX_BASE64_ENCODED_BYTES, MAX_BASE64_UPLOAD_BODY_BYTES, decode_upload_body};
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
) -> Result<axum::response::Response, ConnectorHttpError> {
    let authorization = super::read::authorize_read(&state, request, 1).await?;
    let (value, lease) = read_authorized(&state, &authorization, &query.attachment_id).await?;
    crate::download_budget::encoded_response(&value, Some(lease))
        .map_err(|error| ConnectorHttpError::from_persistence(error.into()))
}

pub(in crate::connector) async fn read_authorized(
    state: &AppState,
    authorization: &agentsassemble_persistence::ConnectorSessionAuthorization,
    attachment_id: &str,
) -> Result<
    (
        agentsassemble_provider::ProviderAttachment,
        crate::download_budget::Lease,
    ),
    ConnectorHttpError,
> {
    let (attachment, lease) = state
        .store
        .bound_authorized_message_attachment(
            RoomMutationAuthority::ConnectorSession(authorization),
            attachment_id,
            |principal, size| state.downloads.reserve_encoded(principal, size),
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    Ok((
        crate::provider_attachment_runtime::into_provider_attachment(attachment),
        lease,
    ))
}

pub(super) async fn upload(
    State(state): State<AppState>,
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = credential(&request, CONNECTOR_SESSION_PREFIX)?;
    let authorization = authorize_upload(&state, &fingerprint).await?;
    let (payload, reservation): (crate::profile_web::EncodedAttachmentUpload, _) =
        decode_upload_body(
            request,
            MAX_BASE64_UPLOAD_BODY_BYTES,
            &policy,
            &state.upload_bodies,
            &authorization.principal().principal_id,
        )
        .await?;
    store_upload(state, authorization, payload, reservation).await
}

pub(in crate::connector) async fn upload_parsed(
    state: AppState,
    fingerprint: [u8; 32],
    payload: crate::profile_web::EncodedAttachmentUpload,
    admission: Option<&crate::connector::mcp_web::RequestAdmission>,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = authorize_upload(&state, &fingerprint).await?;
    // AI transfers remain capped at 10 MiB, at or below every current room
    // policy (10/25/50 MiB). Persistence rechecks that policy at insertion.
    let length = payload
        .data_base64
        .len()
        .saturating_add(payload.filename.len())
        .saturating_add(payload.content_type.len());
    let (_, reservation) = state.upload_bodies.reserve_length(
        Some(&authorization.principal().principal_id),
        length,
        MAX_BASE64_UPLOAD_BODY_BYTES,
    )?;
    if let Some(admission) = admission {
        admission.release_envelope();
    }
    store_upload(state, authorization, payload, reservation).await
}

async fn authorize_upload(
    state: &AppState,
    fingerprint: &[u8; 32],
) -> Result<agentsassemble_persistence::ConnectorSessionAuthorization, ConnectorHttpError> {
    let authorization = state
        .store
        .authorize_connector_session(fingerprint, chrono::Utc::now())
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    if !authorization.principal().capabilities.message_send {
        return Err(ConnectorHttpError::unauthorized());
    }
    Ok(authorization)
}

async fn store_upload(
    state: AppState,
    authorization: agentsassemble_persistence::ConnectorSessionAuthorization,
    payload: crate::profile_web::EncodedAttachmentUpload,
    reservation: crate::upload_body_budget::Reservation,
) -> Result<Json<Value>, ConnectorHttpError> {
    let attachment = state
        .connections
        .clone()
        .spawn(async move {
            let _reservation = reservation;
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
            Ok::<_, ConnectorHttpError>(attachment)
        })
        .await
        .map_err(|_| ConnectorHttpError {
            message: None,
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            code: "connector_operation_failed".into(),
            resolution: agentsassemble_protocol::CommandResolution::Unresolved,
        })??;
    Ok(Json(json!({"attachment":attachment})))
}
