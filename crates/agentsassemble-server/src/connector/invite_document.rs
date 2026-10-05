//! Read-only onboarding at the invitation URL; connector admission remains MCP-owned.
use agentsassemble_persistence::CONNECTOR_INVITE_PREFIX;
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::{AppState, http_api::PRIVATE_NO_STORE, ingress_trust::TrustedIngressOrigin};

const INSTRUCTIONS: &str =
    include_str!("../../../../frontend/src/api/connectorInviteInstructions.txt");

registered_routes! {
    fn entry_routes<AppState>() {
        same_origin_public "/join" => get(entry),
        same_origin_public "/join/" => get(entry),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    entry_routes()
}

async fn entry(State(state): State<AppState>, request: Request) -> Response {
    let query = request.uri().query().unwrap_or_default();
    let fields: Vec<_> = url::form_urlencoded::parse(query.as_bytes()).collect();
    // Other invitation purposes keep the existing browser entry, including its query.
    if !fields
        .iter()
        .any(|(key, value)| key == "token" && value.starts_with(CONNECTOR_INVITE_PREFIX))
    {
        return state.frontend.as_ref().map_or_else(
            || StatusCode::NOT_FOUND.into_response(),
            |frontend| {
                let mut response = Html(frontend.index_html().to_string()).into_response();
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    crate::web::STATIC_FRONTEND_CACHE_CONTROL,
                );
                response
            },
        );
    }
    let [(key, token)] = fields.as_slice() else {
        return document_error(StatusCode::BAD_REQUEST);
    };
    if key != "token"
        || token.len() != CONNECTOR_INVITE_PREFIX.len() + 43
        || !URL_SAFE_NO_PAD
            .decode(&token[CONNECTOR_INVITE_PREFIX.len()..])
            .is_ok_and(|bytes| bytes.len() == 32)
    {
        return document_error(StatusCode::BAD_REQUEST);
    }
    // The ingress middleware has verified this origin or the exact local listener.
    // Never build setup commands from an untrusted Host/Forwarded header.
    let Some(origin) = request.extensions().get::<TrustedIngressOrigin>() else {
        return document_error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let origin = origin.as_str();
    // These URLs are also printed as CLI arguments. Do not interpolate shell syntax,
    // even when an operator configured an unusual origin accepted by the URL parser.
    if !origin
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b":/._-[]".contains(&byte))
    {
        return document_error(StatusCode::BAD_REQUEST);
    }
    let Ok(mut invite) = url::Url::parse(&format!("{origin}/join")) else {
        return document_error(StatusCode::SERVICE_UNAVAILABLE);
    };
    invite.query_pairs_mut().append_pair("token", token);
    let text = INSTRUCTIONS
        .replace("{{MCP_URL}}", &format!("{origin}/mcp"))
        .replace("{{INVITE_URL}}", invite.as_str())
        .replace("{{EXPIRY}}", "만료 여부는 room_join에서 확인합니다.");
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    let escaped = escaped.replace('\n', "<br>\n");
    let mut response = Html(format!(
        "<!doctype html><html lang=\"ko\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"robots\" content=\"noindex,nofollow,noarchive\"><title>AgentsAssemble AI 초대</title></head><body><main><h1>현재 AI 대화 초대</h1><div>{escaped}</div></main></body></html>"
    ))
    .into_response();
    protect_document(&mut response);
    response
}

fn document_error(status: StatusCode) -> Response {
    let mut response = (status, "AI 초대 안내를 표시할 수 없습니다.").into_response();
    protect_document(&mut response);
    response
}

fn protect_document(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, PRIVATE_NO_STORE.clone());
    response.headers_mut().insert(
        header::HeaderName::from_static("x-robots-tag"),
        HeaderValue::from_static("noindex, nofollow, noarchive"),
    );
}
