//! Multiplexed operation ownership within one authenticated channel.
use super::Frame;
use crate::{
    AppState,
    secure_client::SecureClient,
    secure_http::{Provenance, Start},
    secure_queue::{Budget, Output},
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinSet,
};

enum Input {
    Http(crate::secure_http::Input),
    Socket(crate::secure_socket::Input),
}

pub(super) struct Requests {
    state: AppState,
    client: SecureClient,
    provenance: Provenance,
    output: Output,
    budget: Budget,
    acknowledgements: mpsc::Sender<crate::secure_queue::Feedback>,
    limits: Arc<Semaphore>,
    inputs: HashMap<u32, Input>,
    pub(super) tasks: JoinSet<u32>,
    last_id: u32,
    binary_confirmed: Arc<AtomicBool>,
    window_confirmed: Arc<AtomicBool>,
    window: Option<crate::secure_queue::IncomingWindow>,
}
impl Requests {
    pub(super) fn new(
        state: AppState,
        client: SecureClient,
        provenance: Provenance,
        output: Output,
        budget: Budget,
        acknowledgements: mpsc::Sender<crate::secure_queue::Feedback>,
    ) -> Self {
        Self {
            state,
            client,
            provenance,
            output,
            budget,
            acknowledgements,
            limits: Arc::new(Semaphore::new(32)),
            inputs: HashMap::new(),
            tasks: JoinSet::new(),
            last_id: 0,
            binary_confirmed: Arc::new(AtomicBool::new(false)),
            window_confirmed: Arc::new(AtomicBool::new(false)),
            window: None,
        }
    }
    pub(super) async fn remove(&mut self, id: u32) {
        if let Some(Input::Http(input)) = self.inputs.remove(&id) {
            input.drain().await;
        }
    }
    pub(super) async fn clear(&mut self) {
        for input in self.inputs.values() {
            if let Input::Http(input) = input {
                input.cancel.cancel();
                input.drain().await;
            }
        }
        self.inputs.clear();
        self.window.take();
    }
    async fn capacity(&mut self, id: u32) -> Result<Option<OwnedSemaphorePermit>, ()> {
        if id <= self.last_id {
            return Err(());
        }
        self.last_id = id;
        if let Ok(permit) = self.limits.clone().try_acquire_owned() {
            Ok(Some(permit))
        } else {
            let _ = self
                .output
                .send(json!({"op":"error","id":id,"code":"capacity"}))
                .await;
            Ok(None)
        }
    }
    pub(super) fn accept_binary(&mut self, plaintext: &[u8]) -> Result<(), ()> {
        if !self.binary_confirmed.load(Ordering::Acquire) {
            return Err(());
        }
        let record =
            crate::secure_body_record::decode(plaintext, crate::secure_body_record::REQUEST)?;
        match self.inputs.get_mut(&record.id) {
            Some(Input::Http(input)) => input.push_binary(&record, &self.budget),
            None if record.id <= self.last_id => Ok(()),
            _ => Err(()),
        }
    }
    pub(super) async fn accept(&mut self, frame: Frame) -> Result<(), ()> {
        match frame {
            Frame::Ack { sequence } => self
                .acknowledgements
                .send(crate::secure_queue::Feedback::Ack(sequence))
                .await
                .map_err(|_| ()),
            Frame::CancelConfirm { id } => {
                if !self.window_confirmed.load(Ordering::Acquire) {
                    return Err(());
                }
                self.acknowledgements
                    .send(crate::secure_queue::Feedback::CancelConfirm(id))
                    .await
                    .map_err(|_| ())
            }
            Frame::Credit { id, offset } => {
                if !self.window_confirmed.load(Ordering::Acquire) {
                    return Err(());
                }
                self.acknowledgements
                    .send(crate::secure_queue::Feedback::Credit { id, offset })
                    .await
                    .map_err(|_| ())
            }
            Frame::Request {
                id,
                method,
                path,
                headers,
                upload_credit,
            } => {
                let mode = headers
                    .iter()
                    .find(|(name, _)| {
                        name.eq_ignore_ascii_case(crate::secure_queue::WINDOW_CAPABILITY)
                    })
                    .map(|(_, value)| value.as_str());
                if mode == Some("1") && self.window.is_none() {
                    self.window = Some(crate::secure_queue::IncomingWindow::reserve(&self.budget)?);
                }
                self.open_http(Start {
                    id,
                    method,
                    path,
                    headers,
                    upload_credit,
                    binary_confirmed: self.binary_confirmed.clone(),
                    window_confirmed: self.window_confirmed.clone(),
                    window: self.window.as_ref().map(|window| window.available.clone()),
                })
                .await
            }
            Frame::Data { id, data, end } => match self.inputs.get_mut(&id) {
                Some(Input::Http(input)) => input.push(&data, end, &self.budget),
                None if id <= self.last_id => Ok(()),
                _ => Err(()),
            },
            Frame::Cancel { id } => {
                let receipt = matches!(self.inputs.get(&id), Some(Input::Http(input)) if input.window.is_some());
                if let Some(Input::Http(input)) = self.inputs.remove(&id) {
                    input.cancel.cancel();
                    input.drain().await;
                }
                self.acknowledgements
                    .send(crate::secure_queue::Feedback::Cancel { id, receipt })
                    .await
                    .map_err(|_| ())
            }
            Frame::SocketOpen { id, ticket } => Box::pin(self.open_socket(id, &ticket)).await,
            Frame::SocketData { id, data, end } => match self.inputs.get_mut(&id) {
                Some(Input::Socket(input)) => input.push(&data, end, &self.budget),
                None if id <= self.last_id => Ok(()),
                _ => Err(()),
            },
            Frame::SocketClose { id } => {
                self.inputs.remove(&id);
                Ok(())
            }
        }
    }
    async fn open_http(&mut self, start: Start) -> Result<(), ()> {
        let id = start.id;
        let Some(permit) = self.capacity(id).await? else {
            return Ok(());
        };
        match crate::secure_http::start(
            self.state.clone(),
            &self.client,
            &self.provenance,
            &start,
            &self.output,
            permit,
        ) {
            Ok((input, task)) => {
                self.inputs.insert(id, Input::Http(input));
                self.tasks.spawn(async move {
                    task.await;
                    id
                });
                Ok(())
            }
            Err(()) => {
                self.output
                    .send(json!({"op":"error","id":id,"code":"request_rejected"}))
                    .await
            }
        }
    }
    async fn open_socket(&mut self, id: u32, ticket: &str) -> Result<(), ()> {
        let Some(permit) = self.capacity(id).await? else {
            return Ok(());
        };
        match crate::secure_socket::start(
            self.state.clone(),
            &self.client,
            ticket,
            id,
            self.output.clone(),
        )
        .await
        {
            Ok((input, task)) => {
                self.inputs.insert(id, Input::Socket(input));
                self.tasks.spawn(async move {
                    let _permit = permit;
                    task.await;
                    id
                });
                Ok(())
            }
            Err(()) => {
                self.output
                    .send(json!({"op":"error","id":id,"code":"socket_rejected"}))
                    .await
            }
        }
    }
}
