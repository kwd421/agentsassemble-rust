//! A socket owns only network custody; the caller retains runtime and pending requests.
use std::time::Duration;

use agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use uuid::Uuid;

use crate::{AttendeeClientError, AttendeeSocketFrame, AttendeeSocketRequest, RoomAttendeeClient};

pub struct AttendeeSocket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    connection_id: Uuid,
}

impl RoomAttendeeClient {
    /// Claims a fresh network connection without restarting the caller's provider runtime.
    ///
    /// # Errors
    /// Returns redacted admission, handshake, timeout and protocol failures.
    pub async fn connect(&self) -> Result<AttendeeSocket, AttendeeClientError> {
        let mut url = self.endpoint("ws")?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|()| AttendeeClientError::local("invalid_room_server_url"))?;
        let mut request = url.as_str().into_client_request().map_err(|_| invalid())?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", self.bearer()?)
                .parse()
                .map_err(|_| invalid())?,
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_ROOM_SOCKET_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_ROOM_SOCKET_MESSAGE_BYTES));
        // One deadline covers TCP/TLS, upgrade and the server's connection claim.
        tokio::time::timeout(Duration::from_secs(10), async {
            let (stream, _) = connect_async_with_config(request, Some(config), false)
                .await
                .map_err(handshake_error)?;
            let mut socket = AttendeeSocket {
                stream,
                connection_id: Uuid::nil(),
            };
            let AttendeeSocketFrame::Connected { connection_id } = socket.receive().await? else {
                return Err(invalid());
            };
            if connection_id.is_nil() {
                return Err(invalid());
            }
            socket.connection_id = connection_id;
            Ok(socket)
        })
        .await
        .map_err(|_| AttendeeClientError::local("attendee_connect_timeout"))?
    }
}

impl AttendeeSocket {
    #[must_use]
    pub fn connection_id(&self) -> Uuid {
        self.connection_id
    }

    /// Sends one caller-owned retry identity; receiving its acknowledgement is separate.
    ///
    /// # Errors
    /// Returns encoding, size or unresolved transport failures. Sending is not commitment.
    pub async fn send(
        &mut self,
        request: &AttendeeSocketRequest,
    ) -> Result<(), AttendeeClientError> {
        let encoded = serde_json::to_string(request).map_err(|_| invalid())?;
        if encoded.len() > MAX_ROOM_SOCKET_MESSAGE_BYTES {
            return Err(AttendeeClientError::local("attendee_request_too_large"));
        }
        self.write(Message::Text(encoded.into())).await
    }

    /// Receives one application frame, preserving interleaved turn and interrupt messages.
    ///
    /// # Errors
    /// Returns redacted closure, malformed protocol and transport failures.
    pub async fn receive(&mut self) -> Result<AttendeeSocketFrame, AttendeeClientError> {
        loop {
            match self
                .stream
                .next()
                .await
                .ok_or_else(closed)?
                .map_err(|_| transport())?
            {
                Message::Text(encoded) => {
                    return serde_json::from_str(&encoded).map_err(|_| invalid());
                }
                Message::Ping(_) => {
                    // Tungstenite queued the matching pong; flush it without another application send.
                    tokio::time::timeout(Duration::from_secs(10), self.stream.flush())
                        .await
                        .map_err(|_| transport())?
                        .map_err(|_| transport())?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Err(closed()),
                _ => return Err(invalid()),
            }
        }
    }

    /// Keeps only the network connection alive; it does not report provider readiness.
    ///
    /// # Errors
    /// Returns an unresolved transport failure.
    pub async fn ping(&mut self) -> Result<(), AttendeeClientError> {
        self.write(Message::Ping(Vec::new().into())).await
    }

    /// Closes network custody without asserting provider stop or room departure.
    ///
    /// # Errors
    /// Returns an unresolved transport failure.
    pub async fn close(&mut self) -> Result<(), AttendeeClientError> {
        self.write(Message::Close(None)).await
    }

    async fn write(&mut self, message: Message) -> Result<(), AttendeeClientError> {
        tokio::time::timeout(Duration::from_secs(10), self.stream.send(message))
            .await
            .map_err(|_| transport())?
            .map_err(|_| transport())
    }
}

fn invalid() -> AttendeeClientError {
    AttendeeClientError::local("invalid_attendee_socket_frame")
}
fn transport() -> AttendeeClientError {
    AttendeeClientError::local("attendee_socket_unresolved")
}
fn closed() -> AttendeeClientError {
    AttendeeClientError::local("attendee_socket_closed")
}

fn handshake_error(failure: tokio_tungstenite::tungstenite::Error) -> AttendeeClientError {
    if let tokio_tungstenite::tungstenite::Error::Http(response) = failure
        && response.status().is_client_error()
        && !matches!(response.status().as_u16(), 408 | 429)
    {
        return AttendeeClientError {
            code: "attendee_connection_rejected".to_owned(),
            resolution: Some(agentsassemble_protocol::CommandResolution::Rejected),
        };
    }
    transport()
}
