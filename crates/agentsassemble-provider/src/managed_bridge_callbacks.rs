//! Native callback custody survives until the parent returns the corresponding owner result.
use std::collections::HashMap;

use agentsassemble_domain::{ProviderRequest, ProviderRequestResolution};
use futures_util::{StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::wire::protocol_error;
use crate::{
    ProviderRequestCommand, ProviderRequestExchange, ProviderRequestExchangeError,
    ProviderRequestIngress,
    driver::{DriverError, DriverFuture},
    room_attachment::{
        ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadError,
        ProviderAttachmentReadIngress,
    },
    room_portal::{
        ProviderRoomToolCommand, ProviderRoomToolError, ProviderRoomToolIngress,
        ProviderRoomToolRequest, ProviderRoomToolResult,
    },
};

// Bounded transport custody accommodates the broker's 64 requests, 32 portal tools
// and all per-turn attachment reservations. It does not create new tool permission.
pub(super) const MAX_CALLBACKS: usize = 128;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Authority {
    pub(super) session_id: String,
    pub(super) turn_id: String,
    pub(super) input_up_to_seq: i64,
    pub(super) turn_generation: u64,
    pub(super) execution_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "callback", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Callback {
    Tool {
        authority: Authority,
        request: ProviderRoomToolRequest,
    },
    ToolBegun {
        result: Result<(), ProviderRoomToolError>,
    },
    Attachment {
        authority: Authority,
        attachment_id: String,
    },
    Request {
        session_id: String,
        turn_generation: u64,
        execution_id: String,
        request: ProviderRequest,
    },
    Delivered {
        delivered: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Reply {
    ToolBegin,
    ToolResult {
        result: Result<ProviderRoomToolResult, ProviderRoomToolError>,
    },
    Attachment {
        result: Result<ProviderAttachment, ProviderAttachmentReadError>,
    },
    RequestOpened {
        result: Result<(), ProviderRequestExchangeError>,
    },
    Resolution {
        result: Result<ProviderRequestResolution, ProviderRequestExchangeError>,
    },
    Receipt {
        result: Result<(), ProviderRequestExchangeError>,
    },
}

pub(super) struct Callbacks {
    pub(super) requests: ProviderRequestIngress,
    pub(super) attachments: ProviderAttachmentReadIngress,
    pub(super) tools: ProviderRoomToolIngress,
    request_rx: mpsc::Receiver<ProviderRequestCommand>,
    attachment_rx: mpsc::Receiver<ProviderAttachmentReadCommand>,
    tool_rx: mpsc::Receiver<ProviderRoomToolCommand>,
    events: mpsc::Sender<(u64, Callback)>,
    event_rx: mpsc::Receiver<(u64, Callback)>,
    replies: HashMap<u64, mpsc::Sender<Reply>>,
    pending: FuturesUnordered<DriverFuture<'static, Result<u64, DriverError>>>,
    next_id: u64,
}

impl Callbacks {
    pub(super) fn new() -> Self {
        let (requests, request_rx) = ProviderRequestIngress::channel(4);
        let (attachments, attachment_rx) = ProviderAttachmentReadIngress::channel(4);
        let (tools, tool_rx) = ProviderRoomToolIngress::channel(4);
        let (events, event_rx) = mpsc::channel(4);
        Self {
            requests,
            attachments,
            tools,
            request_rx,
            attachment_rx,
            tool_rx,
            events,
            event_rx,
            replies: HashMap::new(),
            pending: FuturesUnordered::new(),
            next_id: 1,
        }
    }

    pub(super) fn reply(&mut self, id: u64, reply: Reply) -> Result<(), DriverError> {
        self.replies
            .get(&id)
            .ok_or_else(protocol_error)?
            .try_send(reply)
            .map_err(|_| protocol_error())
    }

    pub(super) async fn next(&mut self) -> Result<(u64, Callback), DriverError> {
        loop {
            tokio::select! {
                event = self.event_rx.recv() => return event.ok_or_else(protocol_error),
                result = self.pending.next(), if !self.pending.is_empty() => {
                    let id = result.ok_or_else(protocol_error)??;
                    self.replies.remove(&id);
                }
                Some(command) = self.tool_rx.recv(), if self.pending.len() < MAX_CALLBACKS => {
                    let job = self.job()?;
                    self.pending.push(Box::pin(tool(job, command)));
                }
                Some(command) = self.attachment_rx.recv(), if self.pending.len() < MAX_CALLBACKS => {
                    let job = self.job()?;
                    self.pending.push(Box::pin(attachment(job, command)));
                }
                Some(command) = self.request_rx.recv(), if self.pending.len() < MAX_CALLBACKS => {
                    let job = self.job()?;
                    self.pending.push(Box::pin(request(job, command)));
                }
            }
        }
    }

    fn job(&mut self) -> Result<Job, DriverError> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or_else(protocol_error)?;
        let (sender, replies) = mpsc::channel(2);
        self.replies.insert(id, sender);
        Ok(Job {
            id,
            events: self.events.clone(),
            replies,
        })
    }
}

struct Job {
    id: u64,
    events: mpsc::Sender<(u64, Callback)>,
    replies: mpsc::Receiver<Reply>,
}

impl Job {
    async fn send(&self, callback: Callback) -> Result<(), DriverError> {
        self.events
            .send((self.id, callback))
            .await
            .map_err(|_| protocol_error())
    }
    async fn receive(&mut self) -> Result<Reply, DriverError> {
        self.replies.recv().await.ok_or_else(protocol_error)
    }
}

async fn tool(mut job: Job, mut command: ProviderRoomToolCommand) -> Result<u64, DriverError> {
    job.send(Callback::Tool {
        authority: Authority {
            session_id: command.session_id().to_owned(),
            turn_id: command.turn_id().to_owned(),
            input_up_to_seq: command.input_up_to_seq(),
            turn_generation: command.turn_generation(),
            execution_id: command.execution_id().to_owned(),
        },
        request: command.request().clone(),
    })
    .await?;
    let admitted = match job.receive().await? {
        Reply::ToolBegin => {
            let result = command.begin_execution().await;
            let admitted = result.is_ok();
            job.send(Callback::ToolBegun { result }).await?;
            admitted
        }
        // Queue or authority rejection occurs before the server can request execution.
        Reply::ToolResult { result: Err(error) } => {
            command.complete(Err(error));
            return Ok(job.id);
        }
        _ => return Err(protocol_error()),
    };
    let Reply::ToolResult { result } = job.receive().await? else {
        return Err(protocol_error());
    };
    if !admitted && result.is_ok() {
        return Err(protocol_error());
    }
    command.complete(result);
    Ok(job.id)
}

async fn attachment(
    mut job: Job,
    command: ProviderAttachmentReadCommand,
) -> Result<u64, DriverError> {
    job.send(Callback::Attachment {
        authority: Authority {
            session_id: command.session_id().to_owned(),
            turn_id: command.turn_id().to_owned(),
            input_up_to_seq: command.input_up_to_seq(),
            turn_generation: command.turn_generation(),
            execution_id: command.execution_id().to_owned(),
        },
        attachment_id: command.attachment_id().to_owned(),
    })
    .await?;
    let Reply::Attachment { result } = job.receive().await? else {
        return Err(protocol_error());
    };
    command.complete(result);
    Ok(job.id)
}

async fn request(mut job: Job, command: ProviderRequestCommand) -> Result<u64, DriverError> {
    job.send(Callback::Request {
        session_id: command.session_id.clone(),
        turn_generation: command.turn_generation,
        execution_id: command.execution_id.clone(),
        request: command.request.clone(),
    })
    .await?;
    let Reply::RequestOpened { result } = job.receive().await? else {
        return Err(protocol_error());
    };
    if let Err(error) = result {
        command.complete(Err(error));
        return Ok(job.id);
    }
    let (exchange, mut responder, mut completion) = ProviderRequestExchange::channel();
    command.complete(Ok(exchange));
    let mut answered = false;
    let delivered = tokio::select! {
        delivered = completion.completion() => delivered,
        reply = job.receive() => {
            let Reply::Resolution { result } = reply? else { return Err(protocol_error()); };
            answered = true;
            match result {
                Ok(resolution) => { if responder.respond(resolution).is_err() { responder.cancel(); } }
                Err(_) => responder.cancel(),
            }
            completion.completion().await
        }
    };
    job.send(Callback::Delivered { delivered }).await?;
    let reply = job.receive().await?;
    let reply = if !answered && matches!(reply, Reply::Resolution { .. }) {
        // The native exchange ended before the in-flight parent answer arrived.
        job.receive().await?
    } else {
        reply
    };
    let Reply::Receipt { result } = reply else {
        return Err(protocol_error());
    };
    completion.finish(result);
    Ok(job.id)
}
