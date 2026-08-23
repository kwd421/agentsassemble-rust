#[cfg(unix)]
mod antigravity;
#[cfg(unix)]
mod antigravity_hook;
#[cfg(unix)]
mod antigravity_terminal;
#[cfg(unix)]
mod antigravity_transcript;
mod catalog;
mod codex;
mod codex_identity;
mod configuration;
mod filesystem;
#[cfg(unix)]
mod guardian;
mod launch_error;
mod loopback_http;
mod opencode;
mod opencode_protocol;
mod opencode_sse;
mod process;
mod profile;
mod room_portal;
mod room_portal_mcp;
#[cfg(unix)]
mod room_portal_terminal;
mod runtime;
mod runtime_authority;
mod runtime_lease;
mod runtime_recovery;
mod selection;
mod selection_input;
#[cfg(unix)]
mod unix_custody;
#[cfg(unix)]
mod unix_process_tree;

pub use catalog::ProviderCatalogService;
#[cfg(unix)]
pub use guardian::run_process_helper_if_requested;
pub use room_portal::ProviderTurnOutcome;
#[cfg(unix)]
pub use room_portal_terminal::{run_antigravity_hook_if_requested, run_room_helper_if_requested};
pub use runtime::{
    ProviderAdapter, ProviderAdapterError, ProviderRoomObservation, ProviderRuntimeGone,
    ProviderRuntimeObservation, ProviderRuntimeStarted, ProviderShutdownOutcome,
    ProviderTurnCompleted, ProviderTurnRequest,
};
pub use selection::{ProviderSelection, ProviderSelectionError, creation_start_requested};
