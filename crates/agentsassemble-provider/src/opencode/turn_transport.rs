use agentsassemble_domain::DurableAgentSession;
use serde_json::Value;
use tokio::time::Instant;

use super::{OpenCodeDriver, TURN_TIMEOUT, requests};
use crate::{
    loopback_http::{JsonResponse, LoopbackStream, VerifiedLoopbackConnection},
    opencode_protocol::{turn_timeout, turn_transport_error},
    opencode_sse::{OpenCodeTurnEvents, TurnEvent, TurnEventStream},
    runtime::{DriverError, ProviderTurnRequest},
};

impl OpenCodeDriver {
    pub(super) async fn run_turn_transport(
        &mut self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
        connection: VerifiedLoopbackConnection,
        event_response: LoopbackStream,
        payload: &Value,
        path: &str,
    ) -> Result<(JsonResponse, OpenCodeTurnEvents), DriverError> {
        let prompt = connection.post_turn_json(path, payload);
        tokio::pin!(prompt);
        let mut events = TurnEventStream::new(event_response, &session.provider_session_id);
        let mut response = None;
        let mut completed_events = None;
        let mut deadline = Instant::now() + TURN_TIMEOUT;
        loop {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline) => return Err(turn_timeout()),
                result = &mut prompt, if response.is_none() => {
                    response = Some(result.map_err(turn_transport_error)?);
                }
                event = events.next(), if completed_events.is_none() => match event.map_err(turn_transport_error)? {
                    TurnEvent::Completed(events) => completed_events = Some(events),
                    TurnEvent::Request(event) => {
                        // Only this owner suspends the turn budget; each human request has its
                        // own durable deadline and native reply HTTP retains its short deadline.
                        let waiting = Instant::now();
                        requests::handle(self, &session.public.session_id, request, &event).await?;
                        deadline += waiting.elapsed();
                    }
                },
            }
            if response.is_some() && completed_events.is_some() {
                return Ok((
                    response.take().ok_or_else(turn_timeout)?,
                    completed_events.take().ok_or_else(turn_timeout)?,
                ));
            }
        }
    }
}
