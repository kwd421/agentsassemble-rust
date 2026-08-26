use std::time::Duration;

use agentsassemble_domain::public_event_for_principal;
use agentsassemble_protocol::{
    ClientFrame, CommandAck, CommandResolution, ProtocolError, ServerFrame,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;

use crate::{
    AppState, ConsumedTicket,
    connection_admission::ConnectionLease,
    room_socket::{
        EstablishedSubscription, establish, persistence_error, persistence_error_is_internal,
    },
};

const SOCKET_IDLE_TIMEOUT: Duration = Duration::from_mins(5);

#[allow(clippy::too_many_lines)] // One select loop owns the socket's ordering and lifecycle.
pub(crate) async fn run(
    socket: WebSocket,
    state: AppState,
    grant: ConsumedTicket,
    _lease: ConnectionLease,
) {
    let (mut sender, mut receiver) = socket.split();
    let Some(EstablishedSubscription {
        principal,
        mut events,
        mut catalog_updates,
        mut delivered_seq,
        mut channel,
    }) = establish(&mut sender, &mut receiver, &state, grant).await
    else {
        return;
    };
    loop {
        tokio::select! {
            () = state.shutdown.cancelled() => return,
            incoming = tokio::time::timeout(SOCKET_IDLE_TIMEOUT, receiver.next()) => {
                let Ok(Some(Ok(message))) = incoming else { return; };
                let (frame_bytes, control_frame) = match &message {
                    Message::Text(raw) => (raw.len(), false),
                    Message::Binary(raw) => (raw.len(), false),
                    Message::Ping(raw) | Message::Pong(raw) => (raw.len(), true),
                    Message::Close(_) => return,
                };
                if !state.raw_ingress.admit(&principal, frame_bytes, control_frame) {
                    let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", CommandResolution::Unresolved, ProtocolError::new("ingress_limited", "WebSocket ingress budget exceeded.")).await;
                    return;
                }
                let Message::Text(raw) = message else {
                    if matches!(message, Message::Binary(_)) {
                        let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", CommandResolution::Unresolved, ProtocolError::new("binary_frame_unsupported", "Binary WebSocket frames are not supported.")).await;
                        return;
                    }
                    continue;
                };
                let Ok((client_frame, _authenticated_bytes)) =
                    channel.decode_client(raw.as_str())
                else {
                    let _ = channel.send_nack(&mut sender, &state.shutdown, "", "frame", CommandResolution::Unresolved, ProtocolError::new("frame_authentication_invalid", "WebSocket frame authentication failed.")).await;
                    return;
                };
                match client_frame {
                    ClientFrame::Command { request_id, action, payload } => {
                        let action_name = action.as_str().to_owned();
                        let outcome = state.rooms.execute(
                            principal.clone(), request_id.clone(), action, payload,
                        ).await;
                        match outcome {
                            Ok(outcome) => {
                                let frame = ServerFrame::Ack(CommandAck {
                                    request_id,
                                    accepted: true,
                                    resolution: CommandResolution::Committed,
                                    action: action_name,
                                    result: outcome.result,
                                    deduplicated: outcome.deduplicated,
                                });
                                if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                            }
                            Err(failure) => {
                                if persistence_error_is_internal(&failure.error) {
                                    tracing::error!(error = ?failure.error, room_id = %principal.room_id, action = %action_name, "room command persistence failed");
                                }
                                let (code, message) = persistence_error(&failure.error);
                                if channel.send_nack(&mut sender, &state.shutdown, &request_id, &action_name, failure.resolution, ProtocolError::new(code, message)).await.is_err() { return; }
                            }
                        }
                    }
                    ClientFrame::Ping { nonce } => {
                        if channel.send(&mut sender, &state.shutdown, &ServerFrame::Pong { nonce }).await.is_err() { return; }
                    }
                    ClientFrame::Subscribe { .. } => {
                        if channel.send_nack(&mut sender, &state.shutdown, "", "subscribe", CommandResolution::Unresolved, ProtocolError::new("already_subscribed", "This socket is already subscribed.")).await.is_err() { return; }
                    }
                }
            }
            published = events.recv() => {
                match published {
                    Ok(event) => {
                        if event.seq <= delivered_seq {
                            continue;
                        }
                        if event.seq != delivered_seq.saturating_add(1) {
                            let frame = ServerFrame::ResyncRequired {
                                stream: "room_events",
                                reason: "live room event sequence is not contiguous".to_owned(),
                                latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(delivered_seq, |snapshot| snapshot.last_seq),
                            };
                            let _ = channel.send(&mut sender, &state.shutdown, &frame).await;
                            return;
                        }
                        let current_principal = match state.store.resolve_principal(&principal).await {
                            Ok(principal) => principal,
                            Err(error) => {
                                if persistence_error_is_internal(&error) {
                                    tracing::error!(error = ?error, room_id = %principal.room_id, "live principal resolution failed");
                                }
                                let (code, message) = persistence_error(&error);
                                let _ = channel.send_nack(
                                    &mut sender,
                                    &state.shutdown,
                                    "",
                                    "session",
                                    CommandResolution::Unresolved,
                                    ProtocolError::new(code, message),
                                ).await;
                                return;
                            }
                        };
                        let latest_seq = event.seq;
                        let frame = ServerFrame::Event {
                            stream: "room_events",
                            events: vec![public_event_for_principal(&event, &current_principal)],
                            latest_seq,
                        };
                        if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
                        delivered_seq = latest_seq;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let frame = ServerFrame::ResyncRequired {
                            stream: "room_events",
                            reason: "subscriber fell behind the room event stream".to_owned(),
                            latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(0, |snapshot| snapshot.last_seq),
                        };
                        let _ = channel.send(&mut sender, &state.shutdown, &frame).await;
                        return;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            changed = catalog_updates.changed() => {
                if changed.is_err() {
                    continue;
                }
                let frame = ServerFrame::ProviderCatalogUpdated {
                    catalog: catalog_updates.borrow_and_update().clone(),
                };
                if channel.send(&mut sender, &state.shutdown, &frame).await.is_err() { return; }
            }
        }
    }
}
