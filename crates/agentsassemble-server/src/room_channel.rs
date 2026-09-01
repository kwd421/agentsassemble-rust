use std::{io, time::Duration};

use agentsassemble_protocol::{
    ClientFrame, CommandNack, CommandResolution, MAX_ROOM_SOCKET_MESSAGE_BYTES, ProtocolError,
    ServerFrame,
};
use axum::extract::ws::Message;
use futures_util::{Sink, SinkExt};
use tokio_util::sync::CancellationToken;

const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn decode_client_frame(encoded: &str) -> Result<(ClientFrame, usize), io::Error> {
    if encoded.len() > MAX_ROOM_SOCKET_MESSAGE_BYTES {
        return Err(invalid_data("WebSocket frame exceeds the product limit"));
    }
    let frame = serde_json::from_str(encoded).map_err(invalid_frame)?;
    Ok((frame, encoded.len()))
}

pub(crate) fn encode_server_frame(frame: &ServerFrame) -> Result<String, io::Error> {
    let encoded = serde_json::to_string(frame).map_err(io::Error::other)?;
    if encoded.len() > MAX_ROOM_SOCKET_MESSAGE_BYTES {
        return Err(invalid_data("WebSocket frame exceeds the product limit"));
    }
    Ok(encoded)
}

pub(crate) async fn send_encoded<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    encoded: String,
) -> Result<(), axum::Error>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(axum::Error::new(io::Error::new(
            io::ErrorKind::Interrupted,
            "runtime shutdown interrupted WebSocket send",
        ))),
        result = tokio::time::timeout(
            WS_WRITE_TIMEOUT,
            sender.send(Message::Text(encoded.into())),
        ) => result.map_err(axum::Error::new)?,
    }
}

pub(crate) async fn send_frame<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    frame: &ServerFrame,
) -> Result<(), axum::Error>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    let encoded = encode_server_frame(frame).map_err(axum::Error::new)?;
    send_encoded(sender, cancellation, encoded).await
}

pub(crate) async fn send_nack<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    request_id: &str,
    action: &str,
    resolution: CommandResolution,
    error: ProtocolError,
) -> Result<(), axum::Error>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    send_frame(
        sender,
        cancellation,
        &ServerFrame::Nack(CommandNack {
            request_id: request_id.to_owned(),
            accepted: false,
            resolution,
            action: action.to_owned(),
            error,
        }),
    )
    .await
}

fn invalid_frame(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
