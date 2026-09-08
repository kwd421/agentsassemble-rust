use agentsassemble_persistence::{
    ATTENDEE_SESSION_PREFIX, AttendeeToolReadRequest, AttendeeToolReadResult,
};
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::{AttendeeHttpError, connection_id};
use crate::{
    AppState, AttendeeToolReadResponse,
    http_api::{decode_json_body, purpose_bearer_fingerprint},
};

pub(super) async fn read(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<AttendeeToolReadResponse>, AttendeeHttpError> {
    let fingerprint = purpose_bearer_fingerprint(request.headers(), ATTENDEE_SESSION_PREFIX)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection = connection_id(request.headers())?;
    let body: AttendeeToolReadRequest = decode_json_body(request, 16384).await?;
    let result = state
        .store
        .read_attendee_tool(&fingerprint, connection, &body, chrono::Utc::now())
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    Ok(Json(match result {
        AttendeeToolReadResult::SearchMessages(page) => {
            AttendeeToolReadResponse::SearchMessages { result: page }
        }
        AttendeeToolReadResult::MessageContext(context) => {
            AttendeeToolReadResponse::MessageContext { result: context }
        }
        AttendeeToolReadResult::Attachment(attachment) => AttendeeToolReadResponse::Attachment {
            metadata: attachment.metadata,
            content_base64: STANDARD.encode(attachment.content),
        },
    }))
}

pub(super) async fn random(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let fingerprint = purpose_bearer_fingerprint(request.headers(), ATTENDEE_SESSION_PREFIX)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let connection_id = connection_id(request.headers())?;
    let session = state
        .store
        .authorize_attendee_session(&fingerprint, chrono::Utc::now())
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    let body = decode_json_body(request, 16384).await?;
    let result = state
        .rooms
        .execute_attendee(crate::AttendeeOperation::Random {
            session,
            connection_id,
            request: Box::new(body),
        })
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    let crate::AttendeeOperationResult::Random {
        result,
        deduplicated,
    } = result
    else {
        return Err(AttendeeHttpError::rejected(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attendee_result_mismatch",
        ));
    };
    Ok(Json(
        json!({"resolution":"committed", "kind":"random", "result":result, "deduplicated":deduplicated}),
    ))
}
