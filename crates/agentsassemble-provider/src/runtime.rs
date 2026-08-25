#[cfg(unix)]
use std::path::Path;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use agentsassemble_domain::DurableAgentSession;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[cfg(any(unix, windows))]
use crate::antigravity::AntigravityDriver;
#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    codex::CodexDriver, launch_error::DriverLaunchError, opencode::OpenCodeDriver,
    room_portal::ProviderTurnOutcome, runtime_authority::revalidate_runtime_authority,
    runtime_lease::HeldRuntimeLease,
};

const DRIVER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub(crate) trait ProviderDriver: Send {
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>>;
    fn send_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>>;
    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>>;
    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>>;
    fn begin_room_observation(
        &mut self,
        _request: &ProviderTurnRequest,
    ) -> Result<(), DriverError> {
        Err(DriverError::new(
            "room_portal_unavailable",
            "The provider runtime has no server-owned room portal.",
        ))
    }
    fn finish_room_observation(
        &mut self,
        _request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        Err(DriverError::new(
            "room_portal_unavailable",
            "The provider runtime has no server-owned room portal.",
        ))
    }
    fn abort_room_observation(&mut self) {}
    fn requires_restart(&self) -> bool {
        false
    }
    fn attachment_replay_is_safe(&self) -> bool {
        true
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderSessionAttachment {
    pub(crate) provider_session_id: String,
    pub(crate) reused: bool,
    pub(crate) observed_model_id: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{message}")]
pub(crate) struct DriverError {
    pub code: &'static str,
    pub message: &'static str,
}
impl DriverError {
    pub(crate) const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}
trait DriverFactory: Send + Sync {
    fn launch<'a>(
        &'a self,
        session: &'a DurableAgentSession,
        _runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>>;
}
struct ProductionDriverFactory {
    #[cfg(unix)]
    guardian: Option<GuardianLaunch>,
    #[cfg(windows)]
    companion: Option<Arc<crate::filesystem::BoundExecutable>>,
}

impl ProductionDriverFactory {
    fn local() -> Self {
        #[cfg(all(unix, test))]
        let guardian = GuardianLaunch::test_harness().ok();
        #[cfg(all(unix, not(test), any(target_os = "linux", target_os = "android")))]
        let guardian = crate::guardian::reexecution_path()
            .ok()
            .and_then(|executable| GuardianLaunch::production(&executable).ok());
        #[cfg(all(unix, not(test), not(any(target_os = "linux", target_os = "android"))))]
        let guardian = (std::env::var_os("AGENTSASSEMBLE_INTERNAL_SERVER_STAGED")
            == Some("v1".into()))
        .then(crate::guardian::reexecution_path)
        .and_then(Result::ok)
        .and_then(|executable| GuardianLaunch::production(&executable).ok());
        Self {
            #[cfg(unix)]
            guardian,
            #[cfg(windows)]
            companion: crate::filesystem::bind_current_helper_executable()
                .ok()
                .map(Arc::new),
        }
    }

    #[cfg(unix)]
    fn with_guardian(executable: &Path) -> Self {
        Self {
            guardian: GuardianLaunch::production(executable).ok(),
        }
    }
}

impl DriverFactory for ProductionDriverFactory {
    fn launch<'a>(
        &'a self,
        session: &'a DurableAgentSession,
        runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
        #[cfg(not(unix))]
        let _ = runtime_lease;
        Box::pin(async move {
            match (
                session.public.provider_kind.as_str(),
                session.public.transport.as_str(),
            ) {
                ("codex_live_session", "stdio_jsonl") => {
                    #[cfg(unix)]
                    let driver = CodexDriver::spawn(
                        session,
                        runtime_lease,
                        self.guardian.as_ref().ok_or_else(|| {
                            DriverError::new(
                                "provider_custody_unavailable",
                                "The provider process custody helper is unavailable.",
                            )
                        })?,
                    )
                    .await?;
                    #[cfg(not(unix))]
                    let driver = CodexDriver::spawn(session).await?;
                    Ok(Box::new(driver) as Box<dyn ProviderDriver>)
                }
                ("antigravity_live_session", "pty") => {
                    #[cfg(unix)]
                    {
                        let driver = AntigravityDriver::spawn(
                            session,
                            runtime_lease,
                            self.guardian.as_ref().ok_or_else(|| {
                                DriverError::new(
                                    "provider_custody_unavailable",
                                    "The provider process custody helper is unavailable.",
                                )
                            })?,
                        )
                        .await?;
                        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
                    }
                    #[cfg(not(unix))]
                    Err(DriverError::new(
                        "provider_runtime_unsupported",
                        "PTY provider sessions are unsupported on this platform.",
                    )
                    .into())
                }
                ("antigravity_live_session", "conpty") => {
                    #[cfg(windows)]
                    {
                        let driver = AntigravityDriver::spawn(
                            session,
                            self.companion.as_deref().ok_or_else(|| {
                                DriverError::new(
                                    "provider_custody_unavailable",
                                    "The private provider companion is unavailable.",
                                )
                            })?,
                        )
                        .await?;
                        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
                    }
                    #[cfg(not(windows))]
                    Err(DriverError::new(
                        "provider_runtime_unsupported",
                        "ConPTY provider sessions are unsupported on this platform.",
                    )
                    .into())
                }
                ("opencode_server", "http") => {
                    #[cfg(unix)]
                    let driver = OpenCodeDriver::spawn(
                        session,
                        runtime_lease,
                        self.guardian.as_ref().ok_or_else(|| {
                            DriverError::new(
                                "provider_custody_unavailable",
                                "The provider process custody helper is unavailable.",
                            )
                        })?,
                    )
                    .await?;
                    #[cfg(not(unix))]
                    let driver = OpenCodeDriver::spawn(session).await?;
                    Ok(Box::new(driver) as Box<dyn ProviderDriver>)
                }
                _ => Err(DriverError::new(
                    "invalid_runtime_profile",
                    "The stored provider runtime profile is unsupported.",
                )
                .into()),
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRuntimeStarted {
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub provider_session_id: String,
    pub runtime_reused: bool,
    pub provider_session_reused: bool,
    pub provider_session_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStartReservation {
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRuntimeGone {
    pub room_id: String,
    pub session_id: String,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderShutdownOutcome {
    pub gone: Vec<ProviderRuntimeGone>,
    pub failure: Option<ProviderAdapterError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRuntimeObservation {
    Adopted {
        handle_id: String,
        previous_owner_id: String,
        new_owner_id: String,
        runtime_profile_key: String,
    },
    Gone,
    LeaseUncertain {
        handle_id: String,
        owner_id: String,
        reason_code: String,
    },
    Ambiguous {
        reason_code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ProviderAdapterError {
    pub code: &'static str,
    pub message: &'static str,
    pub effect_uncertain: bool,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_stopped: bool,
}

impl ProviderAdapterError {
    fn safe(error: DriverError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            effect_uncertain: false,
            runtime_handle_id: String::new(),
            runtime_owner_id: String::new(),
            runtime_stopped: false,
        }
    }

    fn uncertain(error: DriverError, handle_id: &str, owner_id: &str) -> Self {
        Self {
            code: error.code,
            message: error.message,
            effect_uncertain: true,
            runtime_handle_id: handle_id.to_owned(),
            runtime_owner_id: owner_id.to_owned(),
            runtime_stopped: false,
        }
    }

    fn confirmed_stopped(error: DriverError, handle_id: &str, owner_id: &str) -> Self {
        Self {
            code: error.code,
            message: error.message,
            effect_uncertain: false,
            runtime_handle_id: handle_id.to_owned(),
            runtime_owner_id: owner_id.to_owned(),
            runtime_stopped: true,
        }
    }
}

#[derive(Clone)]
pub struct ProviderAdapter {
    owner: Arc<AdapterOwner>,
}

struct AdapterOwner {
    supervisor_id: String,
    runtimes: Mutex<HashMap<RuntimeKey, Arc<Mutex<RuntimeSlot>>>>,
    factory: Arc<dyn DriverFactory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuntimeKey {
    room_id: String,
    session_id: String,
}

enum RuntimeState {
    Vacant,
    Launching(LaunchingRuntime),
    Running(OwnedRuntime),
    StopConfirmed {
        handle_id: String,
        owner_id: String,
        runtime_lease: HeldRuntimeLease,
    },
}

struct LaunchingRuntime {
    handle_id: String,
    owner_id: String,
    profile_key: String,
    effect_started: bool,
    runtime_lease: HeldRuntimeLease,
}

struct RuntimeSlot {
    state: RuntimeState,
}

struct OwnedRuntime {
    handle_id: String,
    owner_id: String,
    profile_key: String,
    driver: Arc<Mutex<Box<dyn ProviderDriver>>>,
    turn_cancellation: CancellationToken,
    runtime_lease: Option<HeldRuntimeLease>,
}

impl ProviderAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_factory(Arc::new(ProductionDriverFactory::local()))
    }

    /// Builds an adapter whose Unix custody helpers re-execute an exact host binary.
    #[doc(hidden)]
    #[cfg(unix)]
    #[must_use]
    pub fn with_guardian_executable(executable: &Path) -> Self {
        Self::with_factory(Arc::new(ProductionDriverFactory::with_guardian(executable)))
    }

    fn with_factory(factory: Arc<dyn DriverFactory>) -> Self {
        Self {
            owner: Arc::new(AdapterOwner {
                supervisor_id: format!("supervisor-v1-{}", Uuid::new_v4()),
                runtimes: Mutex::new(HashMap::new()),
                factory,
            }),
        }
    }

    /// Stops only the exact handle and supervisor owner in a durable stop lease.
    ///
    /// # Errors
    ///
    /// Any mismatch or unconfirmed shutdown is returned without claiming success.
    pub async fn stop(
        &self,
        room_id: &str,
        session_id: &str,
        handle_id: &str,
        owner_id: &str,
    ) -> Result<(), ProviderAdapterError> {
        let slot = self
            .existing_slot(room_id, session_id)
            .await
            .ok_or_else(|| {
                ProviderAdapterError::uncertain(
                    DriverError::new(
                        "runtime_owner_mismatch",
                        "The provider runtime is not owned by this supervisor.",
                    ),
                    handle_id,
                    owner_id,
                )
            })?;
        let mut slot = slot.lock().await;
        match &mut slot.state {
            RuntimeState::Running(runtime)
                if runtime.handle_id == handle_id && runtime.owner_id == owner_id =>
            {
                runtime.turn_cancellation.cancel();
                let mut driver = runtime.driver.lock().await;
                if let Err(error) = driver.stop().await {
                    return Err(ProviderAdapterError::uncertain(error, handle_id, owner_id));
                }
                drop(driver);
                let Some(runtime_lease) = runtime.runtime_lease.take() else {
                    return Err(ProviderAdapterError::uncertain(
                        DriverError::new(
                            "provider_custody_unavailable",
                            "The provider runtime lease is unavailable.",
                        ),
                        handle_id,
                        owner_id,
                    ));
                };
                slot.state = RuntimeState::StopConfirmed {
                    handle_id: handle_id.to_owned(),
                    owner_id: owner_id.to_owned(),
                    runtime_lease,
                };
                Ok(())
            }
            RuntimeState::StopConfirmed {
                handle_id: confirmed_handle,
                owner_id: confirmed_owner,
                ..
            } if confirmed_handle == handle_id && confirmed_owner == owner_id => Ok(()),
            _ => Err(ProviderAdapterError::uncertain(
                DriverError::new(
                    "runtime_owner_mismatch",
                    "The provider stop lease does not match the owned runtime.",
                ),
                handle_id,
                owner_id,
            )),
        }
    }

    pub async fn release_confirmed_stop(
        &self,
        room_id: &str,
        session_id: &str,
        handle_id: &str,
        owner_id: &str,
    ) {
        let Some(slot) = self.existing_slot(room_id, session_id).await else {
            return;
        };
        let mut slot = slot.lock().await;
        if matches!(
            &slot.state,
            RuntimeState::StopConfirmed {
                handle_id: confirmed_handle,
                owner_id: confirmed_owner,
                runtime_lease,
            } if confirmed_handle == handle_id && confirmed_owner == owner_id
        ) {
            let RuntimeState::StopConfirmed { runtime_lease, .. } = &mut slot.state else {
                unreachable!("confirmed provider stop changed while locked");
            };
            runtime_lease.release_and_remove();
            slot.state = RuntimeState::Vacant;
        }
    }

    /// Stops and reaps every runtime created by this adapter owner.
    ///
    /// # Errors
    ///
    /// Returns when one or more owned processes could not be confirmed stopped.
    pub async fn shutdown(&self) -> Result<(), ProviderAdapterError> {
        let outcome = self.shutdown_with_observations().await;
        self.release_shutdown_observations(&outcome.gone).await;
        outcome.failure.map_or(Ok(()), Err)
    }

    /// Stops every owned runtime and returns the exact durable sessions now proven gone.
    ///
    /// A best-effort failure is returned beside every successful stop observation so the
    /// caller can checkpoint proven absence even when another runtime remains uncertain.
    pub async fn shutdown_with_observations(&self) -> ProviderShutdownOutcome {
        let slots = self
            .owner
            .runtimes
            .lock()
            .await
            .iter()
            .map(|(key, slot)| (key.clone(), Arc::clone(slot)))
            .collect::<Vec<_>>();
        let mut failure = None;
        let mut gone = Vec::new();
        for (key, slot) in slots {
            let mut slot = slot.lock().await;
            if let Some(result) = observation::shutdown_launching_runtime(&key, &mut slot) {
                match result {
                    Ok(stopped) => gone.push(stopped),
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
                continue;
            }
            if let RuntimeState::Running(runtime) = &mut slot.state {
                runtime.turn_cancellation.cancel();
                let driver = Arc::clone(&runtime.driver);
                let stopped = tokio::time::timeout(DRIVER_STOP_TIMEOUT, async move {
                    driver.lock().await.stop().await
                })
                .await;
                match stopped {
                    Ok(Ok(())) => {
                        let Some(runtime_lease) = runtime.runtime_lease.take() else {
                            failure.get_or_insert_with(|| {
                                ProviderAdapterError::uncertain(
                                    DriverError::new(
                                        "provider_custody_unavailable",
                                        "The provider runtime lease is unavailable.",
                                    ),
                                    &runtime.handle_id,
                                    &runtime.owner_id,
                                )
                            });
                            continue;
                        };
                        gone.push(ProviderRuntimeGone {
                            room_id: key.room_id,
                            session_id: key.session_id,
                            runtime_handle_id: runtime.handle_id.clone(),
                            runtime_owner_id: runtime.owner_id.clone(),
                            runtime_lease_token: runtime_lease.token().to_owned(),
                        });
                        slot.state = RuntimeState::StopConfirmed {
                            handle_id: runtime.handle_id.clone(),
                            owner_id: runtime.owner_id.clone(),
                            runtime_lease,
                        };
                    }
                    Ok(Err(error)) => {
                        failure.get_or_insert_with(|| {
                            ProviderAdapterError::uncertain(
                                error,
                                &runtime.handle_id,
                                &runtime.owner_id,
                            )
                        });
                    }
                    Err(_) => {
                        failure.get_or_insert_with(|| {
                            ProviderAdapterError::uncertain(
                                DriverError::new(
                                    "provider_shutdown_timeout",
                                    "An owned provider runtime exceeded the shutdown deadline.",
                                ),
                                &runtime.handle_id,
                                &runtime.owner_id,
                            )
                        });
                    }
                }
            } else if let RuntimeState::StopConfirmed {
                handle_id,
                owner_id,
                runtime_lease,
            } = &slot.state
            {
                gone.push(ProviderRuntimeGone {
                    room_id: key.room_id,
                    session_id: key.session_id,
                    runtime_handle_id: handle_id.clone(),
                    runtime_owner_id: owner_id.clone(),
                    runtime_lease_token: runtime_lease.token().to_owned(),
                });
            }
        }
        ProviderShutdownOutcome { gone, failure }
    }

    pub async fn release_shutdown_observations(&self, gone: &[ProviderRuntimeGone]) {
        for stopped in gone {
            let Some(slot) = self
                .existing_slot(&stopped.room_id, &stopped.session_id)
                .await
            else {
                continue;
            };
            let mut slot = slot.lock().await;
            let RuntimeState::StopConfirmed {
                handle_id,
                owner_id,
                runtime_lease,
            } = &mut slot.state
            else {
                continue;
            };
            if handle_id != &stopped.runtime_handle_id
                || owner_id != &stopped.runtime_owner_id
                || runtime_lease.token() != stopped.runtime_lease_token
            {
                continue;
            }
            runtime_lease.release_and_remove();
            slot.state = RuntimeState::Vacant;
        }
    }

    async fn slot(&self, session: &DurableAgentSession) -> Arc<Mutex<RuntimeSlot>> {
        let key = RuntimeKey {
            room_id: session.public.room_id.clone(),
            session_id: session.public.session_id.clone(),
        };
        let mut runtimes = self.owner.runtimes.lock().await;
        runtimes
            .entry(key)
            .or_insert_with(|| {
                Arc::new(Mutex::new(RuntimeSlot {
                    state: RuntimeState::Vacant,
                }))
            })
            .clone()
    }

    async fn existing_slot(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Option<Arc<Mutex<RuntimeSlot>>> {
        self.owner
            .runtimes
            .lock()
            .await
            .get(&RuntimeKey {
                room_id: room_id.to_owned(),
                session_id: session_id.to_owned(),
            })
            .cloned()
    }
}

impl Default for ProviderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, unix))]
#[path = "runtime_launch_tests.rs"]
mod launch_tests;
#[path = "runtime_observation.rs"]
mod observation;
#[cfg(all(test, unix))]
#[path = "runtime_provider_session_tests.rs"]
mod provider_session_tests;
#[cfg(all(test, unix))]
#[path = "runtime_provider_turn_tests.rs"]
mod provider_turn_tests;
#[cfg(all(test, unix))]
#[path = "runtime_test_cleanup.rs"]
mod test_cleanup;
#[cfg(all(test, unix))]
#[path = "runtime_tests.rs"]
mod tests;

#[path = "runtime_launch_state.rs"]
mod launch_state;
#[path = "runtime_start.rs"]
mod start;
#[path = "runtime_start_authority.rs"]
mod start_authority;
use start::validate_owned_runtime;
use start::{initialize_owned_runtime, reuse_owned_runtime};

#[path = "runtime_turn.rs"]
mod turn;
pub use turn::{ProviderRoomObservation, ProviderTurnCompleted, ProviderTurnRequest};
