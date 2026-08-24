use std::{io, time::Duration};

use agentsassemble_protocol::{
    AuthenticatedFrame, ClientFrame, CommandNack, CommandResolution, ProtocolError, ServerFrame,
};
use axum::extract::ws::Message;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{Sink, SinkExt};
use tokio_util::sync::CancellationToken;

use crate::server_proof::{FrameDirection, sign_frame, verify_frame_proof};

pub(crate) const MAX_WS_INNER_MESSAGE_BYTES: usize = 256 * 1024;
pub(crate) const MAX_WS_WIRE_MESSAGE_BYTES: usize = 384 * 1024;
const MAX_BASE64_PAYLOAD_BYTES: usize = MAX_WS_INNER_MESSAGE_BYTES.div_ceil(3) * 4;
const MAX_SAFE_FRAME_COUNTER: u64 = 9_007_199_254_740_991;
const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct AuthenticatedChannel {
    proof_key: String,
    connection_nonce: String,
    next_client_counter: u64,
    next_server_counter: u64,
}

impl AuthenticatedChannel {
    pub(crate) fn new(proof_key: String, connection_nonce: String) -> Self {
        Self {
            proof_key,
            connection_nonce,
            next_client_counter: 1,
            next_server_counter: 1,
        }
    }

    pub(crate) async fn send<S>(
        &mut self,
        sender: &mut S,
        cancellation: &CancellationToken,
        frame: &ServerFrame,
    ) -> Result<(), axum::Error>
    where
        S: Sink<Message, Error = axum::Error> + Unpin,
    {
        let payload = encode_server_frame(frame).map_err(axum::Error::new)?;
        let counter = self.next_server_counter;
        let proof = sign_frame(
            &self.proof_key,
            &self.connection_nonce,
            FrameDirection::Server,
            counter,
            payload.as_bytes(),
        );
        let envelope = AuthenticatedFrame::Authenticated {
            counter,
            payload: STANDARD.encode(payload.as_bytes()),
            proof,
        };
        let encoded = serde_json::to_string(&envelope).map_err(axum::Error::new)?;
        if encoded.len() > MAX_WS_WIRE_MESSAGE_BYTES {
            return Err(axum::Error::new(limit_error(
                "authenticated WebSocket frame exceeds the wire limit",
            )));
        }
        send_plain_encoded(sender, cancellation, encoded).await?;
        self.next_server_counter = next_counter(counter).map_err(axum::Error::new)?;
        Ok(())
    }

    pub(crate) async fn send_nack<S>(
        &mut self,
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
        self.send(
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

    pub(crate) fn decode_client(
        &mut self,
        encoded: &str,
    ) -> Result<(ClientFrame, usize), io::Error> {
        if encoded.len() > MAX_WS_WIRE_MESSAGE_BYTES {
            return Err(limit_error(
                "authenticated WebSocket frame exceeds the wire limit",
            ));
        }
        let AuthenticatedFrame::Authenticated {
            counter,
            payload,
            proof,
        } = serde_json::from_str(encoded).map_err(invalid_frame)?;
        if counter != self.next_client_counter || counter > MAX_SAFE_FRAME_COUNTER {
            return Err(invalid_data(
                "authenticated frame counter is not contiguous",
            ));
        }
        if payload.len() > MAX_BASE64_PAYLOAD_BYTES {
            return Err(limit_error(
                "authenticated WebSocket payload exceeds the product limit",
            ));
        }
        let decoded = STANDARD.decode(&payload).map_err(invalid_frame)?;
        if decoded.len() > MAX_WS_INNER_MESSAGE_BYTES || STANDARD.encode(&decoded) != payload {
            return Err(invalid_data("authenticated frame payload is not canonical"));
        }
        if !verify_frame_proof(
            &self.proof_key,
            &self.connection_nonce,
            FrameDirection::Client,
            counter,
            &decoded,
            &proof,
        ) {
            return Err(invalid_data("authenticated frame proof is invalid"));
        }
        let frame = serde_json::from_slice(&decoded).map_err(invalid_frame)?;
        self.next_client_counter = next_counter(counter)?;
        Ok((frame, decoded.len()))
    }
}

pub(crate) fn encode_server_frame(frame: &ServerFrame) -> Result<String, io::Error> {
    let encoded = serde_json::to_string(frame).map_err(io::Error::other)?;
    if encoded.len() > MAX_WS_INNER_MESSAGE_BYTES {
        return Err(limit_error(
            "WebSocket product frame exceeds the product limit",
        ));
    }
    Ok(encoded)
}

pub(crate) async fn send_plain_encoded<S>(
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

pub(crate) async fn send_plain_frame<S>(
    sender: &mut S,
    cancellation: &CancellationToken,
    frame: &ServerFrame,
) -> Result<(), axum::Error>
where
    S: Sink<Message, Error = axum::Error> + Unpin,
{
    let encoded = encode_server_frame(frame).map_err(axum::Error::new)?;
    send_plain_encoded(sender, cancellation, encoded).await
}

fn next_counter(counter: u64) -> Result<u64, io::Error> {
    let next = counter
        .checked_add(1)
        .ok_or_else(|| invalid_data("authenticated frame counter overflowed"))?;
    if next > MAX_SAFE_FRAME_COUNTER {
        return Err(invalid_data(
            "authenticated frame counter exceeded the product limit",
        ));
    }
    Ok(next)
}

fn invalid_frame(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn limit_error(message: &'static str) -> io::Error {
    invalid_data(message)
}
