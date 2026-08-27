use std::{ffi::OsString, path::Path, sync::Arc, time::Duration};

use parking_lot::RwLock;
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    public_ingress::CanonicalPublicOrigin,
    public_ingress_process::{OwnedCommandOutcome, run_owned_command},
};

const OPERATION_ATTEMPTS: usize = 3;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(90);
#[cfg(not(test))]
const RETRY_DELAY: Duration = Duration::from_secs(15);
#[cfg(test)]
const RETRY_DELAY: Duration = Duration::ZERO;
const MAX_KV_KEY_BYTES: usize = 128;
const STABLE_ENVIRONMENT: [&str; 17] = [
    "APPDATA",
    "CLOUDFLARE_ACCOUNT_ID",
    "CLOUDFLARE_API_TOKEN",
    "HOME",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "LOCALAPPDATA",
    "NO_PROXY",
    "PATH",
    "SSL_CERT_DIR",
    "SSL_CERT_FILE",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "USERPROFILE",
    "WRANGLER_SEND_METRICS",
];

#[derive(Clone)]
pub struct StableEntryConfig {
    stable_url: Arc<str>,
    namespace_id: Arc<str>,
    kv_key: Arc<str>,
    publisher: Option<Arc<Path>>,
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
    #[error("stable-entry kv_key must contain 1-128 visible ASCII bytes")]
    InvalidKey,
}

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
        Self::from_file(&file, which::which("wrangler").ok())
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
            || !kv_key.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(StableEntryConfigError::InvalidKey);
        }
        Ok(Self {
            stable_url: origin.value.into(),
            namespace_id: namespace_id.to_ascii_lowercase().into(),
            kv_key: kv_key.into(),
            publisher: publisher.map(Arc::from),
        })
    }
}

#[derive(Clone)]
pub(crate) struct StableEntry(Arc<StableEntryInner>);

struct StableEntryInner {
    config: Option<StableEntryConfig>,
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
    published: bool,
    last_error: Option<String>,
    cleanup_failed: bool,
}

pub(crate) struct StableStatus {
    pub(crate) phase: &'static str,
    pub(crate) url: String,
    pub(crate) last_error: Option<String>,
}

impl StableEntry {
    pub(crate) fn new(config: Option<StableEntryConfig>) -> Self {
        let phase = if config.is_some() {
            StablePhase::Pending
        } else {
            StablePhase::Unconfigured
        };
        Self(Arc::new(StableEntryInner {
            config,
            projection: RwLock::new(StableProjection {
                phase,
                published: false,
                last_error: None,
                cleanup_failed: false,
            }),
            operation: Mutex::new(()),
        }))
    }

    pub(crate) async fn publish(&self, target: &str, cancellation: &CancellationToken) {
        self.apply(Some(target), cancellation).await;
    }

    pub(crate) async fn clear(&self) -> bool {
        if self.clear_confirmed() {
            return true;
        }
        self.apply(None, &CancellationToken::new()).await;
        self.clear_confirmed()
    }

    pub(crate) fn status(&self) -> StableStatus {
        let projection = self.0.projection.read();
        let url = if projection.published && matches!(projection.phase, StablePhase::Ready) {
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
            last_error: projection.last_error.clone(),
        }
    }

    fn clear_confirmed(&self) -> bool {
        let projection = self.0.projection.read();
        self.0.config.is_none()
            || matches!(projection.phase, StablePhase::Ready) && !projection.published
    }

    async fn apply(&self, target: Option<&str>, cancellation: &CancellationToken) {
        let Some(config) = self.0.config.as_ref() else {
            return;
        };
        let _operation = self.0.operation.lock().await;
        if self.0.projection.read().cleanup_failed {
            return;
        }
        {
            let mut projection = self.0.projection.write();
            projection.phase = StablePhase::Pending;
            projection.published = false;
            projection.last_error = None;
        }
        let result = run_operation(config, target, cancellation).await;
        let mut projection = self.0.projection.write();
        match result {
            Ok(()) => {
                projection.phase = StablePhase::Ready;
                projection.published = target.is_some();
                projection.last_error = None;
                projection.cleanup_failed = false;
            }
            Err(failure) => {
                projection.phase = StablePhase::Failed;
                projection.published = false;
                projection.last_error = Some(failure.message);
                projection.cleanup_failed = failure.cleanup_failed;
            }
        }
    }
}

async fn run_operation(
    config: &StableEntryConfig,
    target: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<(), StableOperationFailure> {
    let executable = config
        .publisher
        .as_deref()
        .ok_or_else(|| StableOperationFailure::safe("stable-entry publisher is unavailable"))?;
    let arguments = wrangler_arguments(config, target);
    for attempt in 0..OPERATION_ATTEMPTS {
        if cancellation.is_cancelled() {
            return Err(StableOperationFailure::safe(
                "stable-entry publication was cancelled",
            ));
        }
        match run_owned_command(
            executable,
            &arguments,
            &STABLE_ENVIRONMENT,
            cancellation,
            OPERATION_TIMEOUT,
        )
        .await
        {
            Ok(OwnedCommandOutcome::Exited(status)) if status.success() => return Ok(()),
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
            Ok(OwnedCommandOutcome::Exited(_) | OwnedCommandOutcome::TimedOut) | Err(_) => {}
        }
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
    arguments
}

fn default_kv_key() -> String {
    "target".to_owned()
}

#[cfg(test)]
#[path = "stable_entry_tests.rs"]
mod tests;
