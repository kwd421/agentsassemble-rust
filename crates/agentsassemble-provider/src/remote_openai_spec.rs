use agentsassemble_domain::DurableAgentSession;
use serde_json::Value;

use crate::{
    ProviderCredentialId, credentials::ProviderCredentialError, driver::DriverError,
    openai_stream::OpenAiStreamError,
};

/// Names the header and value that carry a session's service tier.
pub(crate) type ServiceTierHeader =
    fn(&DurableAgentSession) -> Option<(&'static str, &'static str)>;

#[derive(Clone, Copy)]
pub(crate) struct RemoteOpenAiErrors {
    pub(crate) context_limit: &'static str,
    pub(crate) rate_limited: &'static str,
    pub(crate) invalid_response: &'static str,
    /// The model stopped at the response limit, which is a bounded outcome, not a protocol fault.
    pub(crate) output_truncated: &'static str,
    pub(crate) invalid_tool_call: &'static str,
    pub(crate) api_unavailable: &'static str,
    pub(crate) session_mismatch: &'static str,
    pub(crate) session_changed: &'static str,
    pub(crate) already_bound: &'static str,
    pub(crate) room_action_missing: &'static str,
    pub(crate) room_read_missing: &'static str,
    pub(crate) interrupt_uncertain: &'static str,
}

pub(crate) struct RemoteOpenAiSpec {
    pub(crate) authentication: RemoteOpenAiAuthentication,
    pub(crate) provider_kind: &'static str,
    pub(crate) endpoint: RemoteOpenAiEndpoint,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    /// The selected tier travels in a request header for providers whose
    /// OpenAI-compatible endpoint does not read it from the payload.
    pub(crate) service_tier_header: Option<ServiceTierHeader>,
    pub(crate) request_payload: fn(&DurableAgentSession, &[Value], Option<&[Value]>) -> Value,
    pub(crate) response_model: ResponseModelIdentity,
    pub(crate) retain_reasoning: bool,
    /// A room turn can only end through a terminal room tool, so a provider that
    /// ignores the plain-text rule can be told to call a tool on those rounds.
    /// Only a provider whose API was verified to honor `tool_choice` sets it.
    pub(crate) require_tool_choice_on_observation: bool,
    /// AI Studio reports a streamed tool-call turn as `stop` instead of
    /// `tool_calls`. Only a provider whose stream was verified to do this sets it.
    pub(crate) tool_calls_may_finish_with_stop: bool,
    pub(crate) errors: RemoteOpenAiErrors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseModelIdentity {
    Requested,
    /// The selected endpoint resolves the requested model name. Its response is
    /// metadata, not authority to change the configured model or session.
    EndpointResolved,
}

#[derive(Clone, Copy)]
pub(crate) enum RemoteOpenAiAuthentication {
    Bearer {
        credential: ProviderCredentialId,
        required: &'static str,
        invalid: &'static str,
        rejected: &'static str,
    },
    /// The provider authenticates with a fixed API-key header instead of Bearer.
    Header {
        name: &'static str,
        credential: ProviderCredentialId,
        required: &'static str,
        invalid: &'static str,
        rejected: &'static str,
    },
    Unauthenticated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteOpenAiEndpoint {
    Fixed(&'static str),
    FixedLoopback(&'static str),
    /// The caller owns the completion endpoint under the provider's fixed host.
    AgentSession,
    /// The caller owns project and location settings; the provider fixes the URL shape.
    AgentSettings,
}

impl RemoteOpenAiEndpoint {
    pub(crate) const fn transport(self) -> &'static str {
        match self {
            Self::Fixed(_) | Self::AgentSession | Self::AgentSettings => "https",
            Self::FixedLoopback(_) => "http",
        }
    }
}

impl RemoteOpenAiSpec {
    /// Build the request body. An observation turn must end with a room action,
    /// so a provider configured for it is asked for a tool call on every round,
    /// but only while tool definitions are actually part of the request.
    pub(crate) fn request_body(
        &self,
        session: &DurableAgentSession,
        messages: &[Value],
        tools: Option<&[Value]>,
        observation: bool,
    ) -> Value {
        let mut payload = (self.request_payload)(session, messages, tools);
        if observation && tools.is_some() && self.require_tool_choice_on_observation {
            payload["tool_choice"] = Value::String("required".to_owned());
        }
        payload
    }

    pub(crate) const fn credential_id(&self) -> Option<ProviderCredentialId> {
        match self.authentication {
            RemoteOpenAiAuthentication::Bearer { credential, .. }
            | RemoteOpenAiAuthentication::Header { credential, .. } => Some(credential),
            RemoteOpenAiAuthentication::Unauthenticated => None,
        }
    }

    pub(crate) const fn credential_error(&self, error: ProviderCredentialError) -> DriverError {
        let (required, invalid) = match self.authentication {
            RemoteOpenAiAuthentication::Bearer {
                required, invalid, ..
            }
            | RemoteOpenAiAuthentication::Header {
                required, invalid, ..
            } => (required, invalid),
            RemoteOpenAiAuthentication::Unauthenticated => {
                return provider_error("provider_protocol_invalid", self.errors.invalid_response);
            }
        };
        match error {
            ProviderCredentialError::MissingSecret => {
                provider_error("provider_credential_missing", required)
            }
            ProviderCredentialError::InvalidSecret => {
                provider_error("provider_credential_invalid", invalid)
            }
            ProviderCredentialError::SecureStoreUnavailable => provider_error(
                "secure_store_unavailable",
                "The secure credential store is unavailable.",
            ),
        }
    }

    pub(crate) fn stream_error(&self, error: OpenAiStreamError) -> DriverError {
        match error {
            OpenAiStreamError::ContextLimit => {
                provider_error("provider_context_limit", self.errors.context_limit)
            }
            OpenAiStreamError::CredentialRejected => match self.authentication {
                RemoteOpenAiAuthentication::Bearer { rejected, .. }
                | RemoteOpenAiAuthentication::Header { rejected, .. } => {
                    provider_error("provider_credential_rejected", rejected)
                }
                RemoteOpenAiAuthentication::Unauthenticated => {
                    provider_error("provider_api_unavailable", self.errors.api_unavailable)
                }
            },
            OpenAiStreamError::RateLimited => {
                provider_error("provider_rate_limited", self.errors.rate_limited)
            }
            // The HTTP status is the only provider detail kept: a number the room can show.
            OpenAiStreamError::Overloaded(status) => DriverError {
                code: "provider_api_busy".into(),
                message: format!("The AI service is temporarily overloaded (HTTP {status}).")
                    .into(),
            },
            OpenAiStreamError::ResponseTooLarge | OpenAiStreamError::InvalidResponse => {
                provider_error("provider_protocol_invalid", self.errors.invalid_response)
            }
            // OpenAI-compatible APIs answer 404 when the model is gone, as a
            // retired free model on OpenRouter does.
            OpenAiStreamError::Http(404) => provider_error(
                "provider_model_unavailable",
                "The AI service does not offer this model (HTTP 404).",
            ),
            OpenAiStreamError::Http(status) => DriverError {
                code: "provider_api_unavailable".into(),
                message: format!("{} (HTTP {status})", self.errors.api_unavailable).into(),
            },
            OpenAiStreamError::Transport => {
                provider_error("provider_api_unavailable", self.errors.api_unavailable)
            }
            OpenAiStreamError::Stalled => provider_error(
                "provider_turn_timeout",
                "The AI service kept the request waiting without answering.",
            ),
        }
    }
}

pub(crate) const fn provider_error(code: &'static str, message: &'static str) -> DriverError {
    DriverError::new(code, message)
}

#[cfg(test)]
mod tests {
    use crate::openai_stream::OpenAiStreamError;

    #[test]
    fn a_missing_model_is_told_apart_from_other_refusals() {
        let spec = &crate::openrouter::OPENROUTER_SPEC;
        assert_eq!(
            spec.stream_error(OpenAiStreamError::Http(404)).code,
            "provider_model_unavailable"
        );
        assert_eq!(
            spec.stream_error(OpenAiStreamError::Http(400)).code,
            "provider_api_unavailable"
        );
    }
}
