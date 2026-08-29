use agentsassemble_domain::MAX_ATTACHMENT_BYTES;
use agentsassemble_persistence::{
    PersistenceError, PersonaImportError, import_ccv3_asset, import_charx_asset, import_risum_asset,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::OnceLock;
use tokio::sync::Semaphore;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, MAX_BASE64_ENCODED_BYTES, MAX_BASE64_UPLOAD_BODY_BYTES, PRIVATE_NO_STORE,
        consume_local_operator, decode_json_body, ensure_empty_body, exact_tauri_cors,
    },
};

const MAX_EMPTY_BODY_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonaImportRequest {
    filename: String,
    data_base64: String,
}

pub(crate) fn routes() -> Router<AppState> {
    persona_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn persona_routes<AppState>() {
        private "/api/personas" => get(list_personas),
        private "/api/personas/import" => post(import_persona),
        private "/api/personas/{persona_id}/thumbnail" => get(persona_thumbnail),
    }
}

async fn list_personas(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, PersonaHttpError> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_EMPTY_BODY_BYTES)
        .await
        .map_err(PersonaHttpError::from_body)?;
    Ok(Json(json!({"items": state.store.persona_assets().await?})))
}

async fn import_persona(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, PersonaHttpError> {
    authorize(&state, request.headers()).await?;
    let permit = import_admission()
        .acquire()
        .await
        .map_err(|_| PersonaHttpError::internal())?;
    let payload: PersonaImportRequest = decode_json_body(request, MAX_BASE64_UPLOAD_BODY_BYTES)
        .await
        .map_err(PersonaHttpError::from_body)?;
    let filename = clean_filename(&payload.filename).ok_or_else(PersonaHttpError::unsupported)?;
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(
        extension.as_str(),
        "json" | "png" | "apng" | "charx" | "risum"
    ) {
        return Err(PersonaHttpError::unsupported());
    }
    let encoded = payload.data_base64;
    let store = state.store.clone();
    let task = tokio::spawn(async move {
        let _permit = permit;
        let content = tokio::task::spawn_blocking(move || decode_upload(&encoded))
            .await
            .map_err(|_| PersonaHttpError::worker_failed())??;
        let imported = match extension.as_str() {
            "charx" => import_charx_asset(&filename, content).await?,
            "risum" => import_risum_asset(&filename, content).await?,
            "json" | "png" | "apng" => import_ccv3_asset(&filename, content).await?,
            _ => unreachable!("extension was validated above"),
        };
        store
            .replace_persona_asset(imported)
            .await
            .map_err(PersonaHttpError::from)
    });
    let persona = task
        .await
        .map_err(|_| PersonaHttpError::worker_failed())??;
    Ok(Json(json!({"persona": persona})))
}

fn import_admission() -> &'static Semaphore {
    static ADMISSION: OnceLock<Semaphore> = OnceLock::new();
    ADMISSION.get_or_init(|| Semaphore::new(2))
}

async fn persona_thumbnail(
    State(state): State<AppState>,
    Path(persona_id): Path<String>,
    request: Request,
) -> Result<Response, PersonaHttpError> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_EMPTY_BODY_BYTES)
        .await
        .map_err(PersonaHttpError::from_body)?;
    let content = state.store.persona_thumbnail(&persona_id).await?;
    let mut response = Response::new(Body::from(content));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"thumbnail.png\""),
    );
    Ok(response)
}

async fn authorize(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), PersonaHttpError> {
    consume_local_operator(state, headers)
        .await
        .ok_or_else(PersonaHttpError::unauthorized)?;
    Ok(())
}

fn clean_filename(value: &str) -> Option<String> {
    value
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .filter(|filename| !filename.is_empty())
        .map(str::to_owned)
}

fn decode_upload(value: &str) -> Result<Vec<u8>, PersonaHttpError> {
    let mut encoded = value.trim();
    if encoded.is_empty() {
        return Err(PersonaHttpError::bad_request(
            "Persona file data is required.",
        ));
    }
    if encoded
        .as_bytes()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"data:"))
        && let Some((_, remainder)) = encoded.split_once(',')
    {
        encoded = remainder;
    }
    if encoded.len() > MAX_BASE64_ENCODED_BYTES {
        return Err(PersonaHttpError::bad_request("Persona file is too large."));
    }
    let content = STANDARD
        .decode(encoded)
        .map_err(|_| PersonaHttpError::bad_request("Persona file data is invalid."))?;
    if content.is_empty() {
        return Err(PersonaHttpError::bad_request(
            "Persona file data is required.",
        ));
    }
    if content.len() > MAX_ATTACHMENT_BYTES {
        return Err(PersonaHttpError::bad_request("Persona file is too large."));
    }
    Ok(content)
}

#[derive(Debug)]
struct PersonaHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl PersonaHttpError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "persona_import_invalid",
            message: message.into(),
        }
    }

    fn unsupported() -> Self {
        Self::bad_request("Supported persona files are .json, .png, .apng, .charx, and .risum.")
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use server-operator ticket is required.".to_owned(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Persona request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Persona request body exceeds the route limit.".to_owned(),
            },
            BodyDecodeError::InvalidJson => Self::bad_request("Request JSON is invalid."),
            BodyDecodeError::NonEmpty => {
                Self::bad_request("GET persona requests must not contain a body.")
            }
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "persona_not_found",
            message: "Persona thumbnail is unavailable.".to_owned(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "persistence_failed",
            message: "Persistence operation failed.".to_owned(),
        }
    }

    fn worker_failed() -> Self {
        tracing::error!("persona import task failed");
        Self::internal()
    }
}

impl From<PersonaImportError> for PersonaHttpError {
    fn from(error: PersonaImportError) -> Self {
        if error == PersonaImportError::ProcessingUnavailable {
            Self::worker_failed()
        } else {
            Self::bad_request(error.to_string())
        }
    }
}

impl From<PersistenceError> for PersonaHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::PersonaAssetMissing => Self::not_found(),
            internal => {
                tracing::error!(error = ?internal, "persona HTTP persistence operation failed");
                Self::internal()
            }
        }
    }
}

impl IntoResponse for PersonaHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
