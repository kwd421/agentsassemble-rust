use agentsassemble_domain::ProviderCatalog;
use agentsassemble_provider::{CatalogRefreshError, ProviderLoginError};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
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
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn operation_routes<AppState>() {
        private "/api/provider-catalog" => get(read_catalog),
        private "/api/providers/update/check" => post(check_update),
        private "/api/providers/update/start" => post(start_update),
        private "/api/providers/usage" => post(usage),
        private "/api/providers/login" => post(login),
        private "/api/providers/login/cancel" => post(cancel_login),
    }
}

async fn read_catalog(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCatalog>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let Query(input) = Query::<CatalogReadRequest>::try_from_uri(request.uri()).map_err(|_| {
        ProviderOperationHttpError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_catalog_read",
            message: "Catalog result requires a provider and an already requested generation.",
        }
    })?;
    ensure_empty_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    match (input.provider_id, input.generation) {
        (None, None) => Ok(Json(state.provider_catalog.snapshot())),
        (Some(provider_id), Some(generation)) => state
            .provider_catalog
            .wait_for_provider(&provider_id, generation)
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
            }),
        _ => Err(ProviderOperationHttpError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_catalog_read",
            message: "Catalog result requires both provider and generation.",
        }),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogReadRequest {
    provider_id: Option<String>,
    generation: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderOperationRequest {
    provider_id: String,
}

async fn check_update(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<agentsassemble_domain::ProviderUpdate>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let input: ProviderOperationRequest = decode_json_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    state
        .provider_update
        .perform(&input.provider_id, None)
        .await
        .map(Json)
        .map_err(ProviderOperationHttpError::from_update)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderUpdateRequest {
    provider_id: String,
    expected_version: String,
}

async fn start_update(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<agentsassemble_domain::ProviderUpdate>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let input: ProviderUpdateRequest = decode_json_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    state
        .provider_update
        .perform(&input.provider_id, Some(input.expected_version))
        .await
        .map(Json)
        .map_err(ProviderOperationHttpError::from_update)
}

async fn usage(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<agentsassemble_domain::ProviderUsage>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let input: ProviderOperationRequest = decode_json_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    state
        .provider_usage
        .read(&input.provider_id)
        .await
        .map(Json)
        .map_err(ProviderOperationHttpError::from_usage)
}

async fn login(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, ProviderOperationHttpError> {
    authorize(&state, request.headers()).await?;
    let input: ProviderOperationRequest = decode_json_body(request, 4096)
        .await
        .map_err(ProviderOperationHttpError::from_body)?;
    let outcome = state
        .provider_login
        .login(&input.provider_id)
        .await
        .map_err(ProviderOperationHttpError::from_login)?;
    if outcome == agentsassemble_provider::ProviderLoginOutcome::Started {
        return Ok(Json(
            json!({"provider_id":input.provider_id, "status":"started"}),
        ));
    }
    let refreshed = state
        .provider_catalog
        .refresh_provider(&input.provider_id, true)
        .await;
    if !refreshed.is_ok_and(|catalog| {
        catalog.status == "ready"
            && catalog.providers.iter().any(|provider| {
                provider.id == input.provider_id && provider.discovery_status == "ready"
            })
    }) {
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
    let input: ProviderOperationRequest = decode_json_body(request, 4096)
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
    fn from_update(error: agentsassemble_provider::ProviderUpdateError) -> Self {
        use agentsassemble_provider::ProviderUpdateError as E;
        let (status, code, message) = match error {
            E::Unsupported => (
                StatusCode::CONFLICT,
                "provider_update_unsupported",
                "이 설치 방식의 버전 확인·업데이트는 공식 안내에서 진행해 주세요.",
            ),
            E::Missing => (
                StatusCode::CONFLICT,
                "provider_update_missing",
                "이 PC에 제공자를 먼저 설치해 주세요.",
            ),
            E::Busy => (
                StatusCode::CONFLICT,
                "provider_update_busy",
                "이 제공자의 버전 확인·업데이트 작업이 진행 중이에요.",
            ),
            E::OfferChanged => (
                StatusCode::CONFLICT,
                "provider_update_offer_changed",
                "제공되는 버전이 달라졌어요. 다시 확인한 뒤 선택해 주세요.",
            ),
            E::Unavailable => (
                StatusCode::BAD_GATEWAY,
                "provider_update_unavailable",
                "현재·최신 버전을 확인하지 못했어요. 설치된 버전은 계속 사용할 수 있어요.",
            ),
            E::InvalidResponse => (
                StatusCode::BAD_GATEWAY,
                "provider_update_invalid",
                "버전 응답을 확인할 수 없어요. 공식 안내를 확인해 주세요.",
            ),
            E::Cancelled => (
                StatusCode::CONFLICT,
                "provider_update_cancelled",
                "버전 확인이 취소됐어요.",
            ),
            E::CleanupUnconfirmed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_update_cleanup_unconfirmed",
                "버전 확인 프로세스의 종료를 확인하지 못했어요.",
            ),
            E::HandoffUnconfirmed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_update_handoff_unconfirmed",
                "업데이트 터미널이 열렸을 수 있어요. 다시 시도하기 전에 확인해 주세요.",
            ),
        };
        Self {
            status,
            code,
            message,
        }
    }

    fn from_usage(error: agentsassemble_provider::ProviderUsageError) -> Self {
        use agentsassemble_provider::ProviderUsageError as E;
        let (status, code, message) = match error {
            E::Unsupported => (
                StatusCode::BAD_REQUEST,
                "provider_usage_unsupported",
                "This provider does not support account usage.",
            ),
            E::Missing => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_usage_missing",
                "Configure the provider account before reading usage.",
            ),
            E::CredentialUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_usage_credential_unavailable",
                "The credential store is unavailable.",
            ),
            E::Authentication => (
                StatusCode::BAD_GATEWAY,
                "provider_usage_authentication",
                "The provider rejected account usage authorization.",
            ),
            E::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "provider_usage_timeout",
                "Provider usage timed out.",
            ),
            E::Cancelled => (
                StatusCode::CONFLICT,
                "provider_usage_cancelled",
                "Provider usage was cancelled.",
            ),
            E::Unavailable => (
                StatusCode::BAD_GATEWAY,
                "provider_usage_unavailable",
                "Provider usage is unavailable.",
            ),
            E::InvalidResponse => (
                StatusCode::BAD_GATEWAY,
                "provider_usage_invalid_response",
                "The provider returned invalid usage data.",
            ),
            E::CleanupUnconfirmed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_usage_cleanup_unconfirmed",
                "Provider usage process cleanup could not be confirmed.",
            ),
        };
        Self {
            status,
            code,
            message,
        }
    }
    const fn from_login(error: ProviderLoginError) -> Self {
        let (status, code, message) = match error {
            ProviderLoginError::Unsupported => (
                StatusCode::BAD_REQUEST,
                "provider_login_unsupported",
                "This provider does not support local login.",
            ),
            ProviderLoginError::HandoffUnconfirmed => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_login_handoff_unconfirmed",
                "The terminal may have opened. Check it and refresh the catalog before retrying.",
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
