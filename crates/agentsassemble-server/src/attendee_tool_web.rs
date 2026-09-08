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
    AppState,
    http_api::{decode_json_body, purpose_bearer_fingerprint},
};

pub(super) async fn read(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AttendeeHttpError> {
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
            json!({"kind":"search_messages", "result":page})
        }
        AttendeeToolReadResult::MessageContext(context) => {
            json!({"kind":"message_context", "result":context})
        }
        AttendeeToolReadResult::Attachment(attachment) => {
            json!({"kind":"attachment", "metadata":attachment.metadata, "content_base64":STANDARD.encode(attachment.content)})
        }
    }))
}
