use agentsassemble_domain::DurableAgentSession;
use serde_json::Value;

use crate::{
    ProviderCredentialId, credentials::ProviderCredentialError, driver::DriverError,
    openai_stream::OpenAiStreamError,
};

#[derive(Clone, Copy)]
pub(crate) struct RemoteOpenAiErrors {
    pub(crate) credential_required: &'static str,
    pub(crate) credential_invalid: &'static str,
    pub(crate) credential_rejected: &'static str,
    pub(crate) context_limit: &'static str,
    pub(crate) rate_limited: &'static str,
    pub(crate) invalid_response: &'static str,
    pub(crate) invalid_tool_call: &'static str,
    pub(crate) api_unavailable: &'static str,
    pub(crate) session_mismatch: &'static str,
    pub(crate) session_changed: &'static str,
    pub(crate) already_bound: &'static str,
    pub(crate) room_action_missing: &'static str,
    pub(crate) tool_round_limit: &'static str,
    pub(crate) room_read_missing: &'static str,
    pub(crate) interrupt_uncertain: &'static str,
}

pub(crate) struct RemoteOpenAiSpec {
    pub(crate) credential: ProviderCredentialId,
    pub(crate) provider_kind: &'static str,
    pub(crate) endpoint: RemoteOpenAiEndpoint,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    pub(crate) request_payload: fn(&DurableAgentSession, &[Value], Option<&[Value]>) -> Value,
    pub(crate) retain_reasoning: bool,
    pub(crate) errors: RemoteOpenAiErrors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteOpenAiEndpoint {
    Fixed(&'static str),
    AgentSession,
}

impl RemoteOpenAiSpec {
    pub(crate) const fn credential_error(&self, error: ProviderCredentialError) -> DriverError {
        match error {
            ProviderCredentialError::MissingSecret => provider_error(
                "provider_credential_missing",
                self.errors.credential_required,
            ),
            ProviderCredentialError::InvalidSecret => provider_error(
                "provider_credential_invalid",
                self.errors.credential_invalid,
            ),
            ProviderCredentialError::SecureStoreUnavailable => provider_error(
                "secure_store_unavailable",
                "The secure credential store is unavailable.",
            ),
        }
    }

    pub(crate) const fn stream_error(&self, error: OpenAiStreamError) -> DriverError {
        match error {
            OpenAiStreamError::ContextLimit => {
                provider_error("provider_context_limit", self.errors.context_limit)
            }
            OpenAiStreamError::CredentialRejected => provider_error(
                "provider_credential_rejected",
                self.errors.credential_rejected,
            ),
            OpenAiStreamError::RateLimited => {
                provider_error("provider_rate_limited", self.errors.rate_limited)
            }
            OpenAiStreamError::ResponseTooLarge | OpenAiStreamError::InvalidResponse => {
                provider_error("provider_protocol_invalid", self.errors.invalid_response)
            }
            OpenAiStreamError::Http | OpenAiStreamError::Transport => {
                provider_error("provider_api_unavailable", self.errors.api_unavailable)
            }
        }
    }
}

pub(crate) const fn provider_error(code: &'static str, message: &'static str) -> DriverError {
    DriverError::new(code, message)
}
