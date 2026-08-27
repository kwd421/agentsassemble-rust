use std::{ffi::OsString, path::Path, sync::Arc, time::Duration};

use parking_lot::RwLock;
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[path = "stable_entry_ownership.rs"]
mod ownership;

use ownership::{OwnershipFailure, StableOwnership};

use crate::{
    public_ingress::CanonicalPublicOrigin,
    public_ingress_process::{OwnedCommandOutcome, run_owned_command},
};

const OPERATION_ATTEMPTS: usize = 3;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(90);
const RETRY_DELAY: Duration = Duration::from_secs(15);
const MAX_KV_KEY_BYTES: usize = 128;
const WRANGLER_ENVIRONMENT: [&str; 10] = [
    "APPDATA",
    "CLOUDFLARE_ACCOUNT_ID",
    "CLOUDFLARE_API_KEY",
    "CLOUDFLARE_API_TOKEN",
    "CLOUDFLARE_EMAIL",
    "HOME",
    "LOCALAPPDATA",
    "PATH",
    "USERPROFILE",
    "WRANGLER_SEND_METRICS",
];

#[derive(Clone)]
pub struct StableEntryConfig {
    stable_url: Arc<str>,
    namespace_id: Arc<str>,
    kv_key: Arc<str>,
    publisher: Option<Arc<Path>>,
    publisher_config: Option<Arc<tempfile::TempDir>>,
}

#[derive(Debug, Error)]
pub enum StableEntryConfigError {
    #[error("stable-entry configuration could not be read")]
    Read(#[source] std::io::Error),
    #[error("stable-entry configuration is not valid JSON")]
    Json(#[source] serde_json::Error),
    #[error("stable-entry URL must be one canonical non-loopback HTTPS origin")]
    InvalidUrl,
    #[error("stable-entry namespace_id must be exactly 32 hexadecimal characters")]
    InvalidNamespace,
    #[error("stable-entry kv_key must contain 1-128 visible ASCII bytes and not start with '-'")]
    InvalidKey,
    #[error("stable-entry publisher isolation could not be prepared")]
    PublisherIsolation(#[source] std::io::Error),
}

#[derive(Debug, Error)]
#[error("stable-entry ownership could not be claimed")]
pub struct StableEntryActivationError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StableEntryFile {
    url: String,
    namespace_id: String,
    #[serde(default = "default_kv_key")]
    kv_key: String,
}

impl StableEntryConfig {
    /// Loads one explicitly selected stable-entry publication target.
    ///
    /// # Errors
    ///
    /// Fails closed when the selected file or any field is invalid.
    pub fn load(path: &Path) -> Result<Self, StableEntryConfigError> {
        let bytes = std::fs::read(path).map_err(StableEntryConfigError::Read)?;
        let file: StableEntryFile =
            serde_json::from_slice(&bytes).map_err(StableEntryConfigError::Json)?;
        Self::from_file(&file, resolve_publisher())
    }

    fn from_file(
        file: &StableEntryFile,
        publisher: Option<std::path::PathBuf>,
    ) -> Result<Self, StableEntryConfigError> {
        let origin = CanonicalPublicOrigin::parse(&file.url)
            .map_err(|_| StableEntryConfigError::InvalidUrl)?;
        let namespace_id = file.namespace_id.trim();
        if namespace_id.len() != 32 || !namespace_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StableEntryConfigError::InvalidNamespace);
        }
        let kv_key = file.kv_key.trim();
        if !(1..=MAX_KV_KEY_BYTES).contains(&kv_key.len())
            || kv_key.starts_with('-')
            || !kv_key.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(StableEntryConfigError::InvalidKey);
        }
        let publisher_config = publisher
            .as_ref()
            .map(|_| isolated_wrangler_config())
            .transpose()?
            .map(Arc::new);
        Ok(Self {
            stable_url: origin.value.into(),
            namespace_id: namespace_id.to_ascii_lowercase().into(),
            kv_key: kv_key.into(),
            publisher: publisher.map(Arc::from),
            publisher_config,
        })
    }
}

fn isolated_wrangler_config() -> Result<tempfile::TempDir, StableEntryConfigError> {
    let directory = tempfile::tempdir().map_err(StableEntryConfigError::PublisherIsolation)?;
    std::fs::write(directory.path().join("wrangler.json"), b"{}\n")
        .map_err(StableEntryConfigError::PublisherIsolation)?;
    Ok(directory)
}

fn resolve_publisher() -> Option<std::path::PathBuf> {
    which::which("wrangler")
        .ok()
        .filter(|path| is_direct_publisher(path))
}

fn is_direct_publisher(path: &Path) -> bool {
    #[cfg(windows)]
    {
        return !path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
        });
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

#[derive(Clone)]
pub(crate) struct StableEntry(Arc<StableEntryInner>);

struct StableEntryInner {
    config: Option<StableEntryConfig>,
    ownership: Option<StableOwnership>,
    projection: RwLock<StableProjection>,
    operation: Mutex<()>,
}

#[derive(Clone, Copy)]
enum StablePhase {
    Unconfigured,
    Pending,
    Ready,
    Failed,
}

struct StableProjection {
    phase: StablePhase,
    published_target: Option<String>,
    last_error: Option<String>,
    cleanup_failed: bool,
}

pub(crate) struct StableStatus {
    pub(crate) phase: &'static str,
    pub(crate) url: String,
    pub(crate) target: Option<String>,
    pub(crate) last_error: Option<String>,
}

impl StableEntry {
    pub(crate) async fn new(
        config: Option<StableEntryConfig>,
        state_root: &Path,
    ) -> Result<Self, StableEntryActivationError> {
        let phase = if config.is_some() {
            StablePhase::Pending
        } else {
            StablePhase::Unconfigured
        };
        let ownership = config.as_ref().map(|_| StableOwnership::new(state_root));
        if let Some(ownership) = ownership.as_ref() {
            ownership
                .claim(&CancellationToken::new())
                .await
                .map_err(|_| StableEntryActivationError)?;
        }
        Ok(Self(Arc::new(StableEntryInner {
            config,
            ownership,
            projection: RwLock::new(StableProjection {
                phase,
                published_target: None,
                last_error: None,
                cleanup_failed: false,
            }),
            operation: Mutex::new(()),
        })))
    }

    pub(crate) async fn publish(&self, target: &str, cancellation: &CancellationToken) {
        self.apply(Some(target), cancellation).await;
    }

    pub(crate) fn begin_generation(&self) {
        let mut projection = self.0.projection.write();
        if self.0.config.is_some() && !projection.cleanup_failed {
            projection.phase = StablePhase::Pending;
            projection.published_target = None;
            projection.last_error = None;
        }
    }

    pub(crate) async fn clear(&self) -> bool {
        if self.clear_confirmed() {
            return true;
        }
        matches!(
            self.apply(None, &CancellationToken::new()).await,
            StableApplyOutcome::Applied
                | StableApplyOutcome::Superseded
                | StableApplyOutcome::Unconfigured
        )
    }

    pub(crate) fn status(&self) -> StableStatus {
        let projection = self.0.projection.read();
        let url = if projection.published_target.is_some()
            && matches!(projection.phase, StablePhase::Ready)
        {
            self.0
                .config
                .as_ref()
                .map_or_else(String::new, |config| config.stable_url.to_string())
        } else {
            String::new()
        };
        StableStatus {
            phase: match projection.phase {
                StablePhase::Unconfigured => "unconfigured",
                StablePhase::Pending => "pending",
                StablePhase::Ready => "ready",
                StablePhase::Failed => "failed",
            },
            url,
            target: projection.published_target.clone(),
            last_error: projection.last_error.clone(),
        }
    }

    fn clear_confirmed(&self) -> bool {
        let projection = self.0.projection.read();
        self.0.config.is_none()
            || matches!(projection.phase, StablePhase::Ready)
                && projection.published_target.is_none()
    }

    async fn apply(
        &self,
        target: Option<&str>,
        cancellation: &CancellationToken,
    ) -> StableApplyOutcome {
        let Some(config) = self.0.config.as_ref() else {
            return StableApplyOutcome::Unconfigured;
        };
        let Some(ownership) = self.0.ownership.as_ref() else {
            unreachable!("configured stable entry must own publication state");
        };
        let _operation = self.0.operation.lock().await;
        if self.0.projection.read().cleanup_failed {
            return StableApplyOutcome::Failed;
        }
        {
            let mut projection = self.0.projection.write();
            projection.phase = StablePhase::Pending;
            projection.published_target = None;
            projection.last_error = None;
        }
        let result = run_operation(config, ownership, target, cancellation).await;
        let mut projection = self.0.projection.write();
        match result {
            Ok(StableMutation::Applied) => {
                projection.phase = StablePhase::Ready;
                projection.published_target = target.map(str::to_owned);
                projection.last_error = None;
                projection.cleanup_failed = false;
                StableApplyOutcome::Applied
            }
            Ok(StableMutation::Superseded) => {
                projection.phase = StablePhase::Failed;
                projection.published_target = None;
                projection.last_error = Some("stable-entry ownership was superseded".to_owned());
                projection.cleanup_failed = false;
                StableApplyOutcome::Superseded
            }
            Err(failure) => {
                projection.phase = StablePhase::Failed;
                projection.published_target = None;
                projection.last_error = Some(failure.message);
                projection.cleanup_failed = failure.cleanup_failed;
                StableApplyOutcome::Failed
            }
        }
    }
}

enum StableApplyOutcome {
    Unconfigured,
    Applied,
    Superseded,
    Failed,
}

enum StableMutation {
    Applied,
    Superseded,
}

async fn run_operation(
    config: &StableEntryConfig,
    ownership: &StableOwnership,
    target: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<StableMutation, StableOperationFailure> {
    let executable = config
        .publisher
        .as_deref()
        .ok_or_else(|| StableOperationFailure::safe("stable-entry publisher is unavailable"))?;
    if cancellation.is_cancelled() {
        return Err(StableOperationFailure::safe(
            "stable-entry publication was cancelled",
        ));
    }
    let arguments = wrangler_arguments(config, target);
    for attempt in 0..OPERATION_ATTEMPTS {
        if cancellation.is_cancelled() {
            return Err(StableOperationFailure::safe(
                "stable-entry publication was cancelled",
            ));
        }
        let Some(ownership_guard) = ownership
            .lock_if_current(cancellation)
            .await
            .map_err(|failure| ownership_failure(failure, target.is_none()))?
        else {
            return Ok(StableMutation::Superseded);
        };
        match run_owned_command(
            executable,
            &arguments,
            &WRANGLER_ENVIRONMENT,
            cancellation,
            OPERATION_TIMEOUT,
        )
        .await
        {
            Ok(OwnedCommandOutcome::Exited(status)) if status.success() => {
                return Ok(StableMutation::Applied);
            }
            Ok(OwnedCommandOutcome::Cancelled) => {
                return Err(StableOperationFailure::safe(
                    "stable-entry publication was cancelled",
                ));
            }
            Ok(OwnedCommandOutcome::CleanupFailed) => {
                return Err(StableOperationFailure {
                    message: "stable-entry publisher cleanup failed".to_owned(),
                    cleanup_failed: true,
                });
            }
            Ok(
                OwnedCommandOutcome::Exited(_)
                | OwnedCommandOutcome::TimedOut
                | OwnedCommandOutcome::WaitFailed,
            )
            | Err(_) => {}
        }
        drop(ownership_guard);
        if attempt + 1 < OPERATION_ATTEMPTS {
            tokio::select! {
                () = cancellation.cancelled() => {
                    return Err(StableOperationFailure::safe(
                        "stable-entry publication was cancelled",
                    ));
                }
                () = tokio::time::sleep(RETRY_DELAY) => {}
            }
        }
    }
    Err(StableOperationFailure::safe(if target.is_some() {
        "stable-entry publication failed"
    } else {
        "stable-entry cleanup failed"
    }))
}

fn ownership_failure(failure: OwnershipFailure, cleanup: bool) -> StableOperationFailure {
    match failure {
        OwnershipFailure::Cancelled => {
            StableOperationFailure::safe("stable-entry publication was cancelled")
        }
        OwnershipFailure::Unavailable => StableOperationFailure {
            message: "stable-entry ownership could not be verified".to_owned(),
            cleanup_failed: cleanup,
        },
    }
}

struct StableOperationFailure {
    message: String,
    cleanup_failed: bool,
}

impl StableOperationFailure {
    fn safe(message: &str) -> Self {
        Self {
            message: message.to_owned(),
            cleanup_failed: false,
        }
    }
}

fn wrangler_arguments(config: &StableEntryConfig, target: Option<&str>) -> Vec<OsString> {
    let mut arguments = ["kv", "key"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    match target {
        Some(target) => arguments.extend([
            OsString::from("put"),
            OsString::from(config.kv_key.as_ref()),
            OsString::from(target),
        ]),
        None => arguments.extend([
            OsString::from("delete"),
            OsString::from(config.kv_key.as_ref()),
        ]),
    }
    arguments.extend([
        OsString::from(format!("--namespace-id={}", config.namespace_id)),
        OsString::from("--remote"),
    ]);
    let publisher_config = config
        .publisher_config
        .as_ref()
        .unwrap_or_else(|| unreachable!("available publisher must own isolated config"));
    let mut config_argument = OsString::from("--config=");
    config_argument.push(publisher_config.path().join("wrangler.json"));
    arguments.push(config_argument);
    arguments
}

fn default_kv_key() -> String {
    "target".to_owned()
}

#[cfg(test)]
#[path = "stable_entry_tests.rs"]
mod tests;
