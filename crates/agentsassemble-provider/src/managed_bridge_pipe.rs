//! Both pipe directions continue under backpressure; cancelled reads retain queued writes.
use std::collections::VecDeque;

use bytes::Bytes;
use futures_util::SinkExt;
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};

use super::{
    exchange::MAX_EXCHANGES,
    wire::{self, Reader, Writer, protocol_error},
};
use crate::driver::DriverError;

pub(super) struct Pipe<'a, R, W> {
    input: &'a mut Reader<R>,
    output: &'a mut Writer<W>,
    queued: VecDeque<Bytes>,
    dirty: bool,
}

impl<'a, R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Pipe<'a, R, W> {
    pub(super) fn new(input: &'a mut Reader<R>, output: &'a mut Writer<W>) -> Self {
        Self {
            input,
            output,
            queued: VecDeque::new(),
            dirty: false,
        }
    }

    pub(super) fn queue<T: Serialize>(&mut self, value: &T) -> Result<(), DriverError> {
        // Each bounded callback can have its open and acknowledgement, plus control replies.
        if self.queued.len() >= MAX_EXCHANGES * 2 + 4 {
            return Err(protocol_error());
        }
        self.queued.push_back(wire::encode(value)?);
        Ok(())
    }

    pub(super) async fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>, DriverError> {
        loop {
            tokio::select! {
                result = wire::read(self.input) => return result,
                result = advance(self.output, &mut self.queued, &mut self.dirty),
                    if !self.queued.is_empty() || self.dirty => result?,
            }
        }
    }

    pub(super) async fn flush(&mut self) -> Result<(), DriverError> {
        while !self.queued.is_empty() || self.dirty {
            advance(self.output, &mut self.queued, &mut self.dirty).await?;
        }
        Ok(())
    }
}

async fn advance<W: AsyncWrite + Unpin>(
    output: &mut Writer<W>,
    queued: &mut VecDeque<Bytes>,
    dirty: &mut bool,
) -> Result<(), DriverError> {
    if let Some(bytes) = queued.front().cloned() {
        // Feed returns Ready immediately after start_send; a cancelled Pending feed
        // has not transferred this frame. Flush progress remains in FramedWrite.
        output.feed(bytes).await.map_err(|_| protocol_error())?;
        queued.pop_front();
        *dirty = true;
    } else {
        output.flush().await.map_err(|_| protocol_error())?;
        *dirty = false;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backpressured_write_does_not_hide_control_or_replay_after_cancelled_read()
    -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::timeout(std::time::Duration::from_secs(5), backpressure_case()).await?
    }

    async fn backpressure_case() -> Result<(), Box<dyn std::error::Error>> {
        let (left, right) = tokio::io::duplex(64);
        let (left_read, left_write) = tokio::io::split(left);
        let (right_read, right_write) = tokio::io::split(right);
        let mut input = wire::reader(left_read);
        let mut output = wire::writer(left_write);
        let mut pipe = Pipe::new(&mut input, &mut output);
        let payload = "x".repeat(4096);
        pipe.queue(&payload)?;
        pipe.queue(&"second")?;
        tokio::select! {
            biased;
            result = pipe.read::<u64>() => return Err(format!("read completed before peer control: {result:?}").into()),
            () = std::future::ready(()) => {}
        }
        let peer = tokio::spawn(async move {
            let mut input = wire::reader(right_read);
            let mut output = wire::writer(right_write);
            wire::write(&mut output, &7_u64).await?;
            let first: String = wire::read(&mut input).await?.ok_or_else(protocol_error)?;
            let second: String = wire::read(&mut input).await?.ok_or_else(protocol_error)?;
            Ok::<_, DriverError>((first, second))
        });
        assert_eq!(pipe.read::<u64>().await?, Some(7));
        pipe.flush().await?;
        assert_eq!(peer.await??, (payload, "second".to_owned()));
        Ok(())
    }
}
