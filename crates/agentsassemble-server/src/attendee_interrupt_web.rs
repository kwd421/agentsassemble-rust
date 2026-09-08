use agentsassemble_persistence::AttendeeInterruptReport;
use axum::{
    Json,
    extract::{Request, State},
};
use serde_json::Value;

use super::{AttendeeHttpError, cleanup};
use crate::{AppState, AttendeeOperation, http_api::decode_json_body};

pub(super) async fn report(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let authority = cleanup::authorize(&state, request.headers()).await?;
    let connection_id = super::connection_id(request.headers())?;
    let report: AttendeeInterruptReport = decode_json_body(request, 8192).await?;
    let result = state
        .rooms
        .execute_attendee(AttendeeOperation::Interrupt {
            authority,
            connection_id,
            report: Box::new(report),
        })
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    cleanup::acknowledge(result)
}
