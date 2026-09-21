//! Concurrent native callbacks retain one room-owned exchange through durable completion.
use std::collections::HashMap;

use agentsassemble_domain::{
    ProviderRequest, ProviderRequestPrompt, ProviderRequestResolution,
    redact_persisted_diagnostic_text,
};
use futures_util::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{ClaudeSdkClient, ClaudeSdkTurn, HostMessage, protocol_error};
use crate::{
    ProviderRequestExchange,
    driver::{DriverError, ProviderTurnRequest},
};

type Answer = (ProviderRequestExchange, ProviderRequestResolution);
type Opened = (Uuid, Result<Option<Answer>, DriverError>);

pub(super) async fn run<I, O>(
    client: &mut ClaudeSdkClient<I, O>,
    session_id: &str,
    turn: &ProviderTurnRequest,
) -> Result<ClaudeSdkTurn, DriverError>
where
    I: AsyncWrite + Unpin,
    O: AsyncRead + Unpin,
{
    let mut opening: FuturesUnordered<BoxFuture<'_, Opened>> = FuturesUnordered::new();
    let mut cancellations = HashMap::<Uuid, CancellationToken>::new();
    let mut ready = HashMap::<Uuid, ProviderRequestExchange>::new();
    loop {
        tokio::select! {
            opened = opening.next(), if !opening.is_empty() => {
                let (id, answer) = opened.ok_or_else(protocol_error)?;
                cancellations.remove(&id);
                if let Some((exchange, resolution)) = answer? {
                    ready.insert(id, exchange);
                    client.send(&serde_json::json!({"type":"request_answer", "request_id":id, "resolution":resolution})).await?;
                } else {
                    complete(client, id).await?;
                }
            }
            message = client.receive() => match message? {
                HostMessage::ProviderRequest { turn_id, mut request } => {
                    let id = request.provider_request_id;
                    if turn_id != turn.turn_id || cancellations.contains_key(&id) || ready.contains_key(&id)
                        || cancellations.len() + ready.len() >= 128 {
                        return Err(protocol_error());
                    }
                    sanitize(&mut request);
                    let ingress = turn.request_ingress.as_ref().ok_or_else(unavailable)?;
                    let cancelled = CancellationToken::new();
                    cancellations.insert(id, cancelled.clone());
                    opening.push(async move {
                        let opened = async {
                            let mut exchange = ingress.open(session_id, turn.turn_generation, &turn.execution_id, request.clone()).await.map_err(|_| unavailable())?;
                            let resolution = exchange.receive().await.map_err(|_| unavailable())?;
                            if request.durable_resolution(&resolution).is_none() { return Err(protocol_error()); }
                            Ok(Some((exchange, resolution)))
                        };
                        let result = tokio::select! {
                            biased;
                            () = cancelled.cancelled() => Ok(None),
                            result = opened => result,
                        };
                        (id, result)
                    }.boxed());
                }
                HostMessage::RequestCancelled { request_id } => {
                    if let Some(cancelled) = cancellations.get(&request_id) {
                        cancelled.cancel();
                    } else if ready.remove(&request_id).is_some() {
                        complete(client, request_id).await?;
                    } else { return Err(protocol_error()); }
                }
                HostMessage::RequestDelivered { request_id, delivered } => {
                    let mut exchange = ready.remove(&request_id).ok_or_else(protocol_error)?;
                    exchange.complete(delivered).await.map_err(|_| unavailable())?;
                    if !delivered { return Err(unavailable()); }
                    complete(client, request_id).await?;
                }
                HostMessage::TurnResult { turn_id, provider_turn_id, session_id: native_session, content }
                    if turn_id == turn.turn_id && native_session == client.session_id
                        && !provider_turn_id.is_empty() && opening.is_empty() && ready.is_empty() => {
                    return Ok(ClaudeSdkTurn { provider_turn_id, session_id: native_session, content });
                }
                _ => return Err(protocol_error()),
            }
        }
    }
}

async fn complete<I, O>(client: &mut ClaudeSdkClient<I, O>, id: Uuid) -> Result<(), DriverError>
where
    I: AsyncWrite + Unpin,
    O: AsyncRead + Unpin,
{
    client
        .send(&serde_json::json!({"type":"request_complete", "request_id":id}))
        .await
}

fn sanitize(request: &mut ProviderRequest) {
    request.title = redact_persisted_diagnostic_text(&request.title, 160);
    request.description = redact_persisted_diagnostic_text(&request.description, 1200);
    match &mut request.prompt {
        ProviderRequestPrompt::Option { options } => sanitize_options(options),
        ProviderRequestPrompt::Answers { questions } => {
            for question in questions {
                question.header = redact_persisted_diagnostic_text(&question.header, 120);
                question.question = redact_persisted_diagnostic_text(&question.question, 800);
                sanitize_options(&mut question.options);
            }
        }
        ProviderRequestPrompt::Acknowledge { .. } => {}
    }
}
fn sanitize_options(options: &mut [agentsassemble_domain::ProviderRequestOption]) {
    for option in options {
        option.label = redact_persisted_diagnostic_text(&option.label, 240);
        option.description = redact_persisted_diagnostic_text(&option.description, 400);
    }
}
const fn unavailable() -> DriverError {
    DriverError::new(
        "provider_request_unavailable",
        "The Claude provider request could not complete.",
    )
}

#[cfg(test)]
mod tests {
    use super::sanitize;
    use agentsassemble_domain::ProviderRequest;

    #[test]
    fn native_permission_detail_is_redacted_before_the_room_sees_it() {
        let mut request: ProviderRequest = serde_json::from_value(serde_json::json!({
            "provider_request_id": "07d57a67-08fd-4ce5-a8ed-ff0122a49a09",
            "request_kind": "permission",
            "title": "Claude requests permission",
            "description": "curl -H 'Authorization: Bearer sk-secret123456' https://example.test/first",
            "timeout_seconds": 600,
            "prompt": { "response_kind": "option", "options": [
                {"id":"allow-once", "label":"Allow once", "kind":"allow_once", "description":""},
                {"id":"deny", "label":"Deny", "kind":"reject_once", "description":""}
            ]}
        })).unwrap_or_else(|error| panic!("test permission request must decode: {error}"));
        sanitize(&mut request);
        assert!(request.description.contains("curl"));
        assert!(request.description.contains("example.test"));
        assert!(!request.description.contains("sk-secret123456"));
        assert!(!request.description.contains("Bearer sk-"));
    }
}
