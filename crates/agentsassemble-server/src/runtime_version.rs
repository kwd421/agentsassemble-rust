use std::time::Duration;

use agentsassemble_domain::RuntimeVersion;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{AppState, http_api, room_client_transport};

pub(crate) fn routes() -> Router<AppState> {
    version_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            http_api::PRIVATE_NO_STORE.clone(),
        ))
        .layer(http_api::exact_tauri_cors([Method::GET]))
}

registered_routes! {
    fn version_routes<AppState>() {
        same_origin_public "/api/runtime/version" => get(version),
    }
}

async fn version(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<RuntimeVersion>, StatusCode> {
    http_api::ensure_empty_body(request, 4096)
        .await
        .map_err(|error| match error {
            http_api::BodyDecodeError::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            http_api::BodyDecodeError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            _ => StatusCode::BAD_REQUEST,
        })?;
    Ok(Json(RuntimeVersion {
        frontend_build_id: state
            .frontend
            .as_ref()
            .map(|release| release.build_id().to_owned()),
        protocol_version: agentsassemble_protocol::PROTOCOL_VERSION,
    }))
}

/// Inspect the exact version reported by a running server.
///
/// # Errors
/// Rejects invalid server URLs, transport/status failures and malformed metadata.
pub async fn read(server: &str) -> anyhow::Result<RuntimeVersion> {
    let server = room_client_transport::normalize_server(server)
        .map_err(|_| anyhow::anyhow!("invalid runtime server URL"))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let (status, value) = room_client_transport::read_json_response(
        client.get(server.join(HTTP_ROUTES[0].path.trim_start_matches('/'))?),
        4096,
    )
    .await
    .map_err(|error| {
        anyhow::anyhow!(match error {
            room_client_transport::ResponseReadError::Transport => "runtime version request failed",
            room_client_transport::ResponseReadError::TooLarge =>
                "runtime version response too large",
            room_client_transport::ResponseReadError::InvalidJson =>
                "runtime version response invalid",
        })
    })?;
    if !status.is_success() {
        anyhow::bail!("runtime version request rejected ({status})");
    }
    let version: RuntimeVersion = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("runtime version response invalid"))?;
    if version.frontend_build_id.as_ref().is_some_and(|id| {
        id.len() != 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        anyhow::bail!("runtime build identity invalid");
    }
    Ok(version)
}
