//! Bounded live-only exchanges shared by the native and room-owner pipe endpoints.
use std::collections::HashMap;

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::sync::mpsc;

use super::wire::protocol_error;
use crate::driver::{DriverError, DriverFuture};

// Accommodates the broker's 64 requests, 32 portal tools and all attachment reservations.
pub(super) const MAX_EXCHANGES: usize = 128;

pub(super) struct Exchanges<Out, In> {
    events: mpsc::Sender<(u64, Out)>,
    event_rx: mpsc::Receiver<(u64, Out)>,
    replies: HashMap<u64, mpsc::Sender<In>>,
    pending: FuturesUnordered<DriverFuture<'static, Result<u64, DriverError>>>,
    next_id: u64,
}

impl<Out: Send + 'static, In: Send + 'static> Exchanges<Out, In> {
    pub(super) fn new() -> Self {
        let (events, event_rx) = mpsc::channel(4);
        Self {
            events,
            event_rx,
            replies: HashMap::new(),
            pending: FuturesUnordered::new(),
            next_id: 1,
        }
    }

    pub(super) fn can_start(&self) -> bool {
        self.pending.len() < MAX_EXCHANGES
    }

    pub(super) fn start<F, Fut>(&mut self, run: F) -> Result<u64, DriverError>
    where
        F: FnOnce(Exchange<Out, In>) -> Fut,
        Fut: std::future::Future<Output = Result<u64, DriverError>> + Send + 'static,
    {
        if !self.can_start() {
            return Err(protocol_error());
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or_else(protocol_error)?;
        let (sender, replies) = mpsc::channel(2);
        self.replies.insert(id, sender);
        self.pending.push(Box::pin(run(Exchange {
            id,
            events: self.events.clone(),
            replies,
        })));
        Ok(id)
    }

    pub(super) fn reply(&mut self, id: u64, reply: In) -> Result<(), DriverError> {
        self.replies
            .get(&id)
            .ok_or_else(protocol_error)?
            .try_send(reply)
            .map_err(|_| protocol_error())
    }

    pub(super) async fn next(&mut self) -> Result<(u64, Out), DriverError> {
        loop {
            tokio::select! {
                event = self.event_rx.recv() => return event.ok_or_else(protocol_error),
                result = self.pending.next(), if !self.pending.is_empty() => {
                    let id = result.ok_or_else(protocol_error)??;
                    self.replies.remove(&id);
                }
            }
        }
    }
}

pub(super) struct Exchange<Out, In> {
    pub(super) id: u64,
    events: mpsc::Sender<(u64, Out)>,
    replies: mpsc::Receiver<In>,
}

impl<Out, In> Exchange<Out, In> {
    pub(super) async fn send(&self, event: Out) -> Result<(), DriverError> {
        self.events
            .send((self.id, event))
            .await
            .map_err(|_| protocol_error())
    }
    pub(super) async fn receive(&mut self) -> Result<In, DriverError> {
        self.replies.recv().await.ok_or_else(protocol_error)
    }
}
