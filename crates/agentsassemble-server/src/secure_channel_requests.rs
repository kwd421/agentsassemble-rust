//! Multiplexed operation ownership within one authenticated channel.
use super::Frame;
use crate::{
    AppState,
    secure_client::SecureClient,
    secure_http::{Provenance, Start},
    secure_queue::{Budget, Output},
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
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
    acknowledgements: mpsc::Sender<u64>,
    limits: Arc<Semaphore>,
    inputs: HashMap<u32, Input>,
    pub(super) tasks: JoinSet<u32>,
    last_id: u32,
}
impl Requests {
    pub(super) fn new(
        state: AppState,
        client: SecureClient,
        provenance: Provenance,
        output: Output,
        budget: Budget,
        acknowledgements: mpsc::Sender<u64>,
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
        }
    }
    pub(super) fn remove(&mut self, id: u32) {
        self.inputs.remove(&id);
    }
    pub(super) fn clear(&mut self) {
        self.inputs.clear();
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
    pub(super) async fn accept(&mut self, frame: Frame) -> Result<(), ()> {
        match frame {
            Frame::Ack { sequence } => self.acknowledgements.try_send(sequence).map_err(|_| ()),
            Frame::Request {
                id,
                method,
                path,
                headers,
            } => {
                self.open_http(Start {
                    id,
                    method,
                    path,
                    headers,
                })
                .await
            }
            Frame::Data { id, data, end } => match self.inputs.get_mut(&id) {
                Some(Input::Http(input)) => input.push(&data, end, &self.budget),
                None if id <= self.last_id => Ok(()),
                _ => Err(()),
            },
            Frame::Cancel { id } => {
                if let Some(Input::Http(input)) = self.inputs.remove(&id) {
                    input.cancel.cancel();
                }
                Ok(())
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
