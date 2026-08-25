use axum::{
    body,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, header},
};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyDecodeError {
    PayloadTooLarge,
    InvalidJson,
    NonEmpty,
}

pub(crate) fn exact_tauri_cors(methods: impl IntoIterator<Item = Method>) -> CorsLayer {
    let origins = [
        HeaderValue::from_static("tauri://localhost"),
        HeaderValue::from_static("http://tauri.localhost"),
        HeaderValue::from_static("https://tauri.localhost"),
    ];
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(methods.into_iter().collect::<Vec<_>>())
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

pub(crate) fn bearer_ticket(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_whitespace()))
}

pub(crate) async fn consume_local_operator(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(ticket) = bearer_ticket(headers) else {
        return false;
    };
    state
        .tickets
        .consume_server_operator(ticket)
        .await
        .is_ok_and(|grant| grant.principal_id == agentsassemble_domain::LOCAL_OPERATOR_USER_ID)
}

pub(crate) async fn ensure_empty_body(
    request: Request,
    limit: usize,
) -> Result<(), BodyDecodeError> {
    reject_declared_oversize(request.headers(), limit)?;
    let encoded = body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| BodyDecodeError::PayloadTooLarge)?;
    if encoded.is_empty() {
        Ok(())
    } else {
        Err(BodyDecodeError::NonEmpty)
    }
}

pub(crate) async fn decode_json_body<T: serde::de::DeserializeOwned>(
    request: Request,
    limit: usize,
) -> Result<T, BodyDecodeError> {
    reject_declared_oversize(request.headers(), limit)?;
    let encoded = body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| BodyDecodeError::PayloadTooLarge)?;
    serde_json::from_slice(&encoded).map_err(|_| BodyDecodeError::InvalidJson)
}

fn reject_declared_oversize(headers: &HeaderMap, limit: usize) -> Result<(), BodyDecodeError> {
    if headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > limit)
    {
        Err(BodyDecodeError::PayloadTooLarge)
    } else {
        Ok(())
    }
}
