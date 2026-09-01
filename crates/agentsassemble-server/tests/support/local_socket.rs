#![allow(dead_code)] // Integration binaries exercise different local-socket helpers.

use agentsassemble_protocol::TicketResponse;
use agentsassemble_server::{AppState, issue_local_ticket};
use tokio_tungstenite::connect_async;

use super::subscription_proof::AuthenticatedTestSocket;

pub async fn connect(
    base_url: &str,
    state: &AppState,
    room_id: &str,
) -> AuthenticatedTestSocket<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let grant = request_ticket(state, room_id).await;
    let ticket = grant.ticket;
    let url = format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    );
    let socket = connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect WebSocket: {error}"))
        .0;
    AuthenticatedTestSocket::new(socket)
}

pub async fn request_ticket(state: &AppState, room_id: &str) -> TicketResponse {
    issue_local_ticket(state, room_id)
        .await
        .unwrap_or_else(|error| panic!("issue private-control-equivalent ticket: {error}"))
}
