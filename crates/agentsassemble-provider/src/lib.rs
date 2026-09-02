#[cfg(any(unix, windows))]
mod antigravity;
#[cfg(any(unix, windows))]
mod antigravity_hook;
#[cfg(any(unix, windows))]
mod antigravity_transport;
#[cfg(unix)]
mod antigravity_unix;
#[cfg(windows)]
mod antigravity_windows;
mod catalog;
mod catalog_service;
mod codex;
#[cfg(unix)]
mod codex_code_mode_host;
mod codex_identity;
mod configuration;
mod credential_provider;
mod credentials;
mod deepseek;
mod driver;
mod filesystem;
#[cfg(unix)]
mod guardian;
#[cfg(unix)]
mod guardian_health;
#[cfg(unix)]
mod guardian_lifetime;
mod launch_error;
mod loopback_http;
mod openai_stream;
mod opencode;
mod opencode_protocol;
mod opencode_sse;
mod opencode_startup;
mod process;
mod profile;
mod registration;
mod remote_https;
mod room_attachment;
mod room_portal;
mod room_portal_mcp;
mod room_portal_mcp_transport;
#[cfg(any(unix, windows))]
mod room_portal_terminal;
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
#[cfg(unix)]
mod unix_custody;
#[cfg(unix)]
mod unix_process_tree;

pub use catalog_service::ProviderCatalogService;
pub use credential_provider::ProviderCredentialId;
pub use credentials::{
    ProviderCredentialError, ProviderCredentialSource, ProviderCredentialStatus,
    ProviderCredentialStore,
};
#[cfg(unix)]
pub use guardian::run_process_helper_if_requested;
pub use profile::runtime_profile_key;
pub use room_attachment::{
    ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadError,
    ProviderAttachmentReadIngress,
};
pub use room_portal::{
    ProviderRoomToolCommand, ProviderRoomToolError, ProviderRoomToolIngress,
    ProviderRoomToolRequest, ProviderRoomToolResult, ProviderTurnOutcome,
};
#[cfg(any(unix, windows))]
pub use room_portal_terminal::run_room_helper_if_requested;
pub use runtime::{
    ProviderAdapter, ProviderAdapterError, ProviderExactTurnAuthority, ProviderPreparedTurn,
    ProviderResidentRuntime, ProviderRoomObservation, ProviderRuntimeGone,
    ProviderRuntimeObservation, ProviderRuntimeStarted, ProviderShutdownOutcome,
    ProviderStartReservation, ProviderTurnCompleted, ProviderTurnControl,
    ProviderTurnInterruptDisposition, ProviderTurnNotStartedProof, ProviderTurnQuiescence,
    ProviderTurnRequest,
};
pub use selection::{ProviderSelection, ProviderSelectionError, creation_start_requested};
