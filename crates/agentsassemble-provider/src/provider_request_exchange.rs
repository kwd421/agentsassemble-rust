//! Live-only request answers and explicit upstream delivery acknowledgement.
use agentsassemble_domain::ProviderRequestResolution;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderRequestExchangeError {
    #[error("the provider request owner is unavailable or the request has closed")]
    Closed,
    #[error("the provider response delivery could not be committed")]
    DeliveryFailed,
}

type Receipt = Result<(), ProviderRequestExchangeError>;

/// Held by the native request handler until its upstream reply and durable receipt finish.
/// Dropping it wakes the broker, including while the human answer is still pending.
pub struct ProviderRequestExchange {
    response: Option<oneshot::Receiver<ProviderRequestResolution>>,
    completion: Option<oneshot::Sender<bool>>,
    receipt: Option<oneshot::Receiver<Receipt>>,
    cancellation: CancellationToken,
}

/// Held by the room's request owner. Neither side implements Debug or serialization.
pub struct ProviderRequestResponder {
    response: Option<oneshot::Sender<ProviderRequestResolution>>,
    cancellation: CancellationToken,
}

/// Delivery observation is separate so the broker can wait while retaining its answer slot.
pub struct ProviderRequestCompletion {
    completion: oneshot::Receiver<bool>,
    receipt: Option<oneshot::Sender<Receipt>>,
    cancellation: CancellationToken,
}

impl ProviderRequestExchange {
    #[must_use]
    pub fn channel() -> (Self, ProviderRequestResponder, ProviderRequestCompletion) {
        let (response_tx, response) = oneshot::channel();
        let (completion, completion_rx) = oneshot::channel();
        let (receipt_tx, receipt) = oneshot::channel();
        let cancellation = CancellationToken::new();
        (
            Self {
                response: Some(response),
                completion: Some(completion),
                receipt: Some(receipt),
                cancellation: cancellation.clone(),
            },
            ProviderRequestResponder {
                response: Some(response_tx),
                cancellation: cancellation.clone(),
            },
            ProviderRequestCompletion {
                completion: completion_rx,
                receipt: Some(receipt_tx),
                cancellation,
            },
        )
    }

    /// Waits for the one live answer. This wait is cancellation-safe.
    ///
    /// # Errors
    /// Returns Closed when the broker cancels or drops the request.
    pub async fn receive(
        &mut self,
    ) -> Result<ProviderRequestResolution, ProviderRequestExchangeError> {
        let response = self
            .response
            .as_mut()
            .ok_or(ProviderRequestExchangeError::Closed)?;
        let result = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(ProviderRequestExchangeError::Closed),
            result = response => result.map_err(|_| ProviderRequestExchangeError::Closed),
        };
        self.response.take();
        result
    }

    /// Reports actual upstream delivery and waits for its durable completion.
    /// Retain this exchange across cancellation of the wait; the report is sent only once.
    ///
    /// # Errors
    /// Rejects lost broker custody and failed durable completion.
    pub async fn complete(&mut self, delivered: bool) -> Receipt {
        if let Some(completion) = self.completion.take() {
            completion
                .send(delivered)
                .map_err(|_| ProviderRequestExchangeError::Closed)?;
        }
        let result = self
            .receipt
            .as_mut()
            .ok_or(ProviderRequestExchangeError::Closed)?
            .await;
        self.receipt.take();
        result.map_err(|_| ProviderRequestExchangeError::DeliveryFailed)?
    }
}

impl Drop for ProviderRequestExchange {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl ProviderRequestResponder {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Consumes the answer slot once. Failure never permits a second recipient.
    ///
    /// # Errors
    /// Returns Closed if the recipient ended or the answer was already delivered.
    pub fn respond(
        &mut self,
        resolution: ProviderRequestResolution,
    ) -> Result<(), ProviderRequestExchangeError> {
        if self.cancellation.is_cancelled() {
            return Err(ProviderRequestExchangeError::Closed);
        }
        self.response
            .take()
            .ok_or(ProviderRequestExchangeError::Closed)?
            .send(resolution)
            .map_err(|_| ProviderRequestExchangeError::Closed)
    }
}

impl ProviderRequestCompletion {
    /// Returns the native delivery acknowledgement or false when its owner disappears.
    /// This wait is cancellation-safe and does not spawn a task or poll.
    pub async fn completion(&mut self) -> bool {
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => false,
            result = &mut self.completion => result.unwrap_or(false),
        }
    }

    pub fn finish(mut self, result: Receipt) {
        if let Some(receipt) = self.receipt.take() {
            let _ = receipt.send(result);
        }
    }
}

impl Drop for ProviderRequestCompletion {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delivery_requires_receipt_and_dropped_native_owner_wakes_broker() {
        let (mut native, mut broker, mut observer) = ProviderRequestExchange::channel();
        broker
            .respond(ProviderRequestResolution::Acknowledge)
            .unwrap();
        assert!(
            broker
                .respond(ProviderRequestResolution::Acknowledge)
                .is_err()
        );
        assert!(matches!(
            native.receive().await.unwrap(),
            ProviderRequestResolution::Acknowledge
        ));
        let completion = native.complete(true);
        tokio::pin!(completion);
        tokio::select! {
            biased;
            result = &mut completion => panic!("completed without durable receipt: {result:?}"),
            () = std::future::ready(()) => {}
        }
        assert!(observer.completion().await);
        observer.finish(Ok(()));
        completion.await.unwrap();

        let (native, mut broker, mut observer) = ProviderRequestExchange::channel();
        drop(native);
        assert!(!observer.completion().await);
        assert!(
            broker
                .respond(ProviderRequestResolution::Acknowledge)
                .is_err()
        );
    }
}
