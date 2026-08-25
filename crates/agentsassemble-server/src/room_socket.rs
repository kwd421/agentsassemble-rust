use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, ProviderCatalog, RoomEvent, SnapshotMode, public_event_for_principal,
    public_settings,
};
use agentsassemble_persistence::{PersistenceError, RoomCatchUp};
use agentsassemble_protocol::{
    ClientFrame, CommandNack, CommandResolution, PROTOCOL_VERSION, ProtocolError, RoomSnapshot,
    RoomStream, ServerFrame, Subscribed,
};
use axum::extract::ws::Message;
use futures_util::{Sink, Stream, StreamExt};
use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    AppState, ConsumedTicket,
    authenticated_channel::{
        AuthenticatedChannel, encode_server_frame, send_plain_encoded, send_plain_frame,
    },
    server_proof::{challenge_is_valid, permissions_digest, sign_subscription, snapshot_digest},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_SUBSCRIPTION_CATCH_UP_EVENTS: i64 = 256;

pub(crate) struct EstablishedSubscription {
    pub principal: AuthenticatedPrincipal,
    pub events: broadcast::Receiver<RoomEvent>,
    pub catalog_updates: watch::Receiver<ProviderCatalog>,
    pub delivered_seq: i64,
    pub channel: AuthenticatedChannel,
}

struct ValidatedSubscription {
    streams: Vec<RoomStream>,
    resume_from_seq: i64,
    server_challenge: String,
}

struct PreparedSnapshot {
    events: broadcast::Receiver<RoomEvent>,
    catalog_updates: watch::Receiver<ProviderCatalog>,
    cursor: i64,
    encoded: String,
}

pub(crate) async fn establish<S, R>(
    sender: &mut S,
    receiver: &mut R,
    state: &AppState,
    grant: ConsumedTicket,
) -> Option<EstablishedSubscription>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    tokio::select! {
        () = state.shutdown.cancelled() => None,
        result = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            establish_before_deadline(sender, receiver, state, grant),
        ) => result.ok().flatten(),
    }
}

async fn establish_before_deadline<S, R>(
    sender: &mut S,
    receiver: &mut R,
    state: &AppState,
    grant: ConsumedTicket,
) -> Option<EstablishedSubscription>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    let ConsumedTicket {
        principal: ticket_principal,
        proof_key,
        connection_nonce,
    } = grant;
    let principal = resolve_principal(sender, state, &ticket_principal).await?;
    let request = receive_subscription(sender, receiver, state, &principal).await?;
    let prepared = prepare_snapshot(sender, state, &principal, request.resume_from_seq).await?;
    let catch_up = load_catch_up(sender, state, &principal, prepared.cursor).await?;
    let receipt = subscription_receipt(
        state,
        &principal,
        &request,
        &proof_key,
        connection_nonce.clone(),
        &prepared,
        catch_up.high_water,
    );
    send_subscription_start(sender, &state.shutdown, receipt, &prepared.encoded).await?;
    let mut channel = AuthenticatedChannel::new(proof_key, connection_nonce);
    let delivered_seq = send_catch_up(
        sender,
        &state.shutdown,
        &principal,
        prepared.cursor,
        catch_up,
        &mut channel,
    )
    .await?;
    Some(EstablishedSubscription {
        principal,
        events: prepared.events,
        catalog_updates: prepared.catalog_updates,
        delivered_seq,
        channel,
    })
}

async fn resolve_principal<S>(
    sender: &mut S,
    state: &AppState,
    ticket_principal: &AuthenticatedPrincipal,
) -> Option<AuthenticatedPrincipal>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    match state.store.resolve_principal(ticket_principal).await {
        Ok(principal) => Some(principal),
        Err(error) => {
            log_internal_persistence_error(&error, "principal resolution failed");
            let (code, message) = persistence_error(&error);
            let _ = send_nack(sender, &state.shutdown, "", "subscribe", &code, &message).await;
            None
        }
    }
}

async fn receive_subscription<S, R>(
    sender: &mut S,
    receiver: &mut R,
    state: &AppState,
    principal: &AuthenticatedPrincipal,
) -> Option<ValidatedSubscription>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
    R: Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    let Some(Ok(message)) = receiver.next().await else {
        return None;
    };
    let (frame_bytes, control_frame) = match &message {
        Message::Text(raw) => (raw.len(), false),
        Message::Binary(raw) => (raw.len(), false),
        Message::Ping(raw) | Message::Pong(raw) => (raw.len(), true),
        Message::Close(_) => return None,
    };
    if !state
        .raw_ingress
        .admit(principal, frame_bytes, control_frame)
    {
        let _ = send_nack(
            sender,
            &state.shutdown,
            "",
            "subscribe",
            "ingress_limited",
            "WebSocket ingress budget exceeded.",
        )
        .await;
        return None;
    }
    let Message::Text(raw) = message else {
        let _ = send_nack(
            sender,
            &state.shutdown,
            "",
            "subscribe",
            "subscribe_required",
            "The first frame must be a valid subscription.",
        )
        .await;
        return None;
    };
    let Ok(ClientFrame::Subscribe {
        streams,
        resume_from_seq,
        server_challenge,
    }) = serde_json::from_str(raw.as_str())
    else {
        let _ = send_nack(
            sender,
            &state.shutdown,
            "",
            "subscribe",
            "subscribe_required",
            "The first frame must be a valid subscription.",
        )
        .await;
        return None;
    };
    if streams != [RoomStream::RoomEvents] || resume_from_seq < 0 {
        let _ = send_nack(
            sender,
            &state.shutdown,
            "",
            "subscribe",
            "invalid_subscription",
            "room_events and a non-negative cursor are required.",
        )
        .await;
        return None;
    }
    if !challenge_is_valid(&server_challenge) {
        let _ = send_nack(
            sender,
            &state.shutdown,
            "",
            "subscribe",
            "server_challenge_invalid",
            "The server challenge must be 32 random bytes encoded as hexadecimal.",
        )
        .await;
        return None;
    }
    Some(ValidatedSubscription {
        streams,
        resume_from_seq,
        server_challenge,
    })
}

async fn prepare_snapshot<S>(
    sender: &mut S,
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    resume_from_seq: i64,
) -> Option<PreparedSnapshot>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    // Register the live receiver before taking any durable snapshot. It buffers every event
    // committed between the snapshot boundary and completion of finite catch-up delivery.
    let events = state.rooms.subscribe(&principal.room_id).await;
    let snapshot_data = match state
        .store
        .snapshot_for(principal, resume_from_seq, 200)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(PersistenceError::InvalidCursor { durable_last_seq }) => {
            let _ = send_plain_frame(
                sender,
                &state.shutdown,
                &ServerFrame::ResyncRequired {
                    stream: "room_events",
                    reason: "resume cursor is ahead of durable room state".to_owned(),
                    latest_seq: durable_last_seq,
                },
            )
            .await;
            return None;
        }
        Err(error) => {
            log_internal_persistence_error(&error, "room snapshot failed");
            let _ = send_nack(
                sender,
                &state.shutdown,
                "",
                "subscribe",
                "snapshot_failed",
                "Room snapshot failed.",
            )
            .await;
            return None;
        }
    };
    let settings = match public_settings(&snapshot_data.settings) {
        Ok(settings) => settings,
        Err(error) => {
            let _ = send_nack(
                sender,
                &state.shutdown,
                "",
                "subscribe",
                "snapshot_failed",
                &error.to_string(),
            )
            .await;
            return None;
        }
    };
    let mut catalog_updates = state.provider_catalog.subscribe();
    let provider_catalog = catalog_updates.borrow_and_update().clone();
    let snapshot_cursor = snapshot_data.last_seq;
    let snapshot = RoomSnapshot {
        stream: "room_events",
        room: snapshot_data.room,
        room_settings: settings,
        participants: snapshot_data.participants,
        agent_sessions: snapshot_data.agent_sessions,
        provider_requests: Vec::new(),
        active_turns: Vec::new(),
        events: snapshot_data
            .events
            .iter()
            .map(|event| public_event_for_principal(event, principal))
            .collect(),
        oldest_seq: snapshot_data.oldest_seq,
        last_seq: snapshot_cursor,
        has_more_before: snapshot_data.has_more_before,
        resume_gap: snapshot_data.resume_gap,
        snapshot_mode: snapshot_data.snapshot_mode,
        available_providers: provider_catalog.providers.clone(),
        provider_catalog,
        capabilities: principal.capabilities.clone(),
    };
    let Some(encoded_snapshot) = fit_snapshot_frame(snapshot) else {
        let _ = send_nack(
            sender,
            &state.shutdown,
            "",
            "subscribe",
            "snapshot_too_large",
            "Room metadata exceeds the WebSocket snapshot limit.",
        )
        .await;
        return None;
    };
    Some(PreparedSnapshot {
        events,
        catalog_updates,
        cursor: snapshot_cursor,
        encoded: encoded_snapshot,
    })
}

async fn load_catch_up<S>(
    sender: &mut S,
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    snapshot_cursor: i64,
) -> Option<RoomCatchUp>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    let catch_up = match state
        .store
        .room_subscription_catch_up(principal, snapshot_cursor, MAX_SUBSCRIPTION_CATCH_UP_EVENTS)
        .await
    {
        Ok(catch_up) => catch_up,
        Err(error) => {
            log_internal_persistence_error(&error, "room subscription catch-up failed");
            let (code, message) = match error {
                PersistenceError::SubscriptionCatchUpExceeded { .. } => (
                    "subscription_catchup_exceeded",
                    "Subscription catch-up exceeded its bounded delivery window.".to_owned(),
                ),
                _ => (
                    "subscription_catchup_failed",
                    "Subscription catch-up failed.".to_owned(),
                ),
            };
            let _ = send_nack(sender, &state.shutdown, "", "subscribe", code, &message).await;
            return None;
        }
    };
    Some(catch_up)
}

fn subscription_receipt(
    state: &AppState,
    principal: &AuthenticatedPrincipal,
    request: &ValidatedSubscription,
    proof_key: &str,
    connection_nonce: String,
    prepared: &PreparedSnapshot,
    catchup_high_water: i64,
) -> Subscribed {
    let mut receipt = Subscribed {
        streams: request.streams.clone(),
        protocol_version: PROTOCOL_VERSION,
        server_challenge: request.server_challenge.clone(),
        connection_nonce,
        room_id: principal.room_id.clone(),
        principal_id: principal.principal_id.clone(),
        participant_id: principal.participant_id.clone(),
        server_surface_revision: state.server_product_surface.revision,
        server_surface_digest: state.server_product_surface.digest.clone(),
        permissions_digest: permissions_digest(&principal.capabilities),
        snapshot_cursor: prepared.cursor,
        catchup_high_water,
        snapshot_digest: snapshot_digest(&prepared.encoded),
        proof: String::new(),
    };
    receipt.proof = sign_subscription(proof_key, &receipt);
    receipt
}

async fn send_subscription_start<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    receipt: Subscribed,
    encoded_snapshot: &str,
) -> Option<()>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    let receipt_frame = ServerFrame::Subscribed(Box::new(receipt));
    let Ok(encoded_receipt) = encode_server_frame(&receipt_frame) else {
        return None;
    };
    if send_plain_encoded(sender, cancellation, encoded_receipt)
        .await
        .is_err()
        || send_plain_encoded(sender, cancellation, encoded_snapshot.to_owned())
            .await
            .is_err()
    {
        return None;
    }
    Some(())
}

async fn send_catch_up<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    principal: &AuthenticatedPrincipal,
    snapshot_cursor: i64,
    catch_up: RoomCatchUp,
    channel: &mut AuthenticatedChannel,
) -> Option<i64>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    let mut delivered_seq = snapshot_cursor;
    for event in catch_up.events {
        if event.seq != delivered_seq.saturating_add(1) {
            return None;
        }
        let frame = ServerFrame::Event {
            stream: "room_events",
            latest_seq: event.seq,
            events: vec![public_event_for_principal(&event, principal)],
        };
        if channel.send(sender, cancellation, &frame).await.is_err() {
            return None;
        }
        delivered_seq = event.seq;
    }
    if delivered_seq != catch_up.high_water {
        return None;
    }
    Some(delivered_seq)
}

pub(crate) async fn send_nack<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    request_id: &str,
    action: &str,
    code: &str,
    message: &str,
) -> Result<(), axum::Error>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    send_plain_frame(
        sender,
        cancellation,
        &ServerFrame::Nack(CommandNack {
            request_id: request_id.to_owned(),
            accepted: false,
            resolution: CommandResolution::Unresolved,
            action: action.to_owned(),
            error: ProtocolError {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        }),
    )
    .await
}

fn fit_snapshot_frame(mut snapshot: RoomSnapshot) -> Option<String> {
    loop {
        let frame = ServerFrame::Snapshot(Box::new(snapshot.clone()));
        if let Ok(encoded) = encode_server_frame(&frame) {
            return Some(encoded);
        }
        if snapshot.events.is_empty() {
            return None;
        }
        let remove = (snapshot.events.len() / 2).max(1);
        snapshot.events.drain(..remove);
        snapshot.oldest_seq = snapshot
            .events
            .first()
            .map_or(snapshot.last_seq, |event| event.seq);
        snapshot.has_more_before = true;
        if snapshot.snapshot_mode != SnapshotMode::Initial {
            snapshot.resume_gap = true;
            snapshot.snapshot_mode = SnapshotMode::Gap;
        }
    }
}

pub(crate) fn persistence_error(error: &PersistenceError) -> (String, String) {
    match error {
        PersistenceError::CommandConflict => ("command_conflict".to_owned(), error.to_string()),
        PersistenceError::CommandRejected { code, message }
        | PersistenceError::CommandUnresolved { code, message } => {
            ((*code).to_owned(), message.clone())
        }
        PersistenceError::StoredCommandRejected { code, message } => {
            (code.clone(), message.clone())
        }
        PersistenceError::ParticipantMissing => ("session_revoked".to_owned(), error.to_string()),
        PersistenceError::RoomMissing => ("room_not_found".to_owned(), error.to_string()),
        PersistenceError::InvalidCursor { .. } => (
            "invalid_cursor".to_owned(),
            "Room cursor is invalid.".to_owned(),
        ),
        PersistenceError::Database(_)
        | PersistenceError::Json(_)
        | PersistenceError::RuntimeAuthorityTask(_)
        | PersistenceError::AuthorityConflict(_)
        | PersistenceError::UnownedDatabase
        | PersistenceError::WriterAlreadyActive(_)
        | PersistenceError::WriterLease(_)
        | PersistenceError::UnsafeDatabasePath(_)
        | PersistenceError::InitializationNotAllowed
        | PersistenceError::InvalidSchemaVersion(_)
        | PersistenceError::SchemaVersionMismatch { .. }
        | PersistenceError::InvalidServerId
        | PersistenceError::SubscriptionCatchUpExceeded { .. }
        | PersistenceError::SubscriptionSequenceGap { .. } => (
            "persistence_failed".to_owned(),
            "Persistence operation failed.".to_owned(),
        ),
    }
}

pub(crate) fn persistence_error_is_internal(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::Database(_)
            | PersistenceError::Json(_)
            | PersistenceError::RuntimeAuthorityTask(_)
            | PersistenceError::AuthorityConflict(_)
            | PersistenceError::UnownedDatabase
            | PersistenceError::WriterAlreadyActive(_)
            | PersistenceError::WriterLease(_)
            | PersistenceError::UnsafeDatabasePath(_)
            | PersistenceError::InitializationNotAllowed
            | PersistenceError::InvalidSchemaVersion(_)
            | PersistenceError::SchemaVersionMismatch { .. }
            | PersistenceError::InvalidServerId
            | PersistenceError::SubscriptionSequenceGap { .. }
    )
}

fn log_internal_persistence_error(error: &PersistenceError, operation: &str) {
    if persistence_error_is_internal(error) {
        tracing::error!(error = ?error, operation, "WebSocket subscription persistence failed");
    }
}
