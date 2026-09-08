use futures_util::StreamExt;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{FramedRead, LinesCodec};

use super::{MAX_PROTOCOL_LINE_BYTES, protocol_closed, protocol_error};
use crate::driver::DriverError;

/// Native Codex framing is shared by room sessions and sessionless account inspection.
pub(super) struct CodexWire<I, O> {
    input: I,
    output: FramedRead<O, LinesCodec>,
}

impl<I: AsyncWrite + Unpin, O: AsyncRead + Unpin> CodexWire<I, O> {
    pub(super) fn new(input: I, output: O) -> Self {
        Self {
            input,
            output: FramedRead::new(
                output,
                LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
            ),
        }
    }

    pub(super) async fn write_message(&mut self, message: &Value) -> Result<(), DriverError> {
        let mut encoded = serde_json::to_vec(message).map_err(|_| protocol_error())?;
        if encoded.len() > MAX_PROTOCOL_LINE_BYTES {
            return Err(DriverError::new(
                "provider_protocol_overflow",
                "The Codex app-server request exceeded its protocol bound.",
            ));
        }
        encoded.push(b'\n');
        self.input
            .write_all(&encoded)
            .await
            .map_err(|_| protocol_closed())?;
        self.input.flush().await.map_err(|_| protocol_closed())
    }

    pub(super) async fn read_message(&mut self) -> Result<(Value, usize), DriverError> {
        let line = self
            .output
            .next()
            .await
            .ok_or_else(protocol_closed)?
            .map_err(|_| protocol_error())?;
        let message = serde_json::from_str::<Value>(&line).map_err(|_| protocol_error())?;
        Ok((message, line.len()))
    }
}
