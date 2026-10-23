//! The built-in harness's model-written summaries: in-turn compaction and
//! memory pieces both ask the agent's own model, so they share one call.
use agentsassemble_domain::DurableAgentSession;
use serde_json::Value;

use super::{RemoteOpenAiDriver, provider_error};
use crate::{
    credentials::ProviderCredential, driver::DriverError,
    remote_openai_spec::ResponseModelIdentity, turn_compaction,
};

impl RemoteOpenAiDriver {
    /// pi-style compaction: past 75% of what the window leaves after the reply
    /// limit (a carried room conversation sooner), the oldest part is replaced
    /// by the model's own summary of it. A fresh usage report follows the next
    /// reply.
    pub(super) async fn compact(
        &self,
        session: &DurableAgentSession,
        credential: Option<&ProviderCredential>,
        messages: &mut Vec<Value>,
        window: &mut turn_compaction::Window,
        measured: Option<(usize, u64)>,
    ) -> Result<(), DriverError> {
        if turn_compaction::used_tokens(messages, measured) <= window.threshold {
            return Ok(());
        }
        let Some((start, cut)) = window.fold_range(messages) else {
            return Ok(());
        };
        let summary_request =
            turn_compaction::summary_request(messages, window.turn_input, start, cut);
        let summary = self
            .summarize(session, credential, &summary_request)
            .await?;
        turn_compaction::fold(messages, start, cut, &summary);
        window.folded(start, cut);
        Ok(())
    }

    /// Asks the agent's own model for a summary (compaction and memory).
    pub(super) async fn summarize(
        &self,
        session: &DurableAgentSession,
        credential: Option<&ProviderCredential>,
        summary_request: &[Value],
    ) -> Result<String, DriverError> {
        let response = self
            .api
            .complete(session, credential, summary_request, None, false)
            .await?;
        // A long or length-cut summary still serves; only a wrong model, a tool
        // call or an empty reply fails the turn.
        let wrong_model = self.spec.response_model == ResponseModelIdentity::Requested
            && response.model != session.public.model;
        (!wrong_model
            && response.message.tool_calls.is_empty()
            && matches!(response.finish_reason.as_str(), "stop" | "length"))
        .then_some(response.message.content.as_deref())
        .flatten()
        .and_then(turn_compaction::bounded_summary)
        .ok_or_else(|| {
            provider_error(
                "provider_protocol_invalid",
                self.spec.errors.invalid_response,
            )
        })
    }
}
