use std::time::Duration;

use agentsassemble_domain::public_event_for_principal;
use agentsassemble_persistence::HumanSessionAuthorization;
use agentsassemble_protocol::{
    ClientFrame, CommandAck, CommandResolution, ProtocolError, ServerFrame,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use tokio::sync::broadcast;

use crate::{
    AppState,
    authenticated_channel::AuthenticatedChannel,
    connection_admission::ConnectionLease,
    room_socket::{
        EstablishedSubscription, establish, persistence_error, persistence_error_is_internal,
        refresh_human_session,
    },
    ticket::ConsumedSocketTicket,
};

const SOCKET_IDLE_TIMEOUT: Duration = Duration::from_mins(5);

#[allow(clippy::too_many_lines)] // One select loop owns the socket's ordering and lifecycle.
pub(crate) async fn run(
    socket: WebSocket,
    state: AppState,
    grant: ConsumedSocketTicket,
    mut revocations: Option<broadcast::Receiver<[u8; 32]>>,
    _lease: ConnectionLease,
) {
    let (mut sender, mut receiver) = socket.split();
    let Some(EstablishedSubscription {
        principal,
        human_session,
        mut events,
        mut catalog_updates,
        mut delivered_seq,
        mut channel,
    }) = establish(&mut sender, &mut receiver, &state, grant).await
    else {
        return;
    };
    let mut principal = principal;
    let mut human_session = human_session;
    let expiry = wait_for_session_expiry(
        human_session
            .as_ref()
            .map(HumanSessionAuthorization::expires_at),
    );
    tokio::pin!(expiry);
    loop {
        tokio::select! {
            () = state.shutdown.cancelled() => return,
            () = &mut expiry => return,
            revoked = receive_revocation(&mut revocations), if revocations.is_some() => {
                match revoked {
                    Ok(fingerprint) => {
                        if human_session.as_ref().is_some_and(|authorization| {
                            authorization.session_fingerprint() == &fingerprint
                        }) {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if refresh_human_session(&state, &mut principal, &mut human_session).await.is_none() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let _ = refresh_human_session(&state, &mut principal, &mut human_session).await;
                        return;
                    }
                }
            }
            incoming = tokio::time::timeout(SOCKET_IDLE_TIMEOUT, receiver.next()) => {
                let Ok(Some(Ok(message))) = incoming else { return; };
                let (frame_bytes, control_frame) = match &message {
                    Message::Text(raw) => (raw.len(), false),
                    Message::Binary(raw) => (raw.len(), false),
                    Message::Ping(raw) | Message::Pong(raw) => (raw.len(), true),
                    Message::Close(_) => return,
                };
                if !state.raw_ingress.admit(&principal, frame_bytes, control_frame) {
                    let _ = send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, ("", "frame", CommandResolution::Unresolved, ProtocolError::new("ingress_limited", "WebSocket ingress budget exceeded."))).await;
                    return;
                }
                let Message::Text(raw) = message else {
                    if matches!(message, Message::Binary(_)) {
                        let _ = send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, ("", "frame", CommandResolution::Unresolved, ProtocolError::new("binary_frame_unsupported", "Binary WebSocket frames are not supported."))).await;
                        return;
                    }
                    continue;
                };
                let Ok((client_frame, _authenticated_bytes)) =
                    channel.decode_client(raw.as_str())
                else {
                    let _ = send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, ("", "frame", CommandResolution::Unresolved, ProtocolError::new("frame_authentication_invalid", "WebSocket frame authentication failed."))).await;
                    return;
                };
                if refresh_human_session(&state, &mut principal, &mut human_session).await.is_none() {
                    return;
                }
                match client_frame {
                    ClientFrame::Command { request_id, action, payload } => {
                        let action_name = action.as_str().to_owned();
                        let outcome = if let Some(authorization) = &human_session {
                            state.rooms.execute_human_session(
                                authorization, request_id.clone(), action, payload,
                            ).await
                        } else {
                            state.rooms.execute(
                                principal.clone(), request_id.clone(), action, payload,
                            ).await
                        };
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
                                if send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await.is_none() { return; }
                            }
                            Err(failure) => {
                                if persistence_error_is_internal(&failure.error) {
                                    tracing::error!(error = ?failure.error, room_id = %principal.room_id, action = %action_name, "room command persistence failed");
                                }
                                let (code, message) = persistence_error(&failure.error);
                                if send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, (&request_id, &action_name, failure.resolution, ProtocolError::new(code, message))).await.is_none() { return; }
                            }
                        }
                    }
                    ClientFrame::Ping { nonce } => {
                        if send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &ServerFrame::Pong { nonce }).await.is_none() { return; }
                    }
                    ClientFrame::Subscribe { .. } => {
                        if send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, ("", "subscribe", CommandResolution::Unresolved, ProtocolError::new("already_subscribed", "This socket is already subscribed."))).await.is_none() { return; }
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
                            let _ = send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await;
                            return;
                        }
                        let current_principal = if human_session.is_some() {
                            if refresh_human_session(&state, &mut principal, &mut human_session).await.is_none() {
                                return;
                            }
                            principal.clone()
                        } else {
                            match state.store.resolve_principal(&principal).await {
                                Ok(principal) => principal,
                                Err(error) => {
                                    if persistence_error_is_internal(&error) {
                                        tracing::error!(error = ?error, room_id = %principal.room_id, "live principal resolution failed");
                                    }
                                    let (code, message) = persistence_error(&error);
                                    let _ = send_authorized_nack(
                                        &state,
                                        &mut principal,
                                        &mut human_session,
                                        &mut channel,
                                        &mut sender,
                                        ("", "session", CommandResolution::Unresolved, ProtocolError::new(code, message)),
                                    ).await;
                                    return;
                                }
                            }
                        };
                        let latest_seq = event.seq;
                        let frame = ServerFrame::Event {
                            stream: "room_events",
                            events: vec![public_event_for_principal(&event, &current_principal)],
                            latest_seq,
                        };
                        if send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await.is_none() { return; }
                        delivered_seq = latest_seq;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let frame = ServerFrame::ResyncRequired {
                            stream: "room_events",
                            reason: "subscriber fell behind the room event stream".to_owned(),
                            latest_seq: state.store.snapshot(&principal.room_id, 0, 1).await.map_or(0, |snapshot| snapshot.last_seq),
                        };
                        let _ = send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await;
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
                if send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await.is_none() { return; }
            }
        }
    }
}

async fn wait_for_session_expiry(expires_at: Option<chrono::DateTime<chrono::Utc>>) {
    let Some(expires_at) = expires_at else {
        std::future::pending::<()>().await;
        return;
    };
    let remaining = expires_at
        .signed_duration_since(chrono::Utc::now())
        .to_std()
        .unwrap_or(Duration::ZERO);
    tokio::time::sleep(remaining).await;
}

async fn receive_revocation(
    revocations: &mut Option<broadcast::Receiver<[u8; 32]>>,
) -> Result<[u8; 32], broadcast::error::RecvError> {
    match revocations {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn send_authorized_frame(
    state: &AppState,
    principal: &mut agentsassemble_domain::AuthenticatedPrincipal,
    human_session: &mut Option<HumanSessionAuthorization>,
    channel: &mut AuthenticatedChannel,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    frame: &ServerFrame,
) -> Option<()> {
    refresh_human_session(state, principal, human_session).await?;
    channel.send(sender, &state.shutdown, frame).await.ok()
}

async fn send_authorized_nack(
    state: &AppState,
    principal: &mut agentsassemble_domain::AuthenticatedPrincipal,
    human_session: &mut Option<HumanSessionAuthorization>,
    channel: &mut AuthenticatedChannel,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    nack: (&str, &str, CommandResolution, ProtocolError),
) -> Option<()> {
    refresh_human_session(state, principal, human_session).await?;
    channel
        .send_nack(sender, &state.shutdown, nack.0, nack.1, nack.2, nack.3)
        .await
        .ok()
}
