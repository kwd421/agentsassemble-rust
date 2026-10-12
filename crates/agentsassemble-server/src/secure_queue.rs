//! Weighted ciphertext queue budgets shared by every channel in one runtime.
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

pub(crate) const CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const WINDOW_BYTES: usize = 384 * 1024;
pub(crate) const BODY_WINDOW_BYTES: usize = WINDOW_BYTES - CHUNK_BYTES;
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

pub(crate) const WINDOW_CAPABILITY: &str = "x-agentsassemble-body-window";
pub(crate) const WINDOW_CONFIRMED: &str = "x-agentsassemble-body-window-confirmed";
pub(crate) struct IncomingWindow {
    pub(crate) available: Arc<Semaphore>,
    _reservation: Charge,
}
impl IncomingWindow {
    pub(crate) fn reserve(budget: &Budget) -> Result<Self, ()> {
        Ok(Self {
            available: Arc::new(Semaphore::new(WINDOW_BYTES)),
            _reservation: budget.charge(WINDOW_BYTES)?,
        })
    }
}

pub(crate) enum Feedback {
    Cancel { id: u32, receipt: bool },
    CancelConfirm(u32),
    Ack(u64),
    Credit { id: u32, offset: u64 },
}

pub(crate) struct Queued {
    pub(crate) http: Option<u32>,
    pub(crate) header: bool,
    pub(crate) ack: bool,
    pub(crate) retire: Option<u32>,
    pub(crate) window: Option<(u32, u64, OwnedSemaphorePermit, OwnedSemaphorePermit)>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) _charge: Charge,
    pub(crate) cancel: CancellationToken,
    pub(crate) progress: Option<crate::http_body_policy::Progress>,
}

#[derive(Clone)]
pub(crate) struct Output {
    windowed: bool,
    http: Option<u32>,
    tx: mpsc::Sender<Queued>,
    pub(crate) budget: Budget,
    data: Arc<Semaphore>,
    body_data: Arc<Semaphore>,
    cancel: CancellationToken,
    progress: Option<crate::http_body_policy::Progress>,
    credit_progress: Option<crate::http_body_policy::Progress>,
}

impl Output {
    pub(crate) fn new(
        global: Arc<Semaphore>,
        cancel: CancellationToken,
    ) -> (Self, mpsc::Receiver<Queued>) {
        let (tx, rx) = mpsc::channel(128);
        (
            Self {
                windowed: false,
                http: None,
                tx,
                budget: Budget::new(global),
                data: Arc::new(Semaphore::new(WINDOW_BYTES)),
                body_data: Arc::new(Semaphore::new(BODY_WINDOW_BYTES)),
                cancel,
                progress: None,
                credit_progress: None,
            },
            rx,
        )
    }
    pub(crate) fn with_http(&self, id: u32, windowed: bool) -> Self {
        Self {
            windowed,
            http: Some(id),
            ..self.clone()
        }
    }
    pub(crate) fn with_cancel(&self, cancel: CancellationToken) -> Self {
        Self {
            cancel,
            body_data: Arc::new(Semaphore::new(BODY_WINDOW_BYTES)),
            ..self.clone()
        }
    }
    pub(crate) fn with_progress(
        &self,
        progress: Option<crate::http_body_policy::Progress>,
    ) -> Self {
        Self {
            credit_progress: if self.windowed {
                progress
                    .as_ref()
                    .map(|_| crate::http_body_policy::Progress::pending_credit())
            } else {
                None
            },
            progress,
            ..self.clone()
        }
    }
    pub(crate) async fn send(&self, frame: serde_json::Value) -> Result<(), ()> {
        let ack = !matches!(
            frame.get("op").and_then(serde_json::Value::as_str),
            Some("credit" | "cancelled")
        );
        let header = self.windowed
            && frame.get("op").and_then(serde_json::Value::as_str) == Some("response");
        let retire = (self.windowed
            && frame.get("op").and_then(serde_json::Value::as_str) == Some("error"))
        .then_some(self.http)
        .flatten();
        self.enqueue(
            serde_json::to_vec(&frame).map_err(|_| ())?,
            None,
            ack,
            header,
            retire,
        )
        .await
    }
    pub(crate) async fn send_bytes(&self, bytes: Vec<u8>) -> Result<(), ()> {
        self.enqueue(bytes, None, true, false, None).await
    }
    pub(crate) async fn send_window(&self, id: u32, offset: u64, bytes: Vec<u8>) -> Result<(), ()> {
        let send = async {
            let length = bytes.len() - 18;
            let body_permit = if length == 0 {
                self.body_data
                    .clone()
                    .try_acquire_many_owned(0)
                    .map_err(|_| ())?
            } else {
                tokio::select! {
                    () = self.cancel.cancelled() => return Err(()),
                    permit = self.body_data.clone().acquire_many_owned(length as u32) => permit.map_err(|_| ())?,
                }
            };
            let _source_wait = self
                .progress
                .as_ref()
                .map(|progress| progress.server_wait());
            let _credit_wait = self
                .credit_progress
                .as_ref()
                .filter(|_| self.body_data.available_permits() == BODY_WINDOW_BYTES - length)
                .map(|progress| progress.server_wait());
            let permit = if length == 0 {
                self.data
                    .clone()
                    .try_acquire_many_owned(0)
                    .map_err(|_| ())?
            } else {
                tokio::select! {
                    () = self.cancel.cancelled() => return Err(()),
                    permit = self.data.clone().acquire_many_owned(length as u32) => permit.map_err(|_| ())?,
                }
            };
            self.enqueue(
                bytes,
                Some((id, offset + length as u64, permit, body_permit)),
                false,
                false,
                None,
            )
            .await
        };
        if let Some(progress) = &self.progress {
            tokio::time::timeout_at(progress.ceiling(), send)
                .await
                .map_err(|_| ())?
        } else {
            send.await
        }
    }
    async fn enqueue(
        &self,
        bytes: Vec<u8>,
        window: Option<(u32, u64, OwnedSemaphorePermit, OwnedSemaphorePermit)>,
        ack: bool,
        header: bool,
        retire: Option<u32>,
    ) -> Result<(), ()> {
        if bytes.len() > 96 * 1024 {
            return Err(());
        }
        let progress = if window.is_some() {
            self.credit_progress.clone()
        } else {
            self.progress.clone()
        };
        let charge = self.budget.wait(bytes.len() + 24, &self.cancel).await?;
        tokio::select! {
            () = self.cancel.cancelled() => Err(()),
            result = self.tx.send(Queued { http: self.http, header, ack, retire, window, bytes, _charge: charge, cancel: self.cancel.clone(), progress }) => result.map_err(|_| ()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn unacknowledged_head_drops_cancelled_tail_and_both_charges() {
        let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
        let closed = CancellationToken::new();
        let (output, outgoing) = Output::new(global.clone(), closed.clone());
        let (sent, mut records) = mpsc::unbounded_channel();
        let sent = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
            tx.send(record).map_err(|_| ())?;
            Ok::<_, ()>(tx)
        }));
        let (_ack, acknowledgements) = mpsc::channel(1);
        let writer = tokio::spawn(crate::secure_channel::write_records(
            sent,
            |bytes| Ok(bytes.to_vec()),
            outgoing,
            acknowledgements,
            closed.clone(),
            output.budget.clone(),
        ));
        output
            .send(serde_json::json!({"op":"response"}))
            .await
            .unwrap();
        records.recv().await.unwrap(); // Non-progress head is waiting for its ACK.
        let cancel = CancellationToken::new();
        let download = output
            .with_cancel(cancel.clone())
            .with_progress(Some(crate::http_body_policy::Progress::new()));
        for _ in 0..5 {
            download
                .send(serde_json::json!({"body":"x".repeat(CHUNK_BYTES)}))
                .await
                .unwrap();
        }
        cancel.cancel();
        assert!(global.available_permits() < GLOBAL_BYTES);
        assert!(output.budget.direction.available_permits() < DIRECTION_BYTES);
        tokio::time::advance(crate::http_body_policy::IDLE).await;
        writer.await.unwrap();
        assert!(closed.is_cancelled());
        assert_eq!(global.available_permits(), GLOBAL_BYTES);
        assert_eq!(output.budget.direction.available_permits(), DIRECTION_BYTES);
        assert!(records.recv().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn timely_slow_acknowledgements_receive_a_fresh_record_deadline() {
        let closed = CancellationToken::new();
        let (output, outgoing) =
            Output::new(Arc::new(Semaphore::new(GLOBAL_BYTES)), closed.clone());
        let (sent, mut records) = mpsc::unbounded_channel();
        let sent = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
            tx.send(record).map_err(|_| ())?;
            Ok::<_, ()>(tx)
        }));
        let (ack, acknowledgements) = mpsc::channel(1);
        let writer = tokio::spawn(crate::secure_channel::write_records(
            sent,
            |bytes| Ok(bytes.to_vec()),
            outgoing,
            acknowledgements,
            closed.clone(),
            output.budget.clone(),
        ));
        for sequence in 0..3 {
            output
                .send(serde_json::json!({"op":"response"}))
                .await
                .unwrap();
            records.recv().await.unwrap();
            tokio::time::advance(crate::http_body_policy::IDLE - std::time::Duration::from_secs(1))
                .await;
            assert!(!closed.is_cancelled());
            ack.send(Feedback::Ack(sequence)).await.unwrap();
        }
        drop(output);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn aggregate_and_direction_capacity_release_and_cancel() {
        let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
        let budget = Budget::new(global.clone());
        let charge = budget
            .charge(DIRECTION_BYTES)
            .unwrap_or_else(|()| panic!("capacity"));
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
                    .unwrap_or_else(|()| panic!("global"))
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

#[cfg(test)]
#[path = "secure_window_tests.rs"]
mod window_tests;
