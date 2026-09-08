use agentsassemble_domain::ProviderCatalog;
use agentsassemble_provider::{CatalogRefreshError, ProviderLoginError};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, decode_json_body,
        ensure_empty_body, exact_tauri_cors,
    },
};

pub(crate) fn routes() -> Router<AppState> {
    operation_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]))
}

registered_routes! {
    fn operation_routes<AppState>() {
        private "/api/provider-catalog/refresh" => post(refresh_catalog),
        private "/api/providers/login" => post(login),
        private "/api/providers/login/cancel" => post(cancel_login),
    }
}

async fn refresh_catalog(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCatalog>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    state
        .provider_catalog
        .refresh()
        .await
        .map(Json)
        .map_err(|error| match error {
            CatalogRefreshError::Unsupported => ProviderOperationHttpError {
                status: StatusCode::CONFLICT,
                code: "catalog_refresh_unsupported",
                message: "This runtime does not own catalog discovery.",
            },
            CatalogRefreshError::Unavailable => ProviderOperationHttpError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "catalog_refresh_unavailable",
                message: "Provider catalog discovery is unavailable.",
            },
        })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    provider_id: String,
}

async fn login(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let input: LoginRequest = decode_json_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    state
        .provider_login
        .login(&input.provider_id)
        .await
        .map_err(ProviderOperationHttpError::from_login)?;
    let refreshed = state.provider_catalog.refresh().await;
    if !refreshed.is_ok_and(|catalog| catalog.status == "ready") {
        return Err(ProviderOperationHttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "login_completed_catalog_unavailable",
            message: "Login completed, but catalog refresh failed. Refresh the catalog again.",
        });
    }
    Ok(Json(
        json!({"provider_id": input.provider_id, "status": "authenticated"}),
    ))
}

async fn cancel_login(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let input: LoginRequest = decode_json_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    let cancelled = state
        .provider_login
        .cancel(&input.provider_id)
        .await
        .map_err(ProviderOperationHttpError::from_login)?;
    Ok(Json(
        json!({"provider_id": input.provider_id, "status": if cancelled { "cancelled" } else { "not_running" }}),
    ))
}

async fn authorize(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), ProviderOperationHttpError> {
    consume_local_operator(state, headers)
        .await
        .ok_or(ProviderOperationHttpError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use server-operator ticket is required.",
        })?;
    Ok(())
}

struct ProviderOperationHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ProviderOperationHttpError {
    const fn from_login(error: ProviderLoginError) -> Self {
        let (status, code, message) = match error {
            ProviderLoginError::Unsupported => (
                StatusCode::BAD_REQUEST,
                "provider_login_unsupported",
                "This provider does not support local login.",
            ),
            ProviderLoginError::Missing => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_login_missing",
                "Install the provider CLI before logging in.",
            ),
            ProviderLoginError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "provider_login_timeout",
                "Provider login timed out.",
            ),
            ProviderLoginError::Cancelled => (
                StatusCode::CONFLICT,
                "provider_login_cancelled",
                "Provider login was cancelled.",
            ),
            ProviderLoginError::Failed => (
                StatusCode::BAD_GATEWAY,
                "provider_login_failed",
                "Provider login failed.",
            ),
            ProviderLoginError::CleanupUnconfirmed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_login_cleanup_unconfirmed",
                "Provider login process cleanup could not be confirmed.",
            ),
        };
        Self {
            status,
            code,
            message,
        }
    }
    const fn from_body(error: BodyDecodeError) -> Self {
        let status = match error {
            BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => StatusCode::BAD_REQUEST,
        };
        Self {
            status,
            code: "provider_operation_invalid",
            message: "Provider operation request body is invalid.",
        }
    }
}

impl IntoResponse for ProviderOperationHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
