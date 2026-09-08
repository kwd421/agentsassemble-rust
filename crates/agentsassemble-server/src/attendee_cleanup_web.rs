use agentsassemble_persistence::{
    ATTENDEE_SESSION_PREFIX, AttendeeCleanupAuthorization, AttendeeCleanupReport,
};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};

use super::AttendeeHttpError;
use crate::{
    AppState, AttendeeOperation, AttendeeOperationResult,
    http_api::{decode_json_body, purpose_bearer_fingerprint},
};

pub(super) async fn read(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AttendeeHttpError> {
    let authority = authorize(&state, &headers).await?;
    let stop = state
        .store
        .load_attendee_cleanup(&authority)
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    Ok(Json(json!({"stop":stop})))
}

pub(super) async fn report(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let authority = authorize(&state, request.headers()).await?;
    let report: AttendeeCleanupReport = decode_json_body(request, 4096).await?;
    let result = state
        .rooms
        .execute_attendee(AttendeeOperation::Cleanup {
            authority,
            report: Box::new(report),
        })
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    acknowledge(result)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaveRequest {
    request_id: uuid::Uuid,
}

pub(super) async fn leave(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let authority = authorize(&state, request.headers()).await?;
    let body: LeaveRequest = decode_json_body(request, 1024).await?;
    let result = state
        .rooms
        .execute_attendee(AttendeeOperation::Leave {
            authority,
            request_id: body.request_id,
        })
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    acknowledge(result)
}

pub(super) fn acknowledge(
    result: AttendeeOperationResult,
) -> Result<Json<Value>, AttendeeHttpError> {
    let AttendeeOperationResult::Reported {
        event_id,
        sequence,
        deduplicated,
    } = result
    else {
        return Err(AttendeeHttpError::rejected(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attendee_result_mismatch",
        ));
    };
    Ok(Json(
        json!({"resolution":"committed", "event_id":event_id, "sequence":sequence, "deduplicated":deduplicated}),
    ))
}

pub(super) async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AttendeeCleanupAuthorization, AttendeeHttpError> {
    let fingerprint =
        purpose_bearer_fingerprint(headers, ATTENDEE_SESSION_PREFIX).ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    state
        .store
        .authorize_attendee_cleanup(&fingerprint)
        .await
        .map_err(AttendeeHttpError::from_persistence)
}
