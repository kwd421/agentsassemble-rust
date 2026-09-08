use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, admission_client_fingerprint, decode_json_body,
        purpose_bearer_fingerprint,
    },
    room_command_result::CommandFailure,
};
use agentsassemble_persistence::{
    ATTENDEE_INVITE_PREFIX, AttendeeAdmissionRequest, PersistenceError,
};
use agentsassemble_protocol::CommandResolution;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinRequest {
    request_id: Uuid,
    client_secret: String,
    provider: String,
    display_name: String,
}

registered_routes! {
    fn attendee_routes<AppState>() {
        same_origin_public "/api/room-attendee/join" => post(join),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    attendee_routes().layer(SetResponseHeaderLayer::overriding(
        CACHE_CONTROL,
        PRIVATE_NO_STORE.clone(),
    ))
}

async fn join(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, AttendeeHttpError> {
    let invite =
        purpose_bearer_fingerprint(request.headers(), ATTENDEE_INVITE_PREFIX).ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::UNAUTHORIZED, "attendee_credential_required")
        })?;
    let body: JoinRequest = decode_json_body(request, 8192).await?;
    let client = admission_client_fingerprint(&body.client_secret).ok_or_else(|| {
        AttendeeHttpError::rejected(StatusCode::BAD_REQUEST, "invalid_attendee_request")
    })?;
    let admitted = state
        .rooms
        .admit_attendee(AttendeeAdmissionRequest {
            invite_fingerprint: &invite,
            client_fingerprint: &client,
            request_id: body.request_id,
            provider_kind: &body.provider,
            display_name: &body.display_name,
        })
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    let authority = &admitted.authorization;
    Ok(Json(json!({
        "session_bearer": admitted.session_bearer,
        "room_id": authority.principal().room_id,
        "room_uid": authority.room_uid(),
        "participant_id": authority.principal().participant_id,
        "provider_kind": authority.provider_kind(),
        "expires_at": authority.expires_at(),
        "deduplicated": admitted.deduplicated,
    })))
}

struct AttendeeHttpError {
    status: StatusCode,
    code: &'static str,
    resolution: CommandResolution,
}

impl AttendeeHttpError {
    fn rejected(status: StatusCode, code: &'static str) -> Self {
        Self {
            status,
            code,
            resolution: CommandResolution::Rejected,
        }
    }

    fn from_persistence(error: PersistenceError) -> Self {
        let failure = CommandFailure::transactional(error);
        let (status, code) = match failure.error {
            PersistenceError::CommandConflict => (StatusCode::CONFLICT, "command_conflict"),
            PersistenceError::CommandRejected { code, .. } => (StatusCode::FORBIDDEN, code),
            PersistenceError::RoomMissing | PersistenceError::ParticipantMissing => {
                (StatusCode::GONE, "room_membership_unavailable")
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "attendee_operation_failed",
            ),
        };
        Self {
            status,
            code,
            resolution: failure.resolution,
        }
    }
}

impl From<BodyDecodeError> for AttendeeHttpError {
    fn from(error: BodyDecodeError) -> Self {
        let status = match error {
            BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
        };
        Self::rejected(status, "invalid_attendee_request")
    }
}

impl IntoResponse for AttendeeHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error":{"code":self.code},"resolution":self.resolution})),
        )
            .into_response()
    }
}
