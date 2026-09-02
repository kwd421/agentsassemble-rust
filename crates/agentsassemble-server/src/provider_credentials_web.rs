use agentsassemble_provider::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStatus,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, consume_local_operator, decode_json_body,
        ensure_empty_body, exact_tauri_cors,
    },
};

const MAX_EMPTY_BODY_BYTES: usize = 4 * 1024;
// This transport ceiling admits the provider owner's 8,192-scalar secret even when
// every scalar is represented as a JSON surrogate pair. Semantic validation stays
// exclusively in ProviderCredentialStore.
const MAX_CREDENTIAL_BODY_BYTES: usize = 128 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetCredentialRequest {
    api_key: String,
}

pub(crate) fn routes() -> Router<AppState> {
    credential_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([
            Method::GET,
            Method::POST,
            Method::DELETE,
        ]))
}

registered_routes! {
    fn credential_routes<AppState>() {
        private "/api/provider-credentials/deepseek" => get(deepseek_status)
            .post(set_deepseek)
            .delete(delete_deepseek),
        private "/api/provider-credentials/cerebras" => get(cerebras_status)
            .post(set_cerebras)
            .delete(delete_cerebras),
        private "/api/provider-credentials/openrouter" => get(openrouter_status)
            .post(set_openrouter)
            .delete(delete_openrouter),
        private "/api/provider-credentials/vercel" => get(vercel_status)
            .post(set_vercel)
            .delete(delete_vercel),
        private "/api/provider-credentials/llmgateway" => get(llm_gateway_status)
            .post(set_llm_gateway)
            .delete(delete_llm_gateway),
    }
}

async fn deepseek_status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    credential_status(state, request, ProviderCredentialId::DeepSeek).await
}

async fn set_deepseek(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    set_credential(state, request, ProviderCredentialId::DeepSeek).await
}

async fn delete_deepseek(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    delete_credential(state, request, ProviderCredentialId::DeepSeek).await
}

async fn cerebras_status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    credential_status(state, request, ProviderCredentialId::Cerebras).await
}

async fn set_cerebras(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    set_credential(state, request, ProviderCredentialId::Cerebras).await
}

async fn delete_cerebras(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    delete_credential(state, request, ProviderCredentialId::Cerebras).await
}

async fn openrouter_status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    credential_status(state, request, ProviderCredentialId::OpenRouter).await
}

async fn set_openrouter(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    set_credential(state, request, ProviderCredentialId::OpenRouter).await
}

async fn delete_openrouter(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    delete_credential(state, request, ProviderCredentialId::OpenRouter).await
}

async fn vercel_status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    credential_status(state, request, ProviderCredentialId::Vercel).await
}

async fn set_vercel(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    set_credential(state, request, ProviderCredentialId::Vercel).await
}

async fn delete_vercel(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    delete_credential(state, request, ProviderCredentialId::Vercel).await
}

async fn llm_gateway_status(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    credential_status(state, request, ProviderCredentialId::LlmGateway).await
}

async fn set_llm_gateway(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    set_credential(state, request, ProviderCredentialId::LlmGateway).await
}

async fn delete_llm_gateway(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    delete_credential(state, request, ProviderCredentialId::LlmGateway).await
}

async fn credential_status(
    state: AppState,
    request: Request,
    provider: ProviderCredentialId,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_EMPTY_BODY_BYTES)
        .await
        .map_err(ProviderCredentialHttpError::from_body)?;
    Ok(Json(state.provider_credentials.status(provider).await?))
}

async fn set_credential(
    state: AppState,
    request: Request,
    provider: ProviderCredentialId,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    authorize(&state, request.headers()).await?;
    let request: SetCredentialRequest = decode_json_body(request, MAX_CREDENTIAL_BODY_BYTES)
        .await
        .map_err(ProviderCredentialHttpError::from_body)?;
    Ok(Json(
        state
            .provider_credentials
            .set(provider, &request.api_key)
            .await?,
    ))
}

async fn delete_credential(
    state: AppState,
    request: Request,
    provider: ProviderCredentialId,
) -> Result<Json<ProviderCredentialStatus>, ProviderCredentialHttpError> {
    authorize(&state, request.headers()).await?;
    ensure_empty_body(request, MAX_EMPTY_BODY_BYTES)
        .await
        .map_err(ProviderCredentialHttpError::from_body)?;
    Ok(Json(state.provider_credentials.delete(provider).await?))
}

async fn authorize(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), ProviderCredentialHttpError> {
    consume_local_operator(state, headers)
        .await
        .ok_or_else(ProviderCredentialHttpError::unauthorized)?;
    Ok(())
}

#[derive(Debug)]
struct ProviderCredentialHttpError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ProviderCredentialHttpError {
    const fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "A valid one-use server-operator ticket is required.",
        }
    }

    const fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Provider credential request body timed out.",
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Provider credential request body exceeds the route limit.",
            },
            BodyDecodeError::InvalidJson | BodyDecodeError::NonEmpty => Self {
                status: StatusCode::BAD_REQUEST,
                code: "provider_credential_invalid",
                message: "Provider credential request is invalid.",
            },
        }
    }
}

impl From<ProviderCredentialError> for ProviderCredentialHttpError {
    fn from(error: ProviderCredentialError) -> Self {
        match error {
            ProviderCredentialError::MissingSecret => Self {
                status: StatusCode::BAD_REQUEST,
                code: "provider_credential_missing",
                message: "Provider credential is missing.",
            },
            ProviderCredentialError::InvalidSecret => Self {
                status: StatusCode::BAD_REQUEST,
                code: "provider_credential_invalid",
                message: "Provider credential is invalid.",
            },
            ProviderCredentialError::SecureStoreUnavailable => Self {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "secure_store_unavailable",
                message: "The platform secure store is unavailable.",
            },
        }
    }
}

impl IntoResponse for ProviderCredentialHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": self.message, "code": self.code})),
        )
            .into_response()
    }
}
