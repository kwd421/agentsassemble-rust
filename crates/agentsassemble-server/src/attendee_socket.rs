use std::time::Duration;

use crate::attendee_wire::AttendeeSocketFrame as Frame;
use agentsassemble_persistence::{AttendeeConnectionAuthorization, AttendeeSessionAuthorization};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{StreamExt, stream::SplitSink};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::socket_protocol::{Request, failure};
use crate::{
    AppState, AttendeeOperation, AttendeeOperationResult, connection_admission::ConnectionLease,
    room_channel::send_encoded,
};

pub(super) async fn run(
    socket: WebSocket,
    state: AppState,
    session: AttendeeSessionAuthorization,
    _lease: ConnectionLease,
) {
    // Subscribe before claiming custody so a concurrent assignment/revocation cannot be lost.
    let room_id = &session.principal().room_id;
    let events = state.rooms.subscribe(room_id).await;
    let revocations = state.rooms.session_revocations(room_id).await;
    let claim = state
        .rooms
        .execute_attendee(AttendeeOperation::Connect {
            session,
            connection_id: Uuid::new_v4(),
        })
        .await;
    let Ok(AttendeeOperationResult::Connected(connection)) = claim else {
        return;
    };
    run_connected(socket, &state, &connection, events, revocations).await;
    if let Err(error) = state
        .rooms
        .execute_attendee(AttendeeOperation::Disconnect { connection })
        .await
    {
        let code = match error {
            agentsassemble_persistence::PersistenceError::CommandRejected { code, .. } => code,
            _ => "attendee_disconnect_failed",
        };
        tracing::warn!(code, "external attendee network cleanup did not commit");
    }
}

async fn run_connected(
    socket: WebSocket,
    state: &AppState,
    connection: &AttendeeConnectionAuthorization,
    mut events: broadcast::Receiver<agentsassemble_domain::RoomEvent>,
    mut revocations: broadcast::Receiver<[u8; 32]>,
) {
    let (mut sender, mut receiver) = socket.split();
    let remaining = connection
        .session()
        .expires_at()
        .signed_duration_since(chrono::Utc::now())
        .to_std()
        .unwrap_or(Duration::ZERO);
    let expiry = tokio::time::sleep(remaining);
    tokio::pin!(expiry);
    if send(
        state,
        connection,
        &mut sender,
        Frame::Connected {
            connection_id: connection.connection_id(),
        },
    )
    .await
    .is_none()
    {
        return;
    }
    let mut ready = false;
    let mut delivered = None;
    let mut delivered_interrupt = None;
    let idle = tokio::time::sleep(Duration::from_mins(5));
    tokio::pin!(idle);
    loop {
        if deliver_stop(state, connection, &mut sender).await != Some(false) {
            break;
        }
        let Some(interrupt_pending) =
            deliver_interrupt(state, connection, &mut sender, &mut delivered_interrupt).await
        else {
            break;
        };
        if ready
            && !interrupt_pending
            && deliver(state, connection, &mut sender, &mut delivered)
                .await
                .is_none()
        {
            break;
        }
        tokio::select! {
            () = state.shutdown.cancelled() => break,
            () = &mut expiry => break,
            () = &mut idle => break,
            message = receiver.next() => {
                let Some(Ok(message)) = message else { break; };
                idle.as_mut().reset(tokio::time::Instant::now() + Duration::from_mins(5));
                let Some(became_ready) = receive(state, connection, &mut sender, message).await else { break; };
                ready |= became_ready;
            }
            event = events.recv() => {
                if matches!(event, Err(broadcast::error::RecvError::Closed)) { break; }
                // A lagged wake still reloads the one canonical outstanding assignment below.
            }
            signal = revocations.recv() => {
                if matches!(signal, Err(broadcast::error::RecvError::Closed)) { break; }
            }
        }
        if state
            .store
            .revalidate_attendee_connection(connection, chrono::Utc::now())
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn receive(
    state: &AppState,
    connection: &AttendeeConnectionAuthorization,
    sender: &mut SplitSink<WebSocket, Message>,
    message: Message,
) -> Option<bool> {
    let (bytes, control) = match &message {
        Message::Text(text) => (text.len(), false),
        Message::Binary(bytes) => (bytes.len(), false),
        Message::Ping(bytes) | Message::Pong(bytes) => (bytes.len(), true),
        Message::Close(_) => return None,
    };
    if !state
        .socket_admission
        .admit_frame(connection.session().principal(), bytes, control)
    {
        return None;
    }
    let Message::Text(encoded) = message else {
        return matches!(message, Message::Ping(_) | Message::Pong(_)).then_some(false);
    };
    let request: Request = serde_json::from_str(&encoded).ok()?;
    let request_id = request.request_id();
    let (response, ready) = match request.apply(state, connection).await {
        Ok(result) => result,
        Err(error) => (failure(request_id, error), false),
    };
    send(state, connection, sender, response).await?;
    Some(ready)
}

async fn deliver_interrupt(
    state: &AppState,
    connection: &AttendeeConnectionAuthorization,
    sender: &mut SplitSink<WebSocket, Message>,
    delivered: &mut Option<String>,
) -> Option<bool> {
    let interrupt = match state
        .store
        .deliver_attendee_interrupt(connection, chrono::Utc::now())
        .await
    {
        Ok(Some(interrupt)) => interrupt,
        Ok(None) => {
            *delivered = None;
            return Some(false);
        }
        Err(error) => {
            send(state, connection, sender, failure(Uuid::nil(), error)).await?;
            return None;
        }
    };
    if delivered.as_ref() != Some(&interrupt.effect_id) {
        let effect_id = interrupt.effect_id.clone();
        send(
            state,
            connection,
            sender,
            Frame::Interrupt {
                interrupt: Box::new(interrupt),
            },
        )
        .await?;
        *delivered = Some(effect_id);
    }
    Some(true)
}

async fn deliver_stop(
    state: &AppState,
    connection: &AttendeeConnectionAuthorization,
    sender: &mut SplitSink<WebSocket, Message>,
) -> Option<bool> {
    let authority = state
        .store
        .authorize_attendee_cleanup(connection.session().session_fingerprint())
        .await
        .ok()?;
    let stop = state.store.load_attendee_cleanup(&authority).await.ok()?;
    let Some(stop) = stop else {
        return Some(false);
    };
    send(state, connection, sender, Frame::Stop { stop }).await?;
    // The positive report uses HTTP so revocation cannot lose its acknowledgement.
    Some(true)
}

async fn deliver(
    state: &AppState,
    connection: &AttendeeConnectionAuthorization,
    sender: &mut SplitSink<WebSocket, Message>,
    delivered: &mut Option<String>,
) -> Option<()> {
    let assignment = state
        .store
        .deliver_attendee_turn(connection, chrono::Utc::now())
        .await;
    let assignment = match assignment {
        Ok(Some(assignment)) => assignment,
        Ok(None) => {
            *delivered = None;
            return Some(());
        }
        Err(error) => {
            send(state, connection, sender, failure(Uuid::nil(), error)).await?;
            return None;
        }
    };
    if delivered.as_ref() == Some(&assignment.authority.execution_id) {
        return Some(());
    }
    let execution_id = assignment.authority.execution_id.clone();
    send(
        state,
        connection,
        sender,
        Frame::Turn {
            assignment: Box::new(assignment),
        },
    )
    .await?;
    *delivered = Some(execution_id);
    Some(())
}

async fn send(
    state: &AppState,
    connection: &AttendeeConnectionAuthorization,
    sender: &mut SplitSink<WebSocket, Message>,
    value: Frame,
) -> Option<()> {
    state
        .store
        .revalidate_attendee_connection(connection, chrono::Utc::now())
        .await
        .ok()?;
    let encoded = serde_json::to_string(&value).ok()?;
    if encoded.len() > agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES {
        return None;
    }
    send_encoded(sender, &state.shutdown, encoded).await.ok()
}
