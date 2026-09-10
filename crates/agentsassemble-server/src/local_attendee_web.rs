//! Local-operator entry points for the independently owned attendee service.
use agentsassemble_domain::{
    LocalAttendeeAction, LocalAttendeeCommand, LocalAttendeeCreate, LocalAttendeeStatus,
};
use agentsassemble_provider::ProviderAdapter;
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use crate::{
    AppState, LocalAttendeeError,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, decode_json_body,
        ensure_empty_body, exact_tauri_cors,
    },
};

registered_routes! {
    fn attendee_routes<AppState>() {
        private "/api/local-attendees" => post(create),
        private "/api/local-attendees/{request_id}" => get(status).post(command),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    attendee_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

async fn create(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<LocalAttendeeStatus>, HttpError> {
    authorize(&state, request.headers()).await?;
    let input: LocalAttendeeCreate = decode_json_body(request, 16 * 1024).await?;
    let adapter = match &state.runtime_state_root {
        Some(root) => ProviderAdapter::with_credentials_and_state_root(
            state.provider_credentials.clone(),
            root,
        ),
        None => ProviderAdapter::with_credentials(state.provider_credentials.clone()),
    };
    state
        .local_attendees
        .create(input, state.provider_catalog.clone(), adapter)
        .await
        .map(Json)
        .map_err(HttpError::from)
}

async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Json<LocalAttendeeStatus>, HttpError> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, 4096).await?;
    state
        .local_attendees
        .status(request_id(&id)?)
        .await
        .map(Json)
        .map_err(HttpError::from)
}

async fn command(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Json<LocalAttendeeStatus>, HttpError> {
    authorize(&state, request.headers()).await?;
    let id = request_id(&id)?;
    let input: LocalAttendeeCommand = decode_json_body(request, 4096).await?;
    let result = match input.action {
        LocalAttendeeAction::RetryAdmission => state.local_attendees.retry(id).await,
        LocalAttendeeAction::Start => state.local_attendees.start(id).await,
        LocalAttendeeAction::Cancel => state.local_attendees.cancel(id).await,
    };
    result.map(Json).map_err(HttpError::from)
}

fn request_id(value: &str) -> Result<Uuid, HttpError> {
    Uuid::parse_str(value).map_err(|_| HttpError {
        status: StatusCode::BAD_REQUEST,
        code: "invalid_local_attendee_request".to_owned(),
        message: None,
    })
}

async fn authorize(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), HttpError> {
    consume_local_operator(state, headers)
        .await
        .ok_or_else(|| HttpError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized".to_owned(),
            message: None,
        })?;
    Ok(())
}

struct HttpError {
    status: StatusCode,
    code: String,
    message: Option<String>,
}

impl From<BodyDecodeError> for HttpError {
    fn from(error: BodyDecodeError) -> Self {
        let status = match error {
            BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
        };
        Self {
            status,
            code: "invalid_local_attendee_request".to_owned(),
            message: None,
        }
    }
}

impl From<LocalAttendeeError> for HttpError {
    fn from(error: LocalAttendeeError) -> Self {
        let status = if error.code == "local_attendee_missing" {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::CONFLICT
        };
        Self {
            status,
            code: error.code,
            message: error.message,
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error":{"code":self.code,"message":self.message.as_deref().unwrap_or("이 PC의 참가 작업 결과를 확인하지 못했어요. 상태를 다시 확인해 주세요.")}}))).into_response()
    }
}
