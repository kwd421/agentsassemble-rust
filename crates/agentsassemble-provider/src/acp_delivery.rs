//! ACP responder queue acceptance is not native stdin delivery.
use futures_util::{Sink, SinkExt};
use serde_json::Value;
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;
use tokio_util::codec::{FramedWrite, LinesCodec};

#[derive(Clone, Default)]
pub(super) struct Deliveries(Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>);

pub(super) struct Delivery {
    owner: Deliveries,
    id: String,
    response: oneshot::Receiver<bool>,
}

struct OutputDeliveries(Deliveries);
impl Drop for OutputDeliveries {
    fn drop(&mut self) {
        self.0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

impl Deliveries {
    pub(super) fn register(&self, id: String) -> io::Result<Delivery> {
        let mut pending = self
            .0
            .lock()
            .map_err(|_| io::Error::other("ACP delivery owner failed"))?;
        if pending.len() >= 128 || pending.contains_key(&id) {
            return Err(io::Error::other("ACP delivery request is unavailable"));
        }
        let (sender, response) = oneshot::channel();
        pending.insert(id.clone(), sender);
        Ok(Delivery {
            owner: self.clone(),
            id,
            response,
        })
    }

    fn acknowledge(&self, value: &Value, delivered: bool) -> io::Result<()> {
        if let Some(batch) = value.as_array() {
            for response in batch {
                self.acknowledge(response, delivered)?;
            }
        } else if value.get("method").is_none()
            && (value.get("result").is_some() || value.get("error").is_some())
            && let Some(id) = value.get("id")
        {
            let mut pending = self
                .0
                .lock()
                .map_err(|_| io::Error::other("ACP delivery owner failed"))?;
            if let Some(reply) = pending.remove(&id.to_string()) {
                let _ = reply.send(delivered);
            }
        }
        Ok(())
    }
}

impl Delivery {
    pub(super) async fn receive(&mut self) -> bool {
        (&mut self.response).await.unwrap_or(false)
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.owner.0.lock() {
            pending.remove(&self.id);
        }
    }
}

pub(super) fn outgoing_lines<W>(
    writer: W,
    deliveries: Deliveries,
) -> impl Sink<String, Error = io::Error> + Unpin + Send + 'static
where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    Box::pin(futures_util::sink::unfold(
        (
            FramedWrite::new(writer, LinesCodec::new()),
            OutputDeliveries(deliveries),
        ),
        |(mut writer, deliveries), line: String| async move {
            let observe = !deliveries
                .0
                .0
                .lock()
                .map_err(|_| io::Error::other("ACP delivery owner failed"))?
                .is_empty();
            let response = if observe {
                Some(serde_json::from_str::<Value>(&line).map_err(io::Error::other)?)
            } else {
                None
            };
            let result = writer.send(line).await.map_err(io::Error::other);
            if let Some(response) = response {
                deliveries.0.acknowledge(&response, result.is_ok())?;
            }
            result?;
            Ok((writer, deliveries))
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broken_native_stdin_never_acknowledges_a_queued_response() {
        let (writer, peer) = tokio::io::duplex(64);
        let deliveries = Deliveries::default();
        let mut receipt = deliveries
            .register("\"native-request\"".to_owned())
            .unwrap_or_else(|error| panic!("register native response: {error}"));
        let mut output = outgoing_lines(writer, deliveries);
        drop(peer);
        assert!(
            output
                .send(r#"{"jsonrpc":"2.0","id":"native-request","result":{}}"#.to_owned())
                .await
                .is_err()
        );
        assert!(!receipt.receive().await);
    }
}
