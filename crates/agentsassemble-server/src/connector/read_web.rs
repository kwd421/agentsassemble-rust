use super::{AppState, ConnectorHttpError};
use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent, public_event_for_principal};
use agentsassemble_persistence::{ConnectorSessionAuthorization, RoomMutationAuthority};
use axum::{
    Json,
    extract::{Query, Request, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};

const SNAPSHOT_EVENTS: usize = 200;
const MESSAGE_LIMIT: usize = 50;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cursor {
    #[serde(default)]
    after_seq: i64,
    #[serde(default)]
    joining: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    q: String,
    channel_id: String,
    #[serde(default)]
    cursor: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Context {
    channel_id: String,
    event_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Vote {
    vote_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StatusQuery {
    #[serde(default)]
    before_seq: i64,
}

pub(super) async fn status(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = authorize_read(&state, request, 20).await?;
    let status = state
        .store
        .conversation_status(
            RoomMutationAuthority::ConnectorSession(&authorization),
            query.before_seq,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let mut value = json!(status);
    enrich(&state, &authorization, &mut value, &[], false).await?;
    Ok(Json(value))
}

pub(super) async fn snapshot(
    State(state): State<AppState>,
    Query(cursor): Query<Cursor>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = authorize_read(&state, request, SNAPSHOT_EVENTS).await?;
    let snapshot = state
        .store
        .snapshot_for(
            RoomMutationAuthority::ConnectorSession(&authorization),
            cursor.after_seq,
            200,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let messages = messages(snapshot.events, authorization.principal(), false);
    let mut value = json!({"room":snapshot.room,"participants":snapshot.participants,"messages":messages,"last_seq":snapshot.last_seq,"resync_required":snapshot.resume_gap});
    enrich(&state, &authorization, &mut value, &[], !cursor.joining).await?;
    Ok(Json(value))
}

pub(super) async fn search(
    State(state): State<AppState>,
    Query(query): Query<Search>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = authorize_read(
        &state,
        request,
        agentsassemble_domain::MESSAGE_SEARCH_PAGE_SIZE,
    )
    .await?;
    let result = state
        .store
        .search_authorized_messages(
            RoomMutationAuthority::ConnectorSession(&authorization),
            &query.channel_id,
            &query.q,
            &query.cursor,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let read_ids = result
        .results
        .iter()
        .map(|message| message.event_id.clone())
        .collect::<Vec<_>>();
    let mut value = json!(result);
    enrich(&state, &authorization, &mut value, &read_ids, false).await?;
    Ok(Json(value))
}

pub(super) async fn context(
    State(state): State<AppState>,
    Query(query): Query<Context>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = authorize_read(
        &state,
        request,
        2 * agentsassemble_domain::MESSAGE_CONTEXT_RADIUS + 1,
    )
    .await?;
    let result = state
        .store
        .authorized_message_context(
            RoomMutationAuthority::ConnectorSession(&authorization),
            &query.channel_id,
            &query.event_id,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let read_ids = result
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let mut value = json!(result);
    enrich(&state, &authorization, &mut value, &read_ids, false).await?;
    Ok(Json(value))
}

pub(super) async fn vote(
    State(state): State<AppState>,
    Query(query): Query<Vote>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let authorization = authorize_read(&state, request, 1).await?;
    let result = state
        .store
        .authorized_room_vote_summary(
            RoomMutationAuthority::ConnectorSession(&authorization),
            &query.vote_id,
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    Ok(Json(json!(result)))
}

pub(super) async fn wait(
    State(state): State<AppState>,
    Query(cursor): Query<Cursor>,
    request: Request,
) -> Result<Json<Value>, ConnectorHttpError> {
    let transport = request
        .extensions()
        .get::<crate::http_admission::HttpConnectionAdmission>()
        .cloned();
    let authorization = authorize_read(&state, request, SNAPSHOT_EVENTS).await?;
    let principal = authorization.principal();
    let _lease = state
        .connection_admission
        .acquire(principal)
        .map_err(|_| rejected(StatusCode::TOO_MANY_REQUESTS, "room_connection_limit"))?;
    // Register before the transactional snapshot, so no commit can fall between catch-up and wait.
    let mut events = state.rooms.subscribe(&principal.room_id).await;
    let mut revocations = state.rooms.session_revocations(&principal.room_id).await;
    let (mut delivered, pending) = {
        let snapshot = state
            .store
            .snapshot_for(
                RoomMutationAuthority::ConnectorSession(&authorization),
                cursor.after_seq,
                200,
            )
            .await
            .map_err(ConnectorHttpError::from_persistence)?;
        if snapshot.resume_gap {
            return Err(rejected(StatusCode::CONFLICT, "connector_resync_required"));
        }
        (
            snapshot.last_seq,
            messages(snapshot.events, principal, true),
        )
    };
    if let Some(value) = wait_result(&state, &authorization, pending, delivered).await? {
        return Ok(Json(value));
    }
    let duration = (authorization.expires_at() - chrono::Utc::now())
        .to_std()
        .map_err(|_| rejected(StatusCode::FORBIDDEN, "session_revoked"))?;
    let expiry = tokio::time::sleep(duration);
    tokio::pin!(expiry);
    let transport = transport.ok_or_else(|| {
        rejected(
            StatusCode::INTERNAL_SERVER_ERROR,
            "http_connection_unavailable",
        )
    })?;
    let _transport_wait = transport.retain_authenticated_wait();
    loop {
        tokio::select! {
            () = state.shutdown.cancelled() => return Err(rejected(StatusCode::SERVICE_UNAVAILABLE,"server_stopping")),
            () = &mut expiry => return Err(rejected(StatusCode::FORBIDDEN,"session_revoked")),
            signal = revocations.recv() => {
                if signal.is_err() && !matches!(signal, Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) { return Err(rejected(StatusCode::GONE,"room_subscription_closed")); }
                state.store.revalidate_connector_session(&authorization, chrono::Utc::now()).await.map_err(ConnectorHttpError::from_persistence)?;
            }
            event = events.recv() => {
                let event = event.map_err(|_| rejected(StatusCode::CONFLICT,"connector_resync_required"))?;
                if event.seq <= delivered { continue; }
                if event.seq != delivered.saturating_add(1) { return Err(rejected(StatusCode::CONFLICT,"connector_resync_required")); }
                delivered = event.seq;
                let pending = messages(vec![event], principal, true);
                if let Some(value) = wait_result(&state, &authorization, pending, delivered).await? { return Ok(Json(value)); }
            }
        }
    }
}

async fn enrich(
    state: &AppState,
    authorization: &ConnectorSessionAuthorization,
    value: &mut Value,
    read_ids: &[String],
    acknowledge_page: bool,
) -> Result<(), ConnectorHttpError> {
    let principal = authorization.principal();
    let participant = state
        .store
        .participant(&principal.room_id, &principal.participant_id)
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    value["role"] = json!(participant.role);
    let allowed = state
        .store
        .delegated_pin_available(
            &agentsassemble_persistence::DelegateRoomAuthority::Connector(authorization.clone()),
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    value["delegated_actions"] = if allowed {
        json!(["set_message_pin"])
    } else {
        json!([])
    };
    let mut metadata = state
        .store
        .connector_mentions(authorization, read_ids, acknowledge_page)
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    if acknowledge_page {
        value["mention_messages"] = metadata["mention_messages"].take();
    }
    for key in [
        "conversation_mode",
        "guidance",
        "unread_mentions",
        "mention_overflow",
        "mention_resync_guidance",
    ] {
        value[key] = metadata[key].take();
    }
    Ok(())
}

async fn wait_result(
    state: &AppState,
    authorization: &ConnectorSessionAuthorization,
    pending: Vec<RoomEvent>,
    delivered: i64,
) -> Result<Option<Value>, ConnectorHttpError> {
    let mut metadata = state
        .store
        .connector_mentions(authorization, &[], false)
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let mentions = metadata["mention_messages"].take();
    if let Some(object) = metadata.as_object_mut() {
        object.remove("mention_messages");
    }
    let ordinary = state
        .store
        .connector_ordinary_messages(
            authorization,
            &pending.iter().map(|event| event.seq).collect::<Vec<_>>(),
        )
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    let mut messages = pending
        .into_iter()
        .filter(|event| ordinary.contains(&event.seq))
        .map(|event| json!(event))
        .collect::<Vec<_>>();
    if let Some(mentions) = mentions.as_array() {
        for mention in mentions {
            if messages.len() < SNAPSHOT_EVENTS
                && !messages.iter().any(|event| event["id"] == mention["id"])
            {
                messages.push(mention.clone());
            }
        }
    }
    messages.sort_by_key(|event| event["seq"].as_i64());
    if messages.is_empty() && metadata["mention_overflow"] != true {
        return Ok(None);
    }
    let pending = json!(messages);
    metadata["messages"] = pending;
    metadata["last_seq"] = json!(delivered);
    Ok(Some(metadata))
}

pub(super) async fn authorize_read(
    state: &AppState,
    request: Request,
    requested_events: usize,
) -> Result<ConnectorSessionAuthorization, ConnectorHttpError> {
    let fingerprint = super::credential(
        &request,
        agentsassemble_persistence::CONNECTOR_SESSION_PREFIX,
    )?;
    let authorization = state
        .store
        .authorize_connector_session(&fingerprint, chrono::Utc::now())
        .await
        .map_err(ConnectorHttpError::from_persistence)?;
    if !state
        .socket_admission
        .admit_history(authorization.principal(), requested_events)
    {
        return Err(rejected(StatusCode::TOO_MANY_REQUESTS, "room_read_limit"));
    }
    Ok(authorization)
}

fn messages(
    events: Vec<RoomEvent>,
    principal: &AuthenticatedPrincipal,
    other_only: bool,
) -> Vec<RoomEvent> {
    let mut messages: Vec<_> = events
        .into_iter()
        .map(|event| public_event_for_principal(&event, principal))
        .filter(|event| {
            event.event_type == "message_final"
                && (!other_only || event.actor.participant_id != principal.participant_id)
        })
        .rev()
        .take(if other_only {
            SNAPSHOT_EVENTS
        } else {
            MESSAGE_LIMIT
        })
        .collect();
    messages.reverse();
    messages
}

fn rejected(status: StatusCode, code: &'static str) -> ConnectorHttpError {
    ConnectorHttpError {
        status,
        code: code.into(),
        resolution: agentsassemble_protocol::CommandResolution::Rejected,
    }
}
