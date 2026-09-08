use agentsassemble_persistence::{PersistenceError, RoomManagerAuthority};
use agentsassemble_protocol::{CreateConnectorInviteRequest, CreatedConnectorInvite};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, header::CACHE_CONTROL},
};
use chrono::Utc;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

use crate::{
    AppState,
    connector_web::ConnectorHttpError,
    http_api::{PRIVATE_NO_STORE, bearer_credential, decode_json_body, exact_tauri_cors},
};

registered_routes! {
    fn manager_routes<AppState>() {
        private "/api/room-connector/invite" => post(create),
    }
}

pub(crate) fn routes() -> Router<AppState> {
    manager_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]))
}

async fn create(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<CreatedConnectorInvite>, ConnectorHttpError> {
    let bearer =
        bearer_credential(request.headers()).ok_or_else(ConnectorHttpError::unauthorized)?;
    let grant = state
        .tickets
        .consume_connector_invite_create(bearer)
        .await
        .map_err(|_| ConnectorHttpError::unauthorized())?;
    let payload: CreateConnectorInviteRequest = decode_json_body(request, 8192).await?;
    let request_id =
        Uuid::parse_str(&payload.request_id).map_err(|_| ConnectorHttpError::invalid())?;
    let ingress = state.public_ingress.ready_snapshot().ok_or_else(|| {
        ConnectorHttpError::from_persistence(PersistenceError::CommandRejected {
            code: "public_ingress_not_ready".into(),
            message: "Public ingress must be ready before creating an invite.".to_owned(),
        })
    })?;
    let room_uid = grant.authority.room_uid.to_string();
    let invite = state
        .store
        .create_connector_invite(
            &RoomManagerAuthority::Local(grant.authority),
            request_id,
            payload.scope,
            Utc::now(),
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let mut join = url::Url::parse(&format!(
        "{}/join",
        ingress.public_url.trim_end_matches('/')
    ))
    .map_err(|_| ConnectorHttpError::invalid())?;
    join.query_pairs_mut()
        .append_pair("token", &invite.invite_bearer);
    Ok(Json(CreatedConnectorInvite {
        request_id: payload.request_id,
        room_uid,
        invite_id: invite.invite_id.to_string(),
        expires_at: invite.expires_at.to_rfc3339(),
        join_url: join.to_string(),
    }))
}
