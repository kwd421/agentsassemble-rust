use agentsassemble_domain::{AuthenticatedPrincipal, SideChatUpdate};
use agentsassemble_persistence::{RoomMutationAuthority, RoomSessionAuthorization};
use agentsassemble_protocol::ServerFrame;
use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::SplitSink;
use tokio::sync::broadcast;

use crate::{AppState, room_channel::send_frame, room_socket::refresh_room_session};

pub(crate) async fn receive_update(
    receiver: &mut Option<broadcast::Receiver<SideChatUpdate>>,
) -> Result<SideChatUpdate, broadcast::error::RecvError> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

pub(crate) async fn deliver_update(
    state: &AppState,
    principal: &mut AuthenticatedPrincipal,
    session: &mut Option<RoomSessionAuthorization>,
    incarnation: uuid::Uuid,
    sender: &mut SplitSink<WebSocket, Message>,
    update: Result<SideChatUpdate, broadcast::error::RecvError>,
) -> Option<()> {
    refresh_room_session(state, principal, session).await?;
    let authority = session.as_ref().map_or(
        RoomMutationAuthority::TrustedPrincipal(principal),
        RoomSessionAuthorization::mutation_authority,
    );
    if let Err(error) = state
        .store
        .authorize_side_chat_delivery(authority, incarnation)
        .await
    {
        if crate::room_socket::persistence_error_is_internal(&error) {
            tracing::error!(error = ?error, room_id = %principal.room_id, "side-chat delivery authorization failed");
        }
        return None;
    }
    match update {
        Ok(update) => send_frame(
            sender,
            &state.shutdown,
            &ServerFrame::SideChatUpdated { update },
        )
        .await
        .ok(),
        Err(broadcast::error::RecvError::Lagged(_)) => {
            let _ = send_frame(
                sender,
                &state.shutdown,
                &ServerFrame::SideChatResyncRequired {
                    reason: "Side-chat delivery fell behind; reconnect and reload its snapshot."
                        .to_owned(),
                },
            )
            .await;
            None
        }
        Err(broadcast::error::RecvError::Closed) => None,
    }
}
