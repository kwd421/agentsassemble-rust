#[cfg(any(unix, windows))]
mod antigravity;
#[cfg(any(unix, windows))]
mod antigravity_hook;
#[cfg(any(unix, windows))]
mod antigravity_terminal;
#[cfg(any(unix, windows))]
mod antigravity_transcript;
#[cfg(any(unix, windows))]
mod antigravity_transport;
#[cfg(unix)]
mod antigravity_unix;
#[cfg(windows)]
mod antigravity_windows;
mod catalog;
mod codex;
mod codex_identity;
mod configuration;
mod filesystem;
#[cfg(unix)]
mod guardian;
#[cfg(unix)]
mod guardian_health;
mod launch_error;
mod loopback_http;
mod opencode;
mod opencode_protocol;
mod opencode_sse;
mod process;
mod profile;
mod room_portal;
mod room_portal_mcp;
#[cfg(any(unix, windows))]
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
#[cfg(any(unix, windows))]
pub use room_portal_terminal::run_room_helper_if_requested;
pub use runtime::{
    ProviderAdapter, ProviderAdapterError, ProviderRoomObservation, ProviderRuntimeGone,
    ProviderRuntimeObservation, ProviderRuntimeStarted, ProviderShutdownOutcome,
    ProviderTurnCompleted, ProviderTurnRequest,
};
pub use selection::{ProviderSelection, ProviderSelectionError, creation_start_requested};
