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
    Ok(Json(
        json!({"room":snapshot.room,"participants":snapshot.participants,"messages":messages(snapshot.events, authorization.principal(), false),"last_seq":snapshot.last_seq,"resync_required":snapshot.resume_gap}),
    ))
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
    Ok(Json(json!(result)))
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
    Ok(Json(json!(result)))
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
    if !pending.is_empty() {
        return Ok(Json(json!({"messages":pending,"last_seq":delivered})));
    }
    let duration = (authorization.expires_at() - chrono::Utc::now())
        .to_std()
        .map_err(|_| rejected(StatusCode::FORBIDDEN, "session_revoked"))?;
    let expiry = tokio::time::sleep(duration);
    tokio::pin!(expiry);
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
                state.store.revalidate_connector_session(&authorization, chrono::Utc::now()).await.map_err(ConnectorHttpError::from_persistence)?;
                if event.seq <= delivered { continue; }
                if event.seq != delivered.saturating_add(1) { return Err(rejected(StatusCode::CONFLICT,"connector_resync_required")); }
                delivered = event.seq;
                let pending = messages(vec![event], principal, true);
                if !pending.is_empty() { return Ok(Json(json!({"messages":pending,"last_seq":delivered}))); }
            }
        }
    }
}

async fn authorize_read(
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
