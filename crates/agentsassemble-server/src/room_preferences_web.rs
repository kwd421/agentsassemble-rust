use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_USER_ID, RoomUserPreferencesPatch, public_settings,
    validate_room_id,
};
use agentsassemble_persistence::{
    LocalRoomPreferencesDirectoryEntry, PersistenceError, RoomPreferencesSnapshot,
};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{Method, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    AppState,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_credential, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
    room_session_http_authority::{
        RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
    },
    ticket::RoomSessionHttpAuthority,
};

const MAX_PREFERENCES_BODY_BYTES: usize = 16 * 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsQuery {
    #[serde(default)]
    room_id: String,
}

pub(crate) fn routes() -> Router<AppState> {
    preference_routes()
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            PRIVATE_NO_STORE.clone(),
        ))
        .layer(exact_tauri_cors([Method::GET, Method::POST]))
}

registered_routes! {
    fn preference_routes<AppState>() {
        same_origin_public "/api/room-settings" => get(read_settings).post(update_preferences),
    }
}

async fn read_settings(
    State(state): State<AppState>,
    Query(query): Query<SettingsQuery>,
    request: Request,
) -> Result<Json<Value>, RoomPreferencesHttpError> {
    if query.room_id.is_empty() {
        consume_directory_ticket(&state, request.headers()).await?;
        ensure_empty_body(request, MAX_PREFERENCES_BODY_BYTES)
            .await
            .map_err(RoomPreferencesHttpError::from_body)?;
        let entries = state.store.local_room_preferences_directory().await?;
        let rooms = entries
            .iter()
            .map(directory_settings_payload)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Json(json!({"rooms": rooms})));
    }

    let grant =
        resolve_preferences_read_authority(&state, request.headers(), request.extensions().get())
            .await?;
    let requested_room_id = validate_room_id(&query.room_id)
        .map_err(|error| RoomPreferencesHttpError::bad_request(error.message))?;
    require_bound_room(&grant, &requested_room_id)?;
    ensure_empty_body(request, MAX_PREFERENCES_BODY_BYTES)
        .await
        .map_err(RoomPreferencesHttpError::from_body)?;
    let snapshot = match &grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .room_preferences(&grant.room_id, &grant.principal_id, &grant.participant_id)
                .await?
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            state
                .store
                .room_session_room_preferences(authorization)
                .await?
        }
    };
    Ok(Json(json!({
        "room_id": requested_room_id,
        "settings": combined_settings_payload(&requested_room_id, &snapshot)?,
    })))
}

async fn update_preferences(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, RoomPreferencesHttpError> {
    let grant =
        resolve_preferences_write_authority(&state, request.headers(), request.extensions().get())
            .await?;
    authorize_preference_write(&state, &grant).await?;
    let payload: Value = decode_json_body(request, MAX_PREFERENCES_BODY_BYTES)
        .await
        .map_err(RoomPreferencesHttpError::from_body)?;
    let room_id = preference_room_id(&grant);
    let (expected_incarnation, patch) = parse_preference_update(&payload, room_id)?;
    let snapshot = match &grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .update_room_preferences(
                    &grant.room_id,
                    expected_incarnation,
                    &grant.principal_id,
                    &grant.participant_id,
                    patch,
                )
                .await?
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            state
                .store
                .update_room_session_room_preferences(authorization, expected_incarnation, patch)
                .await?
        }
    };
    Ok(Json(json!({
        "room_id": room_id,
        "settings": combined_settings_payload(room_id, &snapshot)?,
    })))
}

async fn resolve_preferences_read_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<RoomSessionHttpAuthority, RoomPreferencesHttpError> {
    let credential =
        bearer_credential(headers).ok_or_else(RoomPreferencesHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomSessionHttpAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_preferences_read(credential)
            .await
            .map(RoomSessionHttpAuthority::LocalTicket)
            .map_err(|_| RoomPreferencesHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(RoomPreferencesHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn resolve_preferences_write_authority(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    origin: Option<&crate::ingress_trust::TrustedIngressOrigin>,
) -> Result<RoomSessionHttpAuthority, RoomPreferencesHttpError> {
    let credential =
        bearer_credential(headers).ok_or_else(RoomPreferencesHttpError::unauthorized)?;
    match resolve_room_session_bearer(state, headers, origin, credential).await {
        Ok(RoomSessionBearerResolution::Authorized(authorization)) => {
            Ok(RoomSessionHttpAuthority::Session(authorization))
        }
        Ok(RoomSessionBearerResolution::Other) => state
            .tickets
            .consume_preferences_write(credential)
            .await
            .map(RoomSessionHttpAuthority::LocalTicket)
            .map_err(|_| RoomPreferencesHttpError::unauthorized()),
        Err(RoomSessionBearerError::Invalid) => Err(RoomPreferencesHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => Err(error.into()),
    }
}

async fn authorize_preference_write(
    state: &AppState,
    grant: &RoomSessionHttpAuthority,
) -> Result<(), RoomPreferencesHttpError> {
    match grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => {
            state
                .store
                .authorize_room_user(&grant.room_id, &grant.principal_id, &grant.participant_id)
                .await?;
        }
        RoomSessionHttpAuthority::Session(authorization) => {
            let current = state
                .store
                .revalidate_room_session_authorization(authorization)
                .await?;
            if current.principal().invite_scope != InviteScope::ReadWrite {
                return Err(RoomPreferencesHttpError::read_only());
            }
        }
    }
    Ok(())
}

async fn consume_directory_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), RoomPreferencesHttpError> {
    let ticket = bearer_credential(headers).ok_or_else(RoomPreferencesHttpError::unauthorized)?;
    let grant = state
        .tickets
        .consume_settings_directory_read(ticket)
        .await
        .map_err(|_| RoomPreferencesHttpError::unauthorized())?;
    if grant.principal_id != LOCAL_OPERATOR_USER_ID {
        return Err(RoomPreferencesHttpError::unauthorized());
    }
    Ok(())
}

fn require_bound_room(
    grant: &RoomSessionHttpAuthority,
    requested_room_id: &str,
) -> Result<(), RoomPreferencesHttpError> {
    if preference_room_id(grant) == requested_room_id {
        Ok(())
    } else {
        Err(RoomPreferencesHttpError::unauthorized())
    }
}

fn preference_room_id(grant: &RoomSessionHttpAuthority) -> &str {
    match grant {
        RoomSessionHttpAuthority::LocalTicket(grant) => &grant.room_id,
        RoomSessionHttpAuthority::Session(authorization) => &authorization.principal().room_id,
    }
}

fn parse_preference_update(
    payload: &Value,
    bound_room_id: &str,
) -> Result<(uuid::Uuid, RoomUserPreferencesPatch), RoomPreferencesHttpError> {
    let object = payload
        .as_object()
        .ok_or_else(|| RoomPreferencesHttpError::bad_request("Request body must be an object."))?;
    let room_id = object
        .get("room_id")
        .and_then(Value::as_str)
        .ok_or_else(|| RoomPreferencesHttpError::bad_request("room_id is required."))?;
    let room_id = validate_room_id(room_id)
        .map_err(|error| RoomPreferencesHttpError::bad_request(error.message))?;
    if room_id != bound_room_id {
        return Err(RoomPreferencesHttpError::unauthorized());
    }

    let global_fields = [
        "label",
        "topic",
        "channels",
        "short_label",
        "conversation_mode",
        "tool_mode",
        "ordered_exclude_previous_speaker",
    ];
    if object
        .keys()
        .any(|key| global_fields.contains(&key.as_str()))
    {
        return Err(RoomPreferencesHttpError::global_conflict());
    }
    if let Some(appearance) = object.get("appearance") {
        let appearance = appearance.as_object().ok_or_else(|| {
            RoomPreferencesHttpError::bad_request("appearance must be an object.")
        })?;
        if appearance.keys().any(|key| key != "notifications") {
            return Err(RoomPreferencesHttpError::global_conflict());
        }
    }
    let allowed = ["room_id", "room_uid", "appearance", "channel_settings"];
    if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(RoomPreferencesHttpError::bad_request(format!(
            "Unsupported room preference field: {unknown}."
        )));
    }

    let expected_incarnation = object
        .get("room_uid")
        .and_then(Value::as_str)
        .ok_or_else(|| RoomPreferencesHttpError::bad_request("room_uid is required."))?;
    let parsed_uid = uuid::Uuid::parse_str(expected_incarnation)
        .map_err(|_| RoomPreferencesHttpError::bad_request("room_uid must be a canonical UUID."))?;
    if parsed_uid.to_string() != expected_incarnation {
        return Err(RoomPreferencesHttpError::bad_request(
            "room_uid must be a canonical UUID.",
        ));
    }

    let mut strict_patch = Map::new();
    if let Some(notifications) = object
        .get("appearance")
        .and_then(Value::as_object)
        .and_then(|appearance| appearance.get("notifications"))
    {
        strict_patch.insert("notifications".to_owned(), notifications.clone());
    }
    if let Some(channel_settings) = object.get("channel_settings") {
        strict_patch.insert("channel_settings".to_owned(), channel_settings.clone());
    }
    serde_json::from_value(Value::Object(strict_patch))
        .map(|patch| (parsed_uid, patch))
        .map_err(|error| RoomPreferencesHttpError::bad_request(error.to_string()))
}

fn combined_settings_payload(
    room_id: &str,
    snapshot: &RoomPreferencesSnapshot,
) -> Result<Value, RoomPreferencesHttpError> {
    project_settings(room_id, &snapshot.room_settings, &snapshot.preferences)
}

fn directory_settings_payload(
    entry: &LocalRoomPreferencesDirectoryEntry,
) -> Result<Value, RoomPreferencesHttpError> {
    project_settings(
        &entry.room.room_id,
        &entry.room_settings,
        &entry.preferences,
    )
}

fn project_settings(
    room_id: &str,
    room_settings: &agentsassemble_domain::RoomSettings,
    preferences: &agentsassemble_domain::RoomUserPreferences,
) -> Result<Value, RoomPreferencesHttpError> {
    let mut settings = serde_json::to_value(public_settings(room_settings)?)?;
    let object = settings
        .as_object_mut()
        .ok_or_else(RoomPreferencesHttpError::internal)?;
    object.insert("room_id".to_owned(), Value::String(room_id.to_owned()));
    object.insert(
        "short_label".to_owned(),
        Value::String(room_settings.appearance.icon_label.clone()),
    );
    object.insert(
        "channel_settings".to_owned(),
        serde_json::to_value(&preferences.channel_settings)?,
    );
    object
        .get_mut("appearance")
        .and_then(Value::as_object_mut)
        .ok_or_else(RoomPreferencesHttpError::internal)?
        .insert(
            "notifications".to_owned(),
            serde_json::to_value(preferences.notifications)?,
        );
    Ok(settings)
}

#[derive(Debug)]
struct RoomPreferencesHttpError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl RoomPreferencesHttpError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "Valid room-settings authority is required.".to_owned(),
        }
    }

    fn global_conflict() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "room_settings_transport_conflict",
            message: "Room-global settings must use the canonical room WebSocket command."
                .to_owned(),
        }
    }

    fn read_only() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "session_read_only",
            message: "Read-only room sessions cannot change preferences.".to_owned(),
        }
    }

    fn from_body(error: BodyDecodeError) -> Self {
        match error {
            BodyDecodeError::RequestTimeout => Self {
                status: StatusCode::REQUEST_TIMEOUT,
                code: "request_timeout",
                message: "Request body timed out.".to_owned(),
            },
            BodyDecodeError::PayloadTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                code: "payload_too_large",
                message: "Request body exceeds the route limit.".to_owned(),
            },
            BodyDecodeError::InvalidJson => Self::bad_request("Request JSON is invalid."),
            BodyDecodeError::NonEmpty => {
                Self::bad_request("GET room-settings requests must not contain a body.")
            }
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "persistence_failed",
            message: "Persistence operation failed.".to_owned(),
        }
    }
}

impl From<PersistenceError> for RoomPreferencesHttpError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::RoomMissing => Self {
                status: StatusCode::NOT_FOUND,
                code: "room_not_found",
                message: "Room does not exist.".to_owned(),
            },
            PersistenceError::ParticipantMissing => Self::unauthorized(),
            PersistenceError::CommandRejected { code, message }
                if code == "room_incarnation_changed" =>
            {
                Self {
                    status: StatusCode::CONFLICT,
                    code: "room_incarnation_changed",
                    message,
                }
            }
            PersistenceError::CommandRejected { code, .. }
                if matches!(
                    code.as_bytes(),
                    b"session_revoked"
                        | b"room_inactive"
                        | b"user_profile_missing"
                        | b"profile_authority_mismatch"
                ) =>
            {
                Self::unauthorized()
            }
            PersistenceError::CommandRejected { code, message }
                if matches!(
                    code.as_bytes(),
                    b"room_preferences_invalid" | b"bad_request"
                ) =>
            {
                Self::bad_request(message)
            }
            PersistenceError::CommandRejected { code, message }
                if matches!(
                    code.as_bytes(),
                    b"bootstrap_required" | b"bootstrap_repair_required"
                ) =>
            {
                Self {
                    status: StatusCode::CONFLICT,
                    code: "bootstrap_required",
                    message,
                }
            }
            error => {
                tracing::error!(error = ?error, "room preferences HTTP persistence failed");
                Self::internal()
            }
        }
    }
}

impl From<serde_json::Error> for RoomPreferencesHttpError {
    fn from(error: serde_json::Error) -> Self {
        tracing::error!(error = ?error, "room preferences HTTP projection failed");
        Self::internal()
    }
}

impl IntoResponse for RoomPreferencesHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_preference_update;

    #[test]
    fn preference_parser_rejects_global_and_room_mismatch_without_aliases() {
        let global_payload = json!({"room_id": "general", "label": "Changed"});
        let Err(global) = parse_preference_update(&global_payload, "general") else {
            panic!("global HTTP settings write was accepted");
        };
        assert_eq!(global.status, axum::http::StatusCode::CONFLICT);
        let mismatch_payload = json!({"room_id": "other", "appearance": {"notifications": "mute"}});
        let Err(mismatch) = parse_preference_update(&mismatch_payload, "general") else {
            panic!("ticket-bound room mismatch was accepted");
        };
        assert_eq!(mismatch.status, axum::http::StatusCode::UNAUTHORIZED);
        for body in [
            json!({"room_id": "general"}),
            json!({"room_id": "general", "room_uid": "A53A3F5C-0E7B-4DE1-A70C-8F548E03E90C"}),
        ] {
            let Err(error) = parse_preference_update(&body, "general") else {
                panic!("missing or noncanonical room UID was accepted");
            };
            assert_eq!(error.status, axum::http::StatusCode::BAD_REQUEST);
        }
        assert!(
            parse_preference_update(
                &json!({
                    "room_id": "general",
                    "room_uid": "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
                    "appearance": {"notifications": "mute"},
                    "channel_settings": {
                        "lobby": {"notifications": "default", "last_read_at": "cursor"}
                    }
                }),
                "general",
            )
            .is_ok()
        );
    }
}
