//! Bounded native request ingress; the room owner supplies the live response exchange.
use agentsassemble_domain::ProviderRequest;
use tokio::sync::{mpsc, oneshot};

use crate::{ProviderRequestExchange, ProviderRequestExchangeError};

#[derive(Clone)]
pub struct ProviderRequestIngress {
    sender: mpsc::Sender<ProviderRequestCommand>,
}

impl std::fmt::Debug for ProviderRequestIngress {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRequestIngress")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ProviderRequestIngress {
    fn eq(&self, other: &Self) -> bool {
        self.sender.same_channel(&other.sender)
    }
}
impl Eq for ProviderRequestIngress {}

pub struct ProviderRequestCommand {
    pub session_id: String,
    pub turn_generation: u64,
    pub execution_id: String,
    pub request: ProviderRequest,
    reply: oneshot::Sender<Result<ProviderRequestExchange, ProviderRequestExchangeError>>,
}

impl ProviderRequestIngress {
    #[must_use]
    pub fn channel(capacity: usize) -> (Self, mpsc::Receiver<ProviderRequestCommand>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender }, receiver)
    }

    /// Opens a request using the native handler's exact active execution.
    ///
    /// # Errors
    /// Returns explicit queue, owner or request rejection; never grants permission on failure.
    pub async fn open(
        &self,
        session_id: &str,
        turn_generation: u64,
        execution_id: &str,
        request: ProviderRequest,
    ) -> Result<ProviderRequestExchange, ProviderRequestExchangeError> {
        // Native protocol input crosses a bounded in-process queue before storage authority.
        if !request.is_valid() {
            return Err(ProviderRequestExchangeError::Rejected);
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(ProviderRequestCommand {
                session_id: session_id.to_owned(),
                turn_generation,
                execution_id: execution_id.to_owned(),
                request,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ProviderRequestExchangeError::Busy,
                mpsc::error::TrySendError::Closed(_) => ProviderRequestExchangeError::Closed,
            })?;
        response
            .await
            .map_err(|_| ProviderRequestExchangeError::Closed)?
    }
}

impl ProviderRequestCommand {
    pub fn complete(self, result: Result<ProviderRequestExchange, ProviderRequestExchangeError>) {
        // A lost native caller drops the returned exchange and wakes its broker cancellation.
        let _ = self.reply.send(result);
    }
}
