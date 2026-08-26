use agentsassemble_domain::InviteScope;
use agentsassemble_persistence::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanAdmissionInputError, HumanAdmissionRejection,
    HumanAdmissionResult, HumanInvitePreflight, HumanInvitePreflightContext,
    HumanInvitePreflightPerson, HumanInvitePreflightRejection, PersistenceError,
    PreparedHumanAdmission,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer};

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_ticket, decode_json_body, exact_tauri_cors,
    },
    human_browser_credential::fingerprint_browser_credential,
    human_invite_preflight::{
        HumanInvitePreflightError, authenticated_invite_evidence, preflight_human_invite,
    },
};

const MAX_ADMISSION_BODY_BYTES: usize = 16 * 1024;
const DEVICE_CREDENTIAL_HEADER: HeaderName = HeaderName::from_static("x-device-token");

#[derive(Deserialize)]
struct PreflightRequest {
    invite_token: String,
}

#[derive(Deserialize)]
struct JoinRequest {
    invite_token: String,
    request_id: String,
    #[serde(default)]
    meeting_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    avatar_image_url: String,
    #[serde(default)]
    device_token: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    participant_type: String,
    #[serde(default)]
    owner_display_name: String,
}

#[derive(Serialize)]
struct JoinResponse {
    #[serde(flatten)]
    result: HumanAdmissionResult,
    session_token: String,
}

pub(crate) fn routes() -> Router<AppState> {
    invite_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(invite_cors())
}

registered_routes! {
    fn invite_routes<AppState>() {
        "/api/room-invite/admission" => post(preflight),
        "/api/room-invite/join" => post(join),
    }
}

fn invite_cors() -> CorsLayer {
    exact_tauri_cors([Method::POST]).allow_headers([
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        DEVICE_CREDENTIAL_HEADER,
    ])
}

async fn preflight(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let browser_credential = required_header(request.headers(), &DEVICE_CREDENTIAL_HEADER)?;
    if fingerprint_browser_credential(browser_credential).is_none() {
        return Err(HumanInviteHttpError::bad_request(
            "browser_credential_invalid",
            "A canonical browser credential is required.",
        ));
    }
    let browser_credential = browser_credential.to_owned();
    let session_bearer = bearer_ticket(request.headers());
    if session_bearer.is_some_and(|bearer| {
        crate::human_session_bearer::fingerprint_presented_bearer(bearer).is_none()
    }) {
        return Err(HumanInviteHttpError::unauthorized(
            "session_invalid",
            "The room session bearer is invalid.",
        ));
    }
    let session_bearer = session_bearer.map(str::to_owned);
    let payload: PreflightRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let decision = match preflight_human_invite(
        &state.store,
        &state.human_invite_credentials,
        &payload.invite_token,
        &browser_credential,
        session_bearer.as_deref(),
        Utc::now(),
    )
    .await
    {
        Ok(decision) => decision,
        Err(HumanInvitePreflightError::InviteCredential(_)) => {
            return Ok(Json(rejected_preflight("invite_invalid", "invite_invalid")));
        }
        Err(HumanInvitePreflightError::BrowserCredential) => {
            return Err(HumanInviteHttpError::bad_request(
                "browser_credential_invalid",
                "A canonical browser credential is required.",
            ));
        }
        Err(HumanInvitePreflightError::SessionBearer) => {
            return Err(HumanInviteHttpError::unauthorized(
                "session_invalid",
                "The room session bearer is invalid.",
            ));
        }
        Err(HumanInvitePreflightError::Persistence(error)) => return Err(error.into()),
    };
    Ok(Json(preflight_payload(decision)))
}

async fn join(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<JoinResponse>, HumanInviteHttpError> {
    let payload: JoinRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    if payload.invite_token.trim().is_empty() {
        return Err(HumanInviteHttpError::bad_request(
            "invite_token_required",
            "invite_token is required.",
        ));
    }
    if payload.request_id.trim().is_empty() {
        return Err(HumanInviteHttpError::bad_request(
            "request_id_required",
            "request_id is required.",
        ));
    }
    let credential =
        authenticated_invite_evidence(&state.human_invite_credentials, payload.invite_token.trim())
            .map_err(|_| HumanInviteHttpError::forbidden("invite_invalid", "Invite is invalid."))?;
    let browser_credential_fingerprint = fingerprint_browser_credential(&payload.device_token)
        .ok_or_else(|| {
            HumanInviteHttpError::bad_request(
                "browser_credential_invalid",
                "A canonical browser credential is required.",
            )
        })?;
    let prepared = PreparedHumanAdmission::prepare(
        credential,
        browser_credential_fingerprint,
        &HumanAdmissionInput {
            request_id: payload.request_id,
            meeting_id_assertion: payload.meeting_id,
            display_name: payload.display_name,
            participant_type: payload.participant_type,
            owner_display_name: payload.owner_display_name,
            client_id: payload.client_id,
            avatar_image_url: payload.avatar_image_url,
        },
    )
    .map_err(HumanInviteHttpError::from_input)?;
    match state.rooms.admit_human(prepared).await? {
        HumanAdmissionDecision::Admitted(commit) => {
            let (result, session_token) = commit.into_result_and_bearer();
            Ok(Json(JoinResponse {
                result,
                session_token,
            }))
        }
        HumanAdmissionDecision::Rejected(rejection) => Err(rejection.into()),
    }
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<&'a str, HumanInviteHttpError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            HumanInviteHttpError::bad_request(
                "browser_credential_invalid",
                "A canonical browser credential is required.",
            )
        })
}

fn preflight_payload(decision: HumanInvitePreflight) -> Value {
    match decision {
        HumanInvitePreflight::Rejected(rejection) => {
            let (status, reason) = match rejection {
                HumanInvitePreflightRejection::InviteExpired => ("invite_expired", "token_expired"),
                HumanInvitePreflightRejection::InviteNotFound => {
                    ("invite_invalid", "invite_invalid")
                }
                HumanInvitePreflightRejection::InviteRevoked => {
                    ("invite_invalid", "invite_revoked")
                }
                HumanInvitePreflightRejection::InviteUseLimitReached => {
                    ("invite_invalid", "invite_use_limit_reached")
                }
                HumanInvitePreflightRejection::RoomUnavailable => {
                    ("invite_invalid", "room_unavailable")
                }
                HumanInvitePreflightRejection::SessionUnavailable => {
                    ("invite_invalid", "admission_session_unavailable")
                }
            };
            rejected_preflight(status, reason)
        }
        HumanInvitePreflight::ProfileRequired(context) => {
            preflight_context("profile_required", false, &context, None)
        }
        HumanInvitePreflight::ExistingSession { context, person } => {
            preflight_context("existing_session", true, &context, Some(person))
        }
        HumanInvitePreflight::KnownUser { context, person } => {
            preflight_context("known_user", true, &context, Some(person))
        }
        HumanInvitePreflight::ExistingMember { context, person } => {
            preflight_context("existing_member", true, &context, Some(person))
        }
    }
}

fn rejected_preflight(status: &str, reason: &str) -> Value {
    json!({"status": status, "reason": reason, "can_auto_join": false})
}

fn preflight_context(
    status: &str,
    can_auto_join: bool,
    context: &HumanInvitePreflightContext,
    person: Option<HumanInvitePreflightPerson>,
) -> Value {
    let mut payload = json!({
        "status": status,
        "can_auto_join": can_auto_join,
        "room_id": context.room_id,
        "room_label": context.room_label,
        "invite_scope": invite_scope_text(context.invite_scope),
    });
    if let Some(person) = person {
        payload["participant"] = json!({
            "participant_id": person.participant_id,
            "display_name": person.display_name,
            "avatar_image_url": person.avatar_image_url,
        });
        payload["operator"] = Value::Bool(person.operator);
    }
    payload
}

const fn invite_scope_text(scope: InviteScope) -> &'static str {
    match scope {
        InviteScope::ReadWrite => "room",
        InviteScope::ReadOnly => "read_only",
    }
}

#[derive(Debug)]
struct HumanInviteHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl HumanInviteHttpError {
    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }

    fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
            ),
            BodyDecodeError::PayloadTooLarge => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "Request body exceeds the route limit.",
            ),
            BodyDecodeError::InvalidJson => {
                Self::bad_request("bad_request", "Request JSON is invalid.")
            }
            BodyDecodeError::NonEmpty => {
                Self::bad_request("bad_request", "Request body is invalid.")
            }
        }
    }

    fn from_input(error: HumanAdmissionInputError) -> Self {
        match error {
            HumanAdmissionInputError::RequestId => Self::bad_request(
                "request_id_invalid",
                "request_id must be a canonical non-zero UUID.",
            ),
            HumanAdmissionInputError::ParticipantType => Self::bad_request(
                "participant_type_invalid",
                "Browser admission requires a human participant type.",
            ),
        }
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "persistence_failed",
            "Admission persistence is unavailable.",
        )
    }
}

impl From<HumanAdmissionRejection> for HumanInviteHttpError {
    fn from(rejection: HumanAdmissionRejection) -> Self {
        match rejection {
            HumanAdmissionRejection::InviteNotFound => {
                Self::forbidden("invite_invalid", "Invite is invalid.")
            }
            HumanAdmissionRejection::InviteRevoked => {
                Self::forbidden("invite_revoked", "Invite was revoked.")
            }
            HumanAdmissionRejection::InviteExpired => {
                Self::forbidden("token_expired", "Invite has expired.")
            }
            HumanAdmissionRejection::InviteUseLimitReached => {
                Self::forbidden("invite_use_limit_reached", "Invite use limit was reached.")
            }
            HumanAdmissionRejection::RoomUnavailable => Self::new(
                StatusCode::GONE,
                "room_unavailable",
                "Room was deleted or is unavailable.",
            ),
            HumanAdmissionRejection::MeetingMismatch => Self::forbidden(
                "meeting_mismatch",
                "Invite room does not match the request.",
            ),
            HumanAdmissionRejection::IdentityConflict => Self::forbidden(
                "participant_identity_conflict",
                "The invited identity conflicts with an existing participant.",
            ),
            HumanAdmissionRejection::CapacityReached => Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "room_admission_capacity_reached",
                "Room admission capacity was reached.",
            ),
            HumanAdmissionRejection::SessionUnavailable => Self::forbidden(
                "admission_session_unavailable",
                "The completed admission session is unavailable.",
            ),
            HumanAdmissionRejection::IdempotencyConflict => Self::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "request_id was already used with different admission inputs.",
            ),
        }
    }
}

impl From<PersistenceError> for HumanInviteHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::CommandRejected {
                code: "room_busy", ..
            } => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "room_busy",
                "Room mutation queue is full.",
            ),
            PersistenceError::CommandRejected {
                code: "room_unavailable",
                ..
            }
            | PersistenceError::RoomMissing => Self::new(
                StatusCode::GONE,
                "room_unavailable",
                "Room was deleted or is unavailable.",
            ),
            internal => {
                tracing::error!(error = ?internal, "human invite HTTP persistence failed");
                Self::internal()
            }
        }
    }
}

impl IntoResponse for HumanInviteHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_persistence::HumanAdmissionRejection;
    use axum::http::StatusCode;

    use super::{BodyDecodeError, HumanInviteHttpError};

    #[test]
    fn public_error_mapping_preserves_timeout_and_identity_collision() {
        let timeout = HumanInviteHttpError::from_body(BodyDecodeError::RequestTimeout);
        assert_eq!(timeout.status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(timeout.code, "request_timeout");

        let collision = HumanInviteHttpError::from(HumanAdmissionRejection::IdentityConflict);
        assert_eq!(collision.status, StatusCode::FORBIDDEN);
        assert_eq!(collision.code, "participant_identity_conflict");
    }
}
