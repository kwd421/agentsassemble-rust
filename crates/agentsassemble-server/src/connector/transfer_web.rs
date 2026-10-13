//! Connector file transfers reuse the host upload owner under connector session custody.
use agentsassemble_persistence::{
    CONNECTOR_SESSION_PREFIX, ConnectorFileAuthority, MESSAGE_UPLOAD_PIECE_BYTES,
    MessageAttachmentReadAuthority, MessageUploadAuthority,
};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::header,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AppState, ConnectorHttpError};
use crate::http_api::{decode_json_body, purpose_bearer_fingerprint};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Begin {
    filename: String,
    content_type: String,
    declared_size: usize,
    operation_id: String,
}

#[derive(Deserialize)]
pub(super) struct Offset {
    offset: usize,
}

async fn upload_authority(
    state: &AppState,
    fingerprint: &[u8; 32],
) -> Result<MessageUploadAuthority, ConnectorHttpError> {
    let authorization = state
        .store
        .authorize_connector_session(fingerprint, chrono::Utc::now())
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let principal = authorization.principal();
    Ok(MessageUploadAuthority::Connector(Box::new(
        ConnectorFileAuthority {
            session_fingerprint: *fingerprint,
            room_id: principal.room_id.clone(),
            participant_id: principal.participant_id.clone(),
        },
    )))
}

fn session_fingerprint(headers: &header::HeaderMap) -> Result<[u8; 32], ConnectorHttpError> {
    purpose_bearer_fingerprint(headers, CONNECTOR_SESSION_PREFIX)
        .ok_or_else(ConnectorHttpError::unauthorized)
}

pub(super) async fn begin(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = session_fingerprint(request.headers())?;
    let input: Begin = decode_json_body(request, 16 * 1024).await?;
    let authority = upload_authority(&state, &fingerprint).await?;
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
        .map_err(|_| ConnectorHttpError::invalid())?
        .map_err(ConnectorHttpError::from_profile)?;
    Ok(Json(json!({"transfer": status})))
}

pub(super) async fn data(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    Query(offset): Query<Offset>,
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: axum::extract::Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = session_fingerprint(request.headers())?;
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        != Some("application/octet-stream")
    {
        return Err(ConnectorHttpError::invalid());
    }
    let authority = upload_authority(&state, &fingerprint).await?;
    let status = state
        .uploads
        .data(
            authority,
            handle,
            offset.offset,
            request.into_body(),
            policy.0,
        )
        .await
        .map_err(ConnectorHttpError::from_profile)?;
    Ok(Json(json!({"transfer": status})))
}

pub(super) async fn finish(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    request: axum::extract::Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = session_fingerprint(request.headers())?;
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(ConnectorHttpError::from)?;
    // The authority is re-resolved inside the upload owner, so the fingerprint
    // alone re-establishes custody; no body parameters select it.
    let authority = upload_authority(&state, &fingerprint).await?;
    let service = state.uploads.clone();
    let status = state
        .connections
        .spawn(async move { service.finish(authority, handle).await })
        .await
        .map_err(|_| ConnectorHttpError::invalid())?
        .map_err(ConnectorHttpError::from_profile)?;
    Ok(Json(json!({"transfer": status})))
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    request: axum::extract::Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let fingerprint = session_fingerprint(request.headers())?;
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(ConnectorHttpError::from)?;
    let authority = upload_authority(&state, &fingerprint).await?;
    let service = state.uploads.clone();
    state
        .connections
        .spawn(async move { service.cancel(authority, handle).await })
        .await
        .map_err(|_| ConnectorHttpError::invalid())?
        .map_err(ConnectorHttpError::from_profile)?;
    Ok(Json(json!({"cancelled": true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Download {
    attachment_id: String,
}

pub(super) async fn download(
    State(state): State<AppState>,
    Query(query): Query<Download>,
    headers: header::HeaderMap,
) -> Result<axum::response::Response, ConnectorHttpError> {
    use axum::response::IntoResponse;
    let fingerprint = session_fingerprint(&headers)?;
    // Small files keep their inline representation; only larger published files stream.
    // The history charge is the per-call read budget shared with the inline path.
    let authorization = super::read::authorize_read_fingerprint(&state, &fingerprint, 1).await?;
    let principal = authorization.principal();
    let inline =
        super::attachments::read_authorized(&state, &authorization, &query.attachment_id).await;
    let writable = principal.capabilities.message_send;
    match inline {
        Ok((value, lease)) => {
            return crate::download_budget::encoded_response(&value, Some(lease))
                .map_err(|_| ConnectorHttpError::invalid());
        }
        Err(error) if error.code == "attachment_too_large" && writable => {}
        Err(error) => return Err(error),
    }
    let authority = ConnectorFileAuthority {
        session_fingerprint: fingerprint,
        room_id: principal.room_id.clone(),
        participant_id: principal.participant_id.clone(),
    };
    // The stream lease is the byte budget: reserved at open, before any file
    // byte flows, against the shared per-principal and host download caps.
    let (reader, lease) = state
        .store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Connector(Box::new(authority)),
            &query.attachment_id,
            |principal| state.downloads.reserve_stream(principal),
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let metadata = reader.metadata().clone();
    let pieces = metadata.size.div_ceil(MESSAGE_UPLOAD_PIECE_BYTES);
    let encoded = serde_json::to_vec(&metadata).map_err(|_| ConnectorHttpError::invalid())?;
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::HeaderName::from_static("x-attachment-metadata"),
        STANDARD
            .encode(encoded)
            .parse()
            .map_err(|_| ConnectorHttpError::invalid())?,
    );
    let stream = futures_util::stream::unfold(
        (reader, 0usize, pieces, Some(lease)),
        |(reader, sequence, total, lease)| async move {
            let Some(lease) = lease else { return None };
            if sequence >= total {
                return None;
            }
            match reader.read_piece(sequence).await {
                Ok(piece) => Some((
                    Ok::<_, axum::Error>(Bytes::from(piece)),
                    (reader, sequence + 1, total, Some(lease)),
                )),
                Err(error) => Some((Err(axum::Error::new(error)), (reader, total, total, None))),
            }
        },
    );
    Ok((headers, Body::from_stream(stream)).into_response())
}
