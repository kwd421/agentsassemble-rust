use agentsassemble_domain::{
    LOCAL_OPERATOR_USER_ID, RoomUserPreferencesPatch, public_settings, validate_room_id,
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
    AppState, ConsumedRoomHttpTicket,
    http_api::{
        BodyDecodeError, PRIVATE_NO_STORE, bearer_ticket, decode_json_body, ensure_empty_body,
        exact_tauri_cors,
    },
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
        "/api/room-settings" => get(read_settings).post(update_preferences),
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

    let grant = consume_preferences_read_ticket(&state, request.headers()).await?;
    let requested_room_id = validate_room_id(&query.room_id)
        .map_err(|error| RoomPreferencesHttpError::bad_request(error.message))?;
    require_bound_room(&grant, &requested_room_id)?;
    ensure_empty_body(request, MAX_PREFERENCES_BODY_BYTES)
        .await
        .map_err(RoomPreferencesHttpError::from_body)?;
    let snapshot = state
        .store
        .room_preferences(&grant.room_id, &grant.principal_id, &grant.participant_id)
        .await?;
    Ok(Json(json!({
        "room_id": grant.room_id,
        "settings": combined_settings_payload(&grant.room_id, &snapshot)?,
    })))
}

async fn update_preferences(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, RoomPreferencesHttpError> {
    let grant = consume_preferences_write_ticket(&state, request.headers()).await?;
    state
        .store
        .authorize_room_user(&grant.room_id, &grant.principal_id, &grant.participant_id)
        .await?;
    let payload: Value = decode_json_body(request, MAX_PREFERENCES_BODY_BYTES)
        .await
        .map_err(RoomPreferencesHttpError::from_body)?;
    let patch = parse_preference_update(&payload, &grant.room_id)?;
    let snapshot = if patch.notifications.is_none() && patch.channel_settings.is_none() {
        state
            .store
            .room_preferences(&grant.room_id, &grant.principal_id, &grant.participant_id)
            .await?
    } else {
        state
            .store
            .update_room_preferences(
                &grant.room_id,
                &grant.principal_id,
                &grant.participant_id,
                patch,
            )
            .await?
    };
    Ok(Json(json!({
        "room_id": grant.room_id,
        "settings": combined_settings_payload(&grant.room_id, &snapshot)?,
    })))
}

async fn consume_preferences_read_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<ConsumedRoomHttpTicket, RoomPreferencesHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(RoomPreferencesHttpError::unauthorized)?;
    state
        .tickets
        .consume_preferences_read(ticket)
        .await
        .map_err(|_| RoomPreferencesHttpError::unauthorized())
}

async fn consume_preferences_write_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<ConsumedRoomHttpTicket, RoomPreferencesHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(RoomPreferencesHttpError::unauthorized)?;
    state
        .tickets
        .consume_preferences_write(ticket)
        .await
        .map_err(|_| RoomPreferencesHttpError::unauthorized())
}

async fn consume_directory_ticket(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), RoomPreferencesHttpError> {
    let ticket = bearer_ticket(headers).ok_or_else(RoomPreferencesHttpError::unauthorized)?;
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
    grant: &ConsumedRoomHttpTicket,
    requested_room_id: &str,
) -> Result<(), RoomPreferencesHttpError> {
    if grant.room_id == requested_room_id {
        Ok(())
    } else {
        Err(RoomPreferencesHttpError::unauthorized())
    }
}

fn parse_preference_update(
    payload: &Value,
    bound_room_id: &str,
) -> Result<RoomUserPreferencesPatch, RoomPreferencesHttpError> {
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
        "activity_plugin",
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
    let allowed = ["room_id", "appearance", "channel_settings"];
    if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(RoomPreferencesHttpError::bad_request(format!(
            "Unsupported room preference field: {unknown}."
        )));
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
            message: "A valid one-use room-settings ticket is required.".to_owned(),
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
            PersistenceError::ParticipantMissing
            | PersistenceError::CommandRejected {
                code:
                    "session_revoked"
                    | "room_inactive"
                    | "user_profile_missing"
                    | "profile_authority_mismatch",
                ..
            } => Self::unauthorized(),
            PersistenceError::CommandRejected {
                code: "room_preferences_invalid" | "bad_request",
                message,
            } => Self::bad_request(message),
            PersistenceError::CommandRejected {
                code: "bootstrap_required" | "bootstrap_repair_required",
                message,
            } => Self {
                status: StatusCode::CONFLICT,
                code: "bootstrap_required",
                message,
            },
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
        assert!(
            parse_preference_update(
                &json!({
                    "room_id": "general",
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
