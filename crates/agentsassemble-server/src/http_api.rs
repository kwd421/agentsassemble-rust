use axum::{
    body,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, header},
};
use std::num::NonZeroU64;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::AppState;

pub(crate) const PRIVATE_NO_STORE: HeaderValue = HeaderValue::from_static("private, no-store");
pub(crate) const TAURI_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyDecodeError {
    RequestTimeout,
    PayloadTooLarge,
    InvalidJson,
    NonEmpty,
}

pub(crate) fn exact_tauri_cors(methods: impl IntoIterator<Item = Method>) -> CorsLayer {
    let origins = TAURI_ORIGINS.map(HeaderValue::from_static);
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

pub(crate) async fn consume_local_operator(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<NonZeroU64> {
    let ticket = bearer_ticket(headers)?;
    state
        .tickets
        .consume_server_operator(ticket)
        .await
        .ok()
        .filter(|grant| grant.principal_id == agentsassemble_domain::LOCAL_OPERATOR_USER_ID)
        .map(|grant| grant.issue_sequence)
}

pub(crate) async fn consume_central_registration(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(ticket) = bearer_ticket(headers) else {
        return false;
    };
    state
        .tickets
        .consume_central_registration(ticket)
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
        .map_err(|error| classify_body_read_error(&error))?;
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
        .map_err(|error| classify_body_read_error(&error))?;
    serde_json::from_slice(&encoded).map_err(|_| BodyDecodeError::InvalidJson)
}

fn classify_body_read_error(error: &axum::Error) -> BodyDecodeError {
    let mut cause: &(dyn std::error::Error + 'static) = error;
    loop {
        if cause.is::<tower_http::timeout::TimeoutError>() {
            return BodyDecodeError::RequestTimeout;
        }
        let Some(source) = cause.source() else {
            return BodyDecodeError::PayloadTooLarge;
        };
        cause = source;
    }
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

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, time::Duration};

    use axum::{
        body::{Body, Bytes},
        extract::Request,
    };
    use futures_util::stream;
    use serde_json::Value;
    use tower_http::timeout::DeadlineBody;

    use super::{BodyDecodeError, decode_json_body};

    #[tokio::test]
    async fn body_deadline_remains_distinct_from_length_limit() {
        let stalled = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        let deadline = Body::new(DeadlineBody::new(Duration::from_millis(1), stalled));
        let timed_out = decode_json_body::<Value>(Request::new(deadline), 16).await;
        assert_eq!(timed_out, Err(BodyDecodeError::RequestTimeout));

        let oversized = decode_json_body::<Value>(Request::new(Body::from("0123456789")), 4).await;
        assert_eq!(oversized, Err(BodyDecodeError::PayloadTooLarge));
    }
}
