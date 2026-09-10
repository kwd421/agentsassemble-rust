//! Entry packets preserve friend-manager and exact room-session ownership boundaries.
use super::AttendeeHttpError;
use crate::{
    AppState,
    http_api::{PRIVATE_NO_STORE, bearer_credential, decode_json_body, exact_tauri_cors},
    ingress_trust::TrustedIngressOrigin,
    room_session_http_authority::{
        RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
    },
};
use agentsassemble_persistence::{AttendeeInvite, CompanionInviteRequest, RoomManagerAuthority};
use agentsassemble_protocol::{
    AttendeeEntryPacket, CreateCompanionAttendeeInvite, CreateFriendAttendeeInvite,
};
use agentsassemble_provider::{registered_provider_id, registered_provider_kind};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
};
use chrono::Utc;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

registered_routes! {
    fn entry_routes<AppState>() {
        private "/api/room-attendee/friend-invite" => post(friend),
        same_origin_public "/api/room-attendee/companion-invite" => post(companion),
    }
}

pub(super) fn routes() -> Router<AppState> {
    entry_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::POST]))
}

async fn friend(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<AttendeeEntryPacket>, AttendeeHttpError> {
    let bearer = bearer_credential(request.headers()).ok_or_else(unauthorized)?;
    let grant = state
        .tickets
        .consume_attendee_invite_create(bearer)
        .await
        .map_err(|_| unauthorized())?;
    let body: CreateFriendAttendeeInvite = decode_json_body(request, 8192).await?;
    let origin = public_origin(&state)?;
    let room_id = grant.authority.manager.room_id.clone();
    let invite = state
        .store
        .create_friend_attendee_invite(
            &RoomManagerAuthority::Local(grant.authority),
            parse_id(&body.request_id)?,
            parse_id(&body.friend_id)?,
            registered_provider_kind,
            Utc::now(),
        )
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    packet(&origin, body.request_id, room_id, invite).map(Json)
}

async fn companion(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<AttendeeEntryPacket>, AttendeeHttpError> {
    let bearer = bearer_credential(request.headers()).ok_or_else(unauthorized)?;
    let issuer = match resolve_room_session_bearer(
        &state,
        request.headers(),
        request.extensions().get::<TrustedIngressOrigin>(),
        bearer,
    )
    .await
    {
        Ok(RoomSessionBearerResolution::Authorized(issuer)) => issuer,
        Ok(RoomSessionBearerResolution::Other) | Err(RoomSessionBearerError::Invalid) => {
            return Err(unauthorized());
        }
        Err(RoomSessionBearerError::Persistence(error)) => {
            return Err(AttendeeHttpError::from_persistence(error));
        }
    };
    let body: CreateCompanionAttendeeInvite = decode_json_body(request, 8192).await?;
    let provider_kind = registered_provider_kind(&body.provider).ok_or_else(|| {
        AttendeeHttpError::rejected(StatusCode::BAD_REQUEST, "unsupported_provider")
    })?;
    let origin = public_origin(&state)?;
    let invite = state
        .store
        .create_companion_attendee_invite(
            &issuer,
            CompanionInviteRequest {
                request_id: parse_id(&body.request_id)?,
                provider_kind,
                display_name: &body.display_name,
            },
            Utc::now(),
        )
        .await
        .map_err(AttendeeHttpError::from_persistence)?;
    packet(
        &origin,
        body.request_id,
        issuer.principal().room_id.clone(),
        invite,
    )
    .map(Json)
}

fn packet(
    origin: &str,
    request_id: String,
    room_id: String,
    invite: AttendeeInvite,
) -> Result<AttendeeEntryPacket, AttendeeHttpError> {
    let provider = registered_provider_id(&invite.provider_kind).ok_or_else(|| {
        AttendeeHttpError::rejected(StatusCode::BAD_REQUEST, "unsupported_provider")
    })?;
    let mut join = url::Url::parse(&format!("{}/join", origin.trim_end_matches('/')))
        .map_err(|_| invalid())?;
    join.query_pairs_mut()
        .append_pair("token", &invite.invite_bearer);
    Ok(AttendeeEntryPacket {
        request_id,
        room_id,
        room_uid: invite.room_uid.to_string(),
        invite_id: invite.invite_id.to_string(),
        expires_at: invite.expires_at.to_rfc3339(),
        display_name: invite.display_name,
        provider: provider.to_owned(),
        attend_command: format!("assemble room attend --provider {provider}"),
        join_url: join.to_string(),
    })
}

fn public_origin(state: &AppState) -> Result<String, AttendeeHttpError> {
    state
        .public_ingress
        .ready_snapshot()
        .map(|ingress| ingress.public_url)
        .ok_or_else(|| {
            AttendeeHttpError::rejected(StatusCode::CONFLICT, "public_ingress_not_ready")
        })
}
fn parse_id(value: &str) -> Result<Uuid, AttendeeHttpError> {
    Uuid::parse_str(value).map_err(|_| invalid())
}
fn invalid() -> AttendeeHttpError {
    AttendeeHttpError::rejected(StatusCode::BAD_REQUEST, "invalid_attendee_request")
}
fn unauthorized() -> AttendeeHttpError {
    AttendeeHttpError::rejected(
        StatusCode::UNAUTHORIZED,
        "attendee_invite_authority_required",
    )
}
