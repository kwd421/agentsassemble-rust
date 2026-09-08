use agentsassemble_domain::ProviderCatalog;
use agentsassemble_provider::CatalogRefreshError;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, ensure_empty_body,
        exact_tauri_cors,
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
    }
}

async fn refresh_catalog(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCatalog>, ProviderOperationHttpError> {
    consume_local_operator(&state, request.headers())
        .await
        .ok_or(ProviderOperationHttpError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use server-operator ticket is required.",
        })?;
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

struct ProviderOperationHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ProviderOperationHttpError {
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
