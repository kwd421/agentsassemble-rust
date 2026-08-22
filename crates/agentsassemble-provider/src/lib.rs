mod catalog;
mod codex;
mod codex_identity;
mod filesystem;
#[cfg(unix)]
mod guardian;
mod launch_error;
mod process;
mod profile;
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
pub use runtime::{
    ProviderAdapter, ProviderAdapterError, ProviderRuntimeGone, ProviderRuntimeObservation,
    ProviderRuntimeStarted, ProviderShutdownOutcome,
};
pub use selection::{ProviderSelection, ProviderSelectionError};
