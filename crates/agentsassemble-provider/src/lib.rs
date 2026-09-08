mod acp_client;
mod acp_runtime;
mod catalog;
mod catalog_service;
mod cerebras;
mod claude;
mod claude_sdk_assets;
mod claude_sdk_client;
#[cfg(any(unix, windows))]
mod claude_sdk_runtime;
mod codex;
#[cfg(unix)]
mod codex_code_mode_host;
mod codex_identity;
#[cfg(not(unix))]
mod codex_process;
mod configuration;
mod credential_provider;
mod credentials;
mod cursor;
mod cursor_acp;
mod custom_api;
mod deepseek;
mod driver;
mod filesystem;
mod grok;
mod grok_acp;
#[cfg(unix)]
mod guardian;
#[cfg(unix)]
mod guardian_health;
#[cfg(unix)]
mod guardian_lifetime;
mod launch_cleanup;
mod launch_error;
mod llm_gateway;
mod lm_studio;
mod local_openai;
mod loopback_http;
mod managed_bridge;
mod ollama;
mod openai_stream;
mod opencode;
#[cfg(not(unix))]
mod opencode_process;
mod opencode_protocol;
mod opencode_sse;
mod opencode_startup;
mod openrouter;
mod process;
mod profile;
mod provider_factory;
mod provider_request_exchange;
mod provider_request_ingress;
mod registration;
#[cfg(test)]
mod registration_tests;
mod remote_catalog;
mod remote_https;
mod remote_openai;
mod remote_openai_spec;
mod room_attachment;
mod room_portal;
mod room_portal_mcp;
mod room_portal_mcp_transport;
mod room_portal_tool_contract;
mod runtime;
mod runtime_absence;
mod runtime_authority;
#[cfg(unix)]
mod runtime_boot;
mod runtime_handle;
mod runtime_lease;
mod runtime_recovery;
mod selection;
mod selection_input;
#[cfg(test)]
mod test_support;
mod tokenrouter;
#[cfg(unix)]
mod unix_custody;
#[cfg(unix)]
mod unix_process_tree;
mod vercel;

pub use catalog_service::{CatalogRefreshError, ProviderCatalogService};
pub use credential_provider::ProviderCredentialId;
pub use credentials::{
    ProviderCredentialError, ProviderCredentialSource, ProviderCredentialStatus,
    ProviderCredentialStore,
};
#[cfg(unix)]
pub use guardian::run_process_helper_if_requested;
pub use managed_bridge::run_managed_bridge_if_requested;
pub use profile::runtime_profile_key;
pub use provider_request_exchange::{
    ProviderRequestCompletion, ProviderRequestExchange, ProviderRequestExchangeError,
    ProviderRequestResponder,
};
pub use provider_request_ingress::{ProviderRequestCommand, ProviderRequestIngress};
pub use registration::{registered_provider_id, registered_provider_kind};
pub use room_attachment::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadError,
    ProviderAttachmentReadIngress,
};
pub use room_portal::{
    ProviderRoomToolCommand, ProviderRoomToolError, ProviderRoomToolIngress,
    ProviderRoomToolRequest, ProviderRoomToolResult, ProviderTurnOutcome,
};
pub use runtime::{
    ProviderAdapter, ProviderAdapterError, ProviderExactTurnAuthority, ProviderPreparedTurn,
    ProviderResidentRuntime, ProviderRoomObservation, ProviderRuntimeFailure, ProviderRuntimeGone,
    ProviderRuntimeObservation, ProviderRuntimeStarted, ProviderShutdownOutcome,
    ProviderStartReservation, ProviderTurnCompleted, ProviderTurnControl,
    ProviderTurnInterruptDisposition, ProviderTurnNotStartedProof, ProviderTurnQuiescence,
    ProviderTurnRequest,
};
pub use selection::{ProviderSelection, ProviderSelectionError, creation_start_requested};
