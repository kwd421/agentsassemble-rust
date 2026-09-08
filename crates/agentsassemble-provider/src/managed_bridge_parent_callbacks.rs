//! Only the room owner's ingresses can resolve callbacks from the exact managed session.
use std::sync::Arc;

use tokio::sync::oneshot;

use super::{
    callbacks::{Authority, Callback, Reply},
    exchange::{Exchange, Exchanges},
    wire::protocol_error,
};
use crate::{
    ProviderRequestExchangeError,
    driver::{DriverError, ProviderTurnRequest},
    room_attachment::{
        ProviderAttachmentReadAuthority, ProviderAttachmentReadError, ProviderAttachmentReadIngress,
    },
    room_portal::{
        ProviderRoomToolError, ProviderRoomToolIngress, ProviderRoomToolRequest, RoomToolAuthority,
    },
};

type Job = Exchange<Reply, Callback>;

pub(super) struct ParentCallbacks {
    exchanges: Exchanges<Reply, Callback>,
}

impl ParentCallbacks {
    pub(super) fn new() -> Self {
        Self {
            exchanges: Exchanges::new(),
        }
    }

    pub(super) fn receive(
        &mut self,
        id: u64,
        callback: Callback,
        session_id: &str,
        context: Option<Arc<ProviderTurnRequest>>,
    ) -> Result<(), DriverError> {
        let opened = match callback {
            Callback::Tool { authority, request } => {
                require_session(&authority.session_id, session_id)?;
                let ingress = context
                    .as_ref()
                    .and_then(|request| request.room_observation.as_ref())
                    .and_then(|observation| observation.room_tool_ingress.clone());
                self.exchanges
                    .start(|job| tool(job, ingress, authority, request))?
            }
            Callback::Attachment {
                authority,
                attachment_id,
            } => {
                require_session(&authority.session_id, session_id)?;
                let ingress = context
                    .as_ref()
                    .and_then(|request| request.room_observation.as_ref())
                    .and_then(|observation| observation.attachment_ingress.clone());
                self.exchanges
                    .start(|job| attachment(job, ingress, authority, attachment_id))?
            }
            Callback::Request {
                session_id: owner,
                turn_generation,
                execution_id,
                request,
            } => {
                require_session(&owner, session_id)?;
                let ingress = context.and_then(|context| context.request_ingress.clone());
                self.exchanges.start(|job| {
                    request_exchange(job, ingress, owner, turn_generation, execution_id, request)
                })?
            }
            reply @ (Callback::ToolBegun { .. } | Callback::Delivered { .. }) => {
                return self.exchanges.reply(id, reply);
            }
        };
        if opened != id {
            return Err(protocol_error());
        }
        Ok(())
    }

    pub(super) async fn next(&mut self) -> Result<(u64, Reply), DriverError> {
        self.exchanges.next().await
    }
}

fn require_session(actual: &str, expected: &str) -> Result<(), DriverError> {
    if actual != expected {
        return Err(protocol_error());
    }
    Ok(())
}

async fn tool(
    mut job: Job,
    ingress: Option<ProviderRoomToolIngress>,
    authority: Authority,
    request: ProviderRoomToolRequest,
) -> Result<u64, DriverError> {
    let Some(ingress) = ingress else {
        job.send(Reply::ToolResult {
            result: Err(tool_unavailable()),
        })
        .await?;
        return Ok(job.id);
    };
    let (begin, receive_begin) =
        oneshot::channel::<oneshot::Sender<Result<(), ProviderRoomToolError>>>();
    let call = ingress.submit(
        RoomToolAuthority {
            session_id: authority.session_id,
            turn_id: authority.turn_id,
            input_up_to_seq: authority.input_up_to_seq,
            durable_turn_generation: authority.turn_generation,
            execution_id: authority.execution_id,
        },
        request,
        begin,
    );
    tokio::pin!(call);
    let mut admitted = false;
    let result = tokio::select! {
        result = &mut call => result,
        begin = receive_begin => {
            if let Ok(reply) = begin {
                job.send(Reply::ToolBegin).await?;
                let response = tokio::time::timeout(super::CONTROL_TIMEOUT, job.receive()).await.map_err(|_| protocol_error())??;
                let Callback::ToolBegun { result } = response else { return Err(protocol_error()); };
                let allowed = result.is_ok();
                let transferred = reply.send(result).is_ok();
                admitted = allowed && transferred;
            }
            call.await
        }
    };
    if result.is_ok() && !admitted {
        return Err(protocol_error());
    }
    job.send(Reply::ToolResult { result }).await?;
    Ok(job.id)
}

async fn attachment(
    job: Job,
    ingress: Option<ProviderAttachmentReadIngress>,
    authority: Authority,
    attachment_id: String,
) -> Result<u64, DriverError> {
    let result = if let Some(ingress) = ingress {
        ingress
            .read(
                ProviderAttachmentReadAuthority {
                    session_id: authority.session_id,
                    turn_id: authority.turn_id,
                    input_up_to_seq: authority.input_up_to_seq,
                    turn_generation: authority.turn_generation,
                    execution_id: authority.execution_id,
                },
                attachment_id,
            )
            .await
    } else {
        Err(ProviderAttachmentReadError {
            code: "room_unavailable".into(),
            message: "The room attachment owner is unavailable.".to_owned(),
        })
    };
    job.send(Reply::Attachment { result }).await?;
    Ok(job.id)
}

async fn request_exchange(
    mut job: Job,
    ingress: Option<crate::ProviderRequestIngress>,
    session_id: String,
    turn_generation: u64,
    execution_id: String,
    request: agentsassemble_domain::ProviderRequest,
) -> Result<u64, DriverError> {
    let result = if let Some(ingress) = ingress {
        ingress
            .open(&session_id, turn_generation, &execution_id, request)
            .await
    } else {
        Err(ProviderRequestExchangeError::Closed)
    };
    let mut exchange = match result {
        Ok(exchange) => {
            job.send(Reply::RequestOpened { result: Ok(()) }).await?;
            exchange
        }
        Err(error) => {
            job.send(Reply::RequestOpened { result: Err(error) })
                .await?;
            return Ok(job.id);
        }
    };
    let delivered = tokio::select! {
        result = exchange.receive() => {
            let answered = result.is_ok();
            job.send(Reply::Resolution { result }).await?;
            let Callback::Delivered { delivered } = job.receive().await? else { return Err(protocol_error()); };
            if delivered && !answered { return Err(protocol_error()); }
            delivered
        }
        callback = job.receive() => {
            let Callback::Delivered { delivered: false } = callback? else { return Err(protocol_error()); };
            false
        }
    };
    let result = exchange.complete(delivered).await;
    job.send(Reply::Receipt { result }).await?;
    Ok(job.id)
}

fn tool_unavailable() -> ProviderRoomToolError {
    ProviderRoomToolError {
        code: "room_unavailable".into(),
        message: "The room tool owner is unavailable.".to_owned(),
    }
}
