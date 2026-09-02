use agentsassemble_domain::DurableAgentSession;
use serde_json::Value;

use crate::{
    ProviderCredentialId, credentials::ProviderCredentialError, driver::DriverError,
    openai_stream::OpenAiStreamError,
};

#[derive(Clone, Copy)]
pub(crate) struct RemoteOpenAiErrors {
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
    pub(crate) authentication: RemoteOpenAiAuthentication,
    pub(crate) provider_kind: &'static str,
    pub(crate) endpoint: RemoteOpenAiEndpoint,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    pub(crate) request_payload: fn(&DurableAgentSession, &[Value], Option<&[Value]>) -> Value,
    pub(crate) retain_reasoning: bool,
    pub(crate) errors: RemoteOpenAiErrors,
}

#[derive(Clone, Copy)]
pub(crate) enum RemoteOpenAiAuthentication {
    Bearer {
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
    AgentSession,
}

impl RemoteOpenAiEndpoint {
    pub(crate) const fn transport(self) -> &'static str {
        match self {
            Self::Fixed(_) | Self::AgentSession => "https",
            Self::FixedLoopback(_) => "http",
        }
    }
}

impl RemoteOpenAiSpec {
    pub(crate) const fn credential_id(&self) -> Option<ProviderCredentialId> {
        match self.authentication {
            RemoteOpenAiAuthentication::Bearer { credential, .. } => Some(credential),
            RemoteOpenAiAuthentication::Unauthenticated => None,
        }
    }

    pub(crate) const fn credential_error(&self, error: ProviderCredentialError) -> DriverError {
        let RemoteOpenAiAuthentication::Bearer {
            required, invalid, ..
        } = self.authentication
        else {
            return provider_error("provider_protocol_invalid", self.errors.invalid_response);
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

    pub(crate) const fn stream_error(&self, error: OpenAiStreamError) -> DriverError {
        match error {
            OpenAiStreamError::ContextLimit => {
                provider_error("provider_context_limit", self.errors.context_limit)
            }
            OpenAiStreamError::CredentialRejected => match self.authentication {
                RemoteOpenAiAuthentication::Bearer { rejected, .. } => {
                    provider_error("provider_credential_rejected", rejected)
                }
                RemoteOpenAiAuthentication::Unauthenticated => {
                    provider_error("provider_api_unavailable", self.errors.api_unavailable)
                }
            },
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
