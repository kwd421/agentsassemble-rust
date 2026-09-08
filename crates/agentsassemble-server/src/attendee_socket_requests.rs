//! One connection owns the live request exchange; reconnect never copies secret answers.
use agentsassemble_persistence::{
    AttendeeConnectionAuthorization, OpenProviderRequest, PersistenceError,
};
use agentsassemble_protocol::CommandResolution;
use agentsassemble_provider::ProviderRequestExchange;
use uuid::Uuid;

use crate::{AppState, AttendeeSocketFrame as Frame};

struct Pending {
    id: Uuid,
    exchange: ProviderRequestExchange,
    answer_forwarded: bool,
}

#[derive(PartialEq, Eq)]
struct DeliveryReceipt {
    request_id: Uuid,
    provider_request_id: Uuid,
    delivered: bool,
}

pub(in crate::attendee_web) struct SocketRequests {
    pending: Option<Pending>,
    completed: Option<DeliveryReceipt>,
}

impl SocketRequests {
    pub fn new() -> Self {
        Self {
            pending: None,
            completed: None,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub async fn open(
        &mut self,
        state: &AppState,
        connection: &AttendeeConnectionAuthorization,
        request_id: Uuid,
        request: OpenProviderRequest,
    ) -> Result<Frame, PersistenceError> {
        let id = request.request.provider_request_id;
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id != id)
        {
            return Err(rejected("provider_request_pending"));
        }
        let opened = state
            .rooms
            .open_attendee_request(connection.clone(), request)
            .await?;
        if let Some(exchange) = opened.exchange {
            self.pending = Some(Pending {
                id,
                exchange,
                answer_forwarded: false,
            });
            self.completed = None;
        }
        Ok(Frame::Ack {
            request_id,
            resolution: CommandResolution::Committed,
            event_id: Some(opened.commit.event.id),
            sequence: Some(opened.commit.event.seq),
            deduplicated: Some(opened.commit.deduplicated),
        })
    }

    pub async fn next(&mut self) -> Frame {
        let Some(pending) = &mut self.pending else {
            return std::future::pending().await;
        };
        let id = pending.id;
        if pending.answer_forwarded {
            pending.exchange.cancelled().await;
        } else if let Ok(resolution) = pending.exchange.receive().await {
            pending.answer_forwarded = true;
            return Frame::ProviderResponse {
                provider_request_id: id,
                resolution,
            };
        }
        self.pending = None;
        Frame::ProviderRequestClosed {
            provider_request_id: id,
        }
    }

    pub async fn complete(
        &mut self,
        state: &AppState,
        connection: &AttendeeConnectionAuthorization,
        request_id: Uuid,
        provider_request_id: Uuid,
        delivered: bool,
    ) -> Result<Frame, PersistenceError> {
        state
            .store
            .revalidate_attendee_connection(connection, chrono::Utc::now())
            .await?;
        let receipt = DeliveryReceipt {
            request_id,
            provider_request_id,
            delivered,
        };
        if let Some(completed) = &self.completed {
            if completed != &receipt {
                return Err(PersistenceError::CommandConflict);
            }
            return Ok(ack(request_id, true));
        }
        let Some(pending) = &mut self.pending else {
            return Err(rejected("provider_request_closed"));
        };
        if pending.id != provider_request_id || !pending.answer_forwarded {
            return Err(rejected("invalid_provider_delivery"));
        }
        let result = pending.exchange.complete(delivered).await;
        self.pending = None;
        result.map_err(|_| PersistenceError::CommandUnresolved {
            code: "provider_delivery_unconfirmed".into(),
            message: "The native delivery result could not be confirmed.".to_owned(),
        })?;
        self.completed = Some(receipt);
        Ok(ack(request_id, false))
    }
}

fn ack(request_id: Uuid, deduplicated: bool) -> Frame {
    Frame::Ack {
        request_id,
        resolution: CommandResolution::Committed,
        event_id: None,
        sequence: None,
        deduplicated: Some(deduplicated),
    }
}

fn rejected(code: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: "The provider request delivery is unavailable or inconsistent.".to_owned(),
    }
}
