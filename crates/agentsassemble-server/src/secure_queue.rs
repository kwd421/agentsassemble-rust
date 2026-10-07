//! Weighted ciphertext queue budgets shared by every channel in one runtime.
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

pub(crate) const CHUNK_BYTES: usize = 64 * 1024;
const DIRECTION_BYTES: usize = 512 * 1024;
pub(crate) const GLOBAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Budget {
    direction: Arc<Semaphore>,
    global: Arc<Semaphore>,
}

pub(crate) struct Charge {
    _direction: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

impl Budget {
    pub(crate) fn new(global: Arc<Semaphore>) -> Self {
        Self {
            direction: Arc::new(Semaphore::new(DIRECTION_BYTES)),
            global,
        }
    }
    pub(crate) fn charge(&self, bytes: usize) -> Result<Charge, ()> {
        let bytes = u32::try_from(bytes.max(64)).map_err(|_| ())?;
        let direction = self
            .direction
            .clone()
            .try_acquire_many_owned(bytes)
            .map_err(|_| ())?;
        let global = self
            .global
            .clone()
            .try_acquire_many_owned(bytes)
            .map_err(|_| ())?;
        Ok(Charge {
            _direction: direction,
            _global: global,
        })
    }
    async fn wait(&self, bytes: usize, cancel: &CancellationToken) -> Result<Charge, ()> {
        let bytes = u32::try_from(bytes.max(64)).map_err(|_| ())?;
        if bytes as usize > DIRECTION_BYTES {
            return Err(());
        }
        tokio::select! {
            () = cancel.cancelled() => Err(()),
            result = async {
                let direction = self.direction.clone().acquire_many_owned(bytes).await.map_err(|_| ())?;
                let global = self.global.clone().acquire_many_owned(bytes).await.map_err(|_| ())?;
                Ok(Charge { _direction: direction, _global: global })
            } => result,
        }
    }
}

pub(crate) struct Queued {
    pub(crate) bytes: Vec<u8>,
    pub(crate) _charge: Charge,
}

#[derive(Clone)]
pub(crate) struct Output {
    tx: mpsc::Sender<Queued>,
    budget: Budget,
    cancel: CancellationToken,
}

impl Output {
    pub(crate) fn new(
        global: Arc<Semaphore>,
        cancel: CancellationToken,
    ) -> (Self, mpsc::Receiver<Queued>) {
        let (tx, rx) = mpsc::channel(128);
        (
            Self {
                tx,
                budget: Budget::new(global),
                cancel,
            },
            rx,
        )
    }
    pub(crate) fn with_cancel(&self, cancel: CancellationToken) -> Self {
        Self {
            cancel,
            ..self.clone()
        }
    }
    pub(crate) async fn send(&self, frame: serde_json::Value) -> Result<(), ()> {
        let bytes = serde_json::to_vec(&frame).map_err(|_| ())?;
        if bytes.len() > 96 * 1024 {
            return Err(());
        }
        let charge = self.budget.wait(bytes.len() + 24, &self.cancel).await?;
        tokio::select! {
            () = self.cancel.cancelled() => Err(()),
            result = self.tx.send(Queued { bytes, _charge: charge }) => result.map_err(|_| ()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn aggregate_and_direction_capacity_release_and_cancel() {
        let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
        let budget = Budget::new(global.clone());
        let charge = budget
            .charge(DIRECTION_BYTES)
            .unwrap_or_else(|_| panic!("capacity"));
        assert!(budget.charge(1).is_err());
        drop(charge);
        assert!(budget.charge(DIRECTION_BYTES).is_ok());
        let budgets = (0..16)
            .map(|_| Budget::new(global.clone()))
            .collect::<Vec<_>>();
        let charges = budgets
            .iter()
            .map(|b| {
                b.charge(DIRECTION_BYTES)
                    .unwrap_or_else(|_| panic!("global"))
            })
            .collect::<Vec<_>>();
        assert!(budget.charge(1).is_err());
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(budget.wait(64, &cancelled).await.is_err());
        drop(charges);
        assert_eq!(global.available_permits(), GLOBAL_BYTES);
    }
}
