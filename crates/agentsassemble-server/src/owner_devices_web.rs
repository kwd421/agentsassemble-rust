use agentsassemble_persistence::{PersistenceError, ServerOwnerAuthority};
use agentsassemble_protocol::{
    OwnerDeviceKind, OwnerDeviceSession, OwnerDevices, RevokeOwnerDevices,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header},
};
use serde_json::{Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        PRIVATE_NO_STORE, consume_local_operator, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
};

type Failure = (StatusCode, Json<Value>);

registered_routes! {
    fn owner_device_routes<AppState>() {
        private "/api/owner-sessions" => get(list),
        private "/api/owner-sessions/revoke" => post(revoke),
        same_origin_public "/api/central-owner/sessions" => get(list),
        same_origin_public "/api/central-owner/sessions/revoke" => post(revoke),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    owner_device_routes()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

async fn authorize(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
    remote: bool,
) -> Result<ServerOwnerAuthority, Failure> {
    if remote {
        return crate::central::owner_web::owner_from_session_headers(state, headers, origin)
            .await
            .map_err(|error| failure(error.status, error.code, error.message));
    }
    consume_local_operator(state, headers)
        .await
        .ok_or_else(|| {
            failure(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "A local operator ticket is required.",
            )
        })?;
    Ok(ServerOwnerAuthority::LocalOperator)
}

async fn list(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<OwnerDevices>, Failure> {
    let owner = authorize(
        &state,
        request.headers(),
        request.extensions().get(),
        request.uri().path().starts_with("/api/central-owner/"),
    )
    .await?;
    ensure_empty_body(request, 4096).await.map_err(|_| {
        failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The device list requires an empty request body.",
        )
    })?;
    let rows = state
        .store
        .owner_device_sessions(&owner)
        .await
        .map_err(storage_error)?;
    let mut sessions = Vec::new();
    if matches!(owner, ServerOwnerAuthority::LocalOperator) {
        let host = crate::central::host_identity::host_device_info(None)
            .await
            .map_err(|_| {
                failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "host_device_unavailable",
                    "The host device description is unavailable.",
                )
            })?;
        sessions.push(OwnerDeviceSession {
            session_id: "host".to_owned(),
            device_name: host.host_name,
            browser: "AgentsAssemble 앱".to_owned(),
            os: host.host_os,
            last_connected_at: Some(state.started_at),
            current: true,
            connected: Some(true),
            revocable: false,
            kind: OwnerDeviceKind::Host,
        });
    }
    sessions.extend(rows.into_iter().map(|row| OwnerDeviceSession {
        session_id: row.session_id.to_string(),
        device_name: row.device_name,
        browser: row.browser,
        os: row.os,
        last_connected_at: row.last_connected_at,
        current: row.current,
        connected: row.connected,
        revocable: true,
        kind: if row.pairing {
            OwnerDeviceKind::Pairing
        } else {
            OwnerDeviceKind::Owner
        },
    }));
    Ok(Json(OwnerDevices { sessions }))
}

async fn revoke(State(state): State<AppState>, request: Request) -> Result<Json<Value>, Failure> {
    let owner = authorize(
        &state,
        request.headers(),
        request.extensions().get(),
        request.uri().path().starts_with("/api/central-owner/"),
    )
    .await?;
    let body: RevokeOwnerDevices = decode_json_body(request, 4096).await.map_err(|_| {
        failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The device revocation request is invalid.",
        )
    })?;
    let target = match body {
        RevokeOwnerDevices::Session { session_id } => Some(session_id),
        RevokeOwnerDevices::All {} => None,
    };
    let committed = state
        .store
        .revoke_owner_devices(&owner, target)
        .await
        .map_err(storage_error)?;
    // Persistence precedes closure, including when the caller revokes its own session.
    state.owner_sessions.revoke(&committed.owner_fingerprints);
    state
        .rooms
        .publish_session_revocations(&committed.room_sessions)
        .await;
    Ok(Json(
        json!({ "status": "revoked", "revoked_count": committed.revoked_count }),
    ))
}

fn storage_error(error: PersistenceError) -> Failure {
    match error {
        PersistenceError::CommandRejected { code, message }
            if code.as_ref() == "owner_device_not_found" =>
        {
            failure(StatusCode::NOT_FOUND, &code, &message)
        }
        PersistenceError::CommandRejected { code, message } => {
            failure(StatusCode::UNAUTHORIZED, &code, &message)
        }
        _ => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "owner_devices_failed",
            "The host could not persist the device operation.",
        ),
    }
}

fn failure(status: StatusCode, code: &str, message: &str) -> Failure {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}
