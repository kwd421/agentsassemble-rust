//! Attendee file transfers reuse the host upload owner under attendee turn custody.
use agentsassemble_persistence::{
    ATTENDEE_SESSION_PREFIX, AttendeeFileAuthority, AttendeeToolRead, AttendeeToolReadRequest,
    AttendeeToolReadResult, MESSAGE_UPLOAD_PIECE_BYTES, MessageAttachmentReadAuthority,
    MessageUploadAuthority, ProviderFileAuthority,
};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{AttendeeHttpError, connection_id};
use crate::{
    AppState, AttendeeToolReadResponse,
    http_api::{decode_json_body, purpose_bearer_fingerprint},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Begin {
    turn_generation: u64,
    execution_id: String,
    filename: String,
    content_type: String,
    declared_size: usize,
    operation_id: String,
}

#[derive(Deserialize)]
pub(super) struct Offset {
    #[serde(rename = "offset")]
    at: usize,
    turn_generation: u64,
    execution_id: String,
}

#[derive(Deserialize)]
pub(super) struct Handle {
    turn_generation: u64,
    execution_id: String,
}

#[derive(Deserialize)]
pub(super) struct Download {
    attachment_id: String,
    turn_generation: u64,
    execution_id: String,
}

async fn upload_authority(
    state: &AppState,
    fingerprint: &[u8; 32],
    connection: Uuid,
    turn_generation: u64,
    execution_id: &str,
) -> Result<MessageUploadAuthority, AttendeeHttpError> {
    let (_, session) = state
        .store
        .authorize_attendee_file_turn(fingerprint, connection, turn_generation, execution_id)
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    Ok(MessageUploadAuthority::Attendee(Box::new(
        AttendeeFileAuthority {
            room_id: session.public.room_id.clone(),
            session_id: session.public.session_id.clone(),
            participant_id: session.public.participant_id.clone(),
            session_fingerprint: *fingerprint,
            connection_id: connection,
            turn_generation,
            execution_id: execution_id.to_owned(),
        },
    )))
}

pub(super) async fn begin(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let fingerprint = purpose_bearer_fingerprint(request.headers(), ATTENDEE_SESSION_PREFIX)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection = connection_id(request.headers())?;
    let input: Begin = decode_json_body(request, 16 * 1024).await?;
    let authority = upload_authority(
        &state,
        &fingerprint,
        connection,
        input.turn_generation,
        &input.execution_id,
    )
    .await?;
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
        .map_err(|_| {
            AttendeeHttpError::rejected(
                StatusCode::SERVICE_UNAVAILABLE,
                "attendee_operation_failed",
            )
        })?
        .map_err(AttendeeHttpError::from_profile)?;
    Ok(Json(json!({"transfer": status})))
}

pub(super) async fn data(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    Query(offset): Query<Offset>,
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: axum::extract::Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let fingerprint = purpose_bearer_fingerprint(request.headers(), ATTENDEE_SESSION_PREFIX)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection = connection_id(request.headers())?;
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        != Some("application/octet-stream")
    {
        return Err(AttendeeHttpError::from_profile(
            crate::profile_web::ProfileHttpError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "upload_binary_required",
                "Upload data requires application/octet-stream.",
            ),
        ));
    }
    let authority = upload_authority(
        &state,
        &fingerprint,
        connection,
        offset.turn_generation,
        &offset.execution_id,
    )
    .await?;
    let status = state
        .uploads
        .data(authority, handle, offset.at, request.into_body(), policy.0)
        .await
        .map_err(AttendeeHttpError::from_profile)?;
    Ok(Json(json!({"transfer": status})))
}

pub(super) async fn finish(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    Query(query): Query<Handle>,
    request: axum::extract::Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let fingerprint = purpose_bearer_fingerprint(request.headers(), ATTENDEE_SESSION_PREFIX)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection = connection_id(request.headers())?;
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(AttendeeHttpError::from)?;
    let authority = upload_authority(
        &state,
        &fingerprint,
        connection,
        query.turn_generation,
        &query.execution_id,
    )
    .await?;
    let service = state.uploads.clone();
    let status = state
        .connections
        .spawn(async move { service.finish(authority, handle).await })
        .await
        .map_err(|_| {
            AttendeeHttpError::rejected(
                StatusCode::SERVICE_UNAVAILABLE,
                "attendee_operation_failed",
            )
        })?
        .map_err(AttendeeHttpError::from_profile)?;
    Ok(Json(json!({"transfer": status})))
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    Query(query): Query<Handle>,
    request: axum::extract::Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let fingerprint = purpose_bearer_fingerprint(request.headers(), ATTENDEE_SESSION_PREFIX)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection = connection_id(request.headers())?;
    crate::http_api::ensure_empty_body(request, 0)
        .await
        .map_err(AttendeeHttpError::from)?;
    let authority = upload_authority(
        &state,
        &fingerprint,
        connection,
        query.turn_generation,
        &query.execution_id,
    )
    .await?;
    let service = state.uploads.clone();
    state
        .connections
        .spawn(async move { service.cancel(authority, handle).await })
        .await
        .map_err(|_| {
            AttendeeHttpError::rejected(
                StatusCode::SERVICE_UNAVAILABLE,
                "attendee_operation_failed",
            )
        })?
        .map_err(AttendeeHttpError::from_profile)?;
    Ok(Json(json!({"cancelled": true})))
}

pub(super) async fn download(
    State(state): State<AppState>,
    Query(query): Query<Download>,
    headers: header::HeaderMap,
) -> Result<axum::response::Response, AttendeeHttpError> {
    use axum::response::IntoResponse;
    let fingerprint =
        purpose_bearer_fingerprint(&headers, ATTENDEE_SESSION_PREFIX).ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection = connection_id(&headers)?;
    // Small files keep their inline representation; only larger published files stream.
    let read = AttendeeToolReadRequest {
        turn_generation: query.turn_generation,
        execution_id: query.execution_id.clone(),
        tool: AttendeeToolRead::Attachment {
            attachment_id: query.attachment_id.clone(),
        },
    };
    let inline = state
        .store
        .read_attendee_tool(
            &fingerprint,
            connection,
            &read,
            chrono::Utc::now(),
            |principal, size| state.downloads.reserve_encoded(principal, size),
        )
        .await;
    let (_, session) = state
        .store
        .authorize_attendee_file_turn(
            &fingerprint,
            connection,
            query.turn_generation,
            &query.execution_id,
        )
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    let writable = matches!(
        session.public.permission_mode.as_str(),
        "workspace_write" | "full_access"
    );
    match inline {
        Ok((AttendeeToolReadResult::Attachment(attachment), lease)) => {
            let response = AttendeeToolReadResponse::Attachment {
                metadata: attachment.metadata,
                content_base64: STANDARD.encode(attachment.content),
            };
            return crate::download_budget::encoded_response(&response, lease).map_err(|_| {
                AttendeeHttpError::rejected(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "attendee_operation_failed",
                )
            });
        }
        Ok(_) => {
            return Err(AttendeeHttpError::rejected(
                StatusCode::INTERNAL_SERVER_ERROR,
                "attendee_result_mismatch",
            ));
        }
        Err(agentsassemble_persistence::PersistenceError::CommandRejected { ref code, .. })
            if code == "attachment_too_large" && writable => {}
        Err(error) => return Err(AttendeeHttpError::from_persistence(error)),
    }
    let authority = ProviderFileAuthority {
        room_id: session.public.room_id.clone(),
        session_id: session.public.session_id.clone(),
        participant_id: session.public.participant_id.clone(),
        turn_id: session.public.active_turn_id.clone(),
        input_up_to_seq: session.input_up_to_seq,
        turn_generation: query.turn_generation,
        execution_id: query.execution_id.clone(),
    };
    let custody = AttendeeFileAuthority {
        room_id: session.public.room_id.clone(),
        session_id: session.public.session_id.clone(),
        participant_id: session.public.participant_id.clone(),
        session_fingerprint: fingerprint,
        connection_id: connection,
        turn_generation: query.turn_generation,
        execution_id: query.execution_id.clone(),
    };
    let (reader, lease) = state
        .store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Attendee {
                attendee: Box::new(custody),
                provider: Box::new(authority),
            },
            &query.attachment_id,
            |principal| state.downloads.reserve_stream(principal),
        )
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    let metadata = reader.metadata().clone();
    let pieces = metadata.size.div_ceil(MESSAGE_UPLOAD_PIECE_BYTES);
    let encoded = serde_json::to_vec(&metadata).map_err(|_| {
        AttendeeHttpError::rejected(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attendee_operation_failed",
        )
    })?;
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::HeaderName::from_static("x-attachment-metadata"),
        STANDARD.encode(encoded).parse().map_err(|_| {
            AttendeeHttpError::rejected(
                StatusCode::INTERNAL_SERVER_ERROR,
                "attendee_operation_failed",
            )
        })?,
    );
    let stream = futures_util::stream::unfold(
        (reader, 0usize, pieces, Some(lease)),
        |(reader, sequence, total, lease)| async move {
            let lease = lease?;
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
