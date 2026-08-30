use std::time::Duration;

use agentsassemble_domain::public_event_for_principal;
use agentsassemble_persistence::HumanSessionAuthorization;
use agentsassemble_protocol::{
    ClientFrame, CommandAck, CommandResolution, ProtocolError, RoomAction, ServerFrame,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use tokio::sync::broadcast;

use crate::{
    AppState,
    authenticated_channel::AuthenticatedChannel,
    connection_admission::ConnectionLease,
    room_history_socket::read_history_frame,
    room_socket::{
        EstablishedSubscription, establish, persistence_error, persistence_error_is_internal,
        refresh_human_session,
    },
    room_vote_socket::read_vote_summary_frame,
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
                if !session_remains_authorized_after_revocation_signal(
                    &state,
                    &mut principal,
                    &mut human_session,
                    revoked,
                ).await {
                    return;
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
                if !state.socket_admission.admit_frame(&principal, frame_bytes, control_frame) {
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
                        if action == RoomAction::RoomHistory {
                            match read_history_frame(
                                &state.store,
                                &state.socket_admission,
                                &principal,
                                &request_id,
                                &payload,
                            ).await {
                                Ok(frame) => {
                                    if send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await.is_none() { return; }
                                }
                                Err(failure) => {
                                    if persistence_error_is_internal(&failure.error) {
                                        tracing::error!(error = ?failure.error, room_id = %principal.room_id, action = %action.as_str(), "room history read failed");
                                    }
                                    let (code, message) = persistence_error(&failure.error);
                                    if send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, (&request_id, action.as_str(), failure.resolution, ProtocolError::new(code, message))).await.is_none() { return; }
                                }
                            }
                            continue;
                        }
                        if action == RoomAction::RoomVoteSummary {
                            match read_vote_summary_frame(
                                &state.store,
                                &principal,
                                human_session.as_ref(),
                                &request_id,
                                &payload,
                            ).await {
                                Ok(frame) => {
                                    if send_authorized_frame(&state, &mut principal, &mut human_session, &mut channel, &mut sender, &frame).await.is_none() { return; }
                                }
                                Err(failure) => {
                                    if persistence_error_is_internal(&failure.error) {
                                        tracing::error!(error = ?failure.error, room_id = %principal.room_id, action = %action.as_str(), "room vote summary read failed");
                                    }
                                    let (code, message) = persistence_error(&failure.error);
                                    if send_authorized_nack(&state, &mut principal, &mut human_session, &mut channel, &mut sender, (&request_id, action.as_str(), failure.resolution, ProtocolError::new(code, message))).await.is_none() { return; }
                                }
                            }
                            continue;
                        }
                        let closes_human_session =
                            action == RoomAction::ParticipantLeave
                                && human_session.is_some();
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
                                if closes_human_session {
                                    // This exact committed ACK is the final frame authorized by
                                    // the command that revoked the session. Revalidation would
                                    // suppress it and leave the copied browser flow unresolved.
                                    let _ = channel.send(&mut sender, &state.shutdown, &frame).await;
                                    return;
                                }
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

async fn session_remains_authorized_after_revocation_signal(
    state: &AppState,
    principal: &mut agentsassemble_domain::AuthenticatedPrincipal,
    human_session: &mut Option<HumanSessionAuthorization>,
    signal: Result<[u8; 32], broadcast::error::RecvError>,
) -> bool {
    match signal {
        Ok(fingerprint) => human_session
            .as_ref()
            .is_none_or(|authorization| authorization.session_fingerprint() != &fingerprint),
        Err(broadcast::error::RecvError::Lagged(_)) => {
            refresh_human_session(state, principal, human_session)
                .await
                .is_some()
        }
        Err(broadcast::error::RecvError::Closed) => {
            let _ = refresh_human_session(state, principal, human_session).await;
            false
        }
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use agentsassemble_domain::{ProviderCatalog, UserProfilePatch};
    use agentsassemble_provider::ProviderCatalogService;
    use tokio::sync::broadcast;

    use super::{receive_revocation, session_remains_authorized_after_revocation_signal};
    use crate::{AppState, HostSecret, TicketStore, ticket_tests::HumanSessionFixture};

    #[tokio::test]
    async fn lagged_revocations_revalidate_and_closed_notification_fails_closed() {
        let fixture = HumanSessionFixture::new(1).await;
        let authorization = fixture.authorize(0).await;
        let mut principal = authorization.principal().clone();
        let mut human_session = Some(authorization);
        let state = AppState::local(
            fixture.store().clone(),
            TicketStore::new(Duration::from_secs(30), 8),
            HostSecret::new("socket-revocation-test-host-secret")
                .unwrap_or_else(|error| panic!("construct test host secret: {error}")),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        )
        .await
        .unwrap_or_else(|error| panic!("construct socket revocation state: {error}"));

        fixture
            .store()
            .update_human_session_profile(
                human_session
                    .as_ref()
                    .unwrap_or_else(|| panic!("test human session is absent")),
                1,
                UserProfilePatch {
                    display_name: Some("Lag Refresh".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("update profile before lag signal: {error}"));
        let (lag_tx, lag_rx) = broadcast::channel(1);
        lag_tx
            .send([0x11; 32])
            .unwrap_or_else(|error| panic!("send first lag signal: {error}"));
        lag_tx
            .send([0x22; 32])
            .unwrap_or_else(|error| panic!("send second lag signal: {error}"));
        let mut lag_rx = Some(lag_rx);
        let lagged = receive_revocation(&mut lag_rx).await;
        assert!(matches!(
            lagged,
            Err(broadcast::error::RecvError::Lagged(1))
        ));
        assert!(
            session_remains_authorized_after_revocation_signal(
                &state,
                &mut principal,
                &mut human_session,
                lagged,
            )
            .await
        );
        assert_eq!(principal.display_name, "Lag Refresh");

        fixture
            .store()
            .update_human_session_profile(
                human_session
                    .as_ref()
                    .unwrap_or_else(|| panic!("refreshed human session is absent")),
                2,
                UserProfilePatch {
                    display_name: Some("Closed Refresh".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("update profile before closed signal: {error}"));
        let (closed_tx, closed_rx) = broadcast::channel::<[u8; 32]>(1);
        drop(closed_tx);
        let mut closed_rx = Some(closed_rx);
        let closed = receive_revocation(&mut closed_rx).await;
        assert!(matches!(closed, Err(broadcast::error::RecvError::Closed)));
        assert!(
            !session_remains_authorized_after_revocation_signal(
                &state,
                &mut principal,
                &mut human_session,
                closed,
            )
            .await
        );
        assert_eq!(principal.display_name, "Closed Refresh");
        state
            .rooms
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown socket revocation state: {error}"));
    }
}
