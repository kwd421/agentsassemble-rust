#[cfg(unix)]
use std::path::Path;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use agentsassemble_domain::DurableAgentSession;
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    codex::CodexDriver, runtime_authority::revalidate_runtime_authority,
    runtime_lease::HeldRuntimeLease, runtime_recovery::observe_previous_runtime,
};

const DRIVER_STOP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait ProviderDriver: Send {
    fn ensure_ready(&mut self) -> DriverFuture<'_, Result<(), DriverError>>;
    fn is_alive(&mut self) -> Result<bool, DriverError>;
    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>>;
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
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverError>>;
}

struct ProductionDriverFactory {
    #[cfg(unix)]
    guardian: Option<GuardianLaunch>,
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
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverError>> {
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
                ("antigravity_live_session", "pty" | "conpty") => Err(DriverError::new(
                    "provider_runtime_unavailable",
                    "The Antigravity runtime driver is not implemented yet.",
                )),
                ("opencode_server", "http") => Err(DriverError::new(
                    "provider_runtime_unavailable",
                    "The OpenCode runtime driver is not implemented yet.",
                )),
                _ => Err(DriverError::new(
                    "invalid_runtime_profile",
                    "The stored provider runtime profile is unsupported.",
                )),
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
}

impl ProviderAdapterError {
    fn safe(error: DriverError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            effect_uncertain: false,
            runtime_handle_id: String::new(),
            runtime_owner_id: String::new(),
        }
    }

    fn uncertain(error: DriverError, handle_id: &str, owner_id: &str) -> Self {
        Self {
            code: error.code,
            message: error.message,
            effect_uncertain: true,
            runtime_handle_id: handle_id.to_owned(),
            runtime_owner_id: owner_id.to_owned(),
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
    Running(OwnedRuntime),
    StopConfirmed {
        handle_id: String,
        owner_id: String,
        runtime_lease: HeldRuntimeLease,
    },
}

struct RuntimeSlot {
    state: RuntimeState,
}

struct OwnedRuntime {
    handle_id: String,
    owner_id: String,
    profile_key: String,
    driver: Box<dyn ProviderDriver>,
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

    /// Starts or proves one exact session runtime through its transport driver.
    ///
    /// # Errors
    ///
    /// Returns a redacted fail-closed runtime error.
    pub async fn start(
        &self,
        session: &DurableAgentSession,
    ) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
        let slot = self.slot(session).await;
        let mut slot = slot.lock().await;
        match &mut slot.state {
            RuntimeState::Running(runtime) => reuse_owned_runtime(session, runtime).await,
            RuntimeState::StopConfirmed { .. } => {
                Err(ProviderAdapterError::safe(DriverError::new(
                    "operation_in_progress",
                    "A confirmed provider stop is awaiting its durable checkpoint.",
                )))
            }
            RuntimeState::Vacant => {
                if !session.runtime_handle_id.is_empty() || !session.runtime_owner_id.is_empty() {
                    return Err(ProviderAdapterError {
                        code: "runtime_owner_mismatch",
                        message: "The durable provider handle is not owned by this supervisor.",
                        effect_uncertain: true,
                        runtime_handle_id: session.runtime_handle_id.clone(),
                        runtime_owner_id: session.runtime_owner_id.clone(),
                    });
                }
                revalidate_runtime_authority(session)
                    .await
                    .map_err(ProviderAdapterError::safe)?;
                let runtime_lease =
                    HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)
                        .map_err(|_| {
                            ProviderAdapterError::safe(DriverError::new(
                                "provider_custody_unavailable",
                                "The provider runtime lease could not be established.",
                            ))
                        })?;
                let handle_id = format!("runtime-v3-{}", Uuid::new_v4());
                let driver = match self.owner.factory.launch(session, &runtime_lease).await {
                    Ok(driver) => driver,
                    Err(error) => {
                        runtime_lease.cleanup_pre_effect();
                        return Err(ProviderAdapterError::safe(error));
                    }
                };
                slot.state = RuntimeState::Running(OwnedRuntime {
                    handle_id,
                    owner_id: self.owner.supervisor_id.clone(),
                    profile_key: session.runtime_profile_key.clone(),
                    driver,
                    runtime_lease: Some(runtime_lease),
                });
                let RuntimeState::Running(runtime) = &mut slot.state else {
                    unreachable!("new provider runtime slot must be running");
                };
                initialize_owned_runtime(session, runtime).await
            }
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
                if let Err(error) = runtime.driver.is_alive() {
                    return Err(ProviderAdapterError::uncertain(error, handle_id, owner_id));
                }
                if let Err(error) = runtime.driver.stop().await {
                    return Err(ProviderAdapterError::uncertain(error, handle_id, owner_id));
                }
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

    pub async fn observe(&self, session: &DurableAgentSession) -> ProviderRuntimeObservation {
        let Some(slot) = self
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
        else {
            return observe_previous_runtime(session).await;
        };
        let mut slot = slot.lock().await;
        if let RuntimeState::StopConfirmed {
            handle_id,
            owner_id,
            ..
        } = &slot.state
        {
            return if handle_id == &session.runtime_handle_id
                && owner_id == &session.runtime_owner_id
            {
                ProviderRuntimeObservation::Gone
            } else {
                ProviderRuntimeObservation::Ambiguous {
                    reason_code: "runtime_identity_mismatch".to_owned(),
                }
            };
        }
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return ProviderRuntimeObservation::Gone;
        };
        if runtime.handle_id != session.runtime_handle_id
            || runtime.profile_key != session.runtime_profile_key
        {
            return ProviderRuntimeObservation::Ambiguous {
                reason_code: "runtime_identity_mismatch".to_owned(),
            };
        }
        match runtime.driver.is_alive() {
            Ok(true) => {}
            Ok(false) => {
                return ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: runtime.handle_id.clone(),
                    owner_id: runtime.owner_id.clone(),
                    reason_code: "provider_leader_exited".to_owned(),
                };
            }
            Err(_) => {
                return ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: runtime.handle_id.clone(),
                    owner_id: runtime.owner_id.clone(),
                    reason_code: "runtime_health_unknown".to_owned(),
                };
            }
        }
        if let Err(error) = revalidate_runtime_authority(session).await {
            return ProviderRuntimeObservation::LeaseUncertain {
                handle_id: runtime.handle_id.clone(),
                owner_id: runtime.owner_id.clone(),
                reason_code: error.code.to_owned(),
            };
        }
        ProviderRuntimeObservation::Adopted {
            handle_id: runtime.handle_id.clone(),
            previous_owner_id: session.runtime_owner_id.clone(),
            new_owner_id: runtime.owner_id.clone(),
            runtime_profile_key: runtime.profile_key.clone(),
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
            if let RuntimeState::Running(runtime) = &mut slot.state {
                let stopped =
                    tokio::time::timeout(DRIVER_STOP_TIMEOUT, runtime.driver.stop()).await;
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

fn started(
    session: &DurableAgentSession,
    runtime: &OwnedRuntime,
    runtime_reused: bool,
) -> ProviderRuntimeStarted {
    ProviderRuntimeStarted {
        runtime_handle_id: runtime.handle_id.clone(),
        runtime_owner_id: runtime.owner_id.clone(),
        provider_session_id: session.provider_session_id.clone(),
        runtime_reused,
        provider_session_reused: false,
        provider_session_active: false,
    }
}

async fn reuse_owned_runtime(
    session: &DurableAgentSession,
    runtime: &mut OwnedRuntime,
) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
    validate_owned_runtime(session, runtime)?;
    match runtime.driver.is_alive() {
        Ok(true) => {}
        Ok(false) => {
            return Err(ProviderAdapterError::uncertain(
                DriverError::new(
                    "provider_runtime_exited",
                    "The owned provider runtime exited before it became ready.",
                ),
                &runtime.handle_id,
                &runtime.owner_id,
            ));
        }
        Err(error) => {
            return Err(ProviderAdapterError::uncertain(
                error,
                &runtime.handle_id,
                &runtime.owner_id,
            ));
        }
    }
    if let Err(error) = revalidate_runtime_authority(session).await {
        return Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        ));
    }
    match runtime.driver.ensure_ready().await {
        Ok(()) => Ok(started(session, runtime, true)),
        Err(error) => Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        )),
    }
}

async fn initialize_owned_runtime(
    session: &DurableAgentSession,
    runtime: &mut OwnedRuntime,
) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
    match runtime.driver.ensure_ready().await {
        Ok(()) => Ok(started(session, runtime, false)),
        Err(error) => Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        )),
    }
}

fn validate_owned_runtime(
    session: &DurableAgentSession,
    runtime: &OwnedRuntime,
) -> Result<(), ProviderAdapterError> {
    let durable_handle_matches =
        session.runtime_handle_id.is_empty() || session.runtime_handle_id == runtime.handle_id;
    let durable_owner_matches =
        session.runtime_owner_id.is_empty() || session.runtime_owner_id == runtime.owner_id;
    if runtime.profile_key != session.runtime_profile_key
        || !durable_handle_matches
        || !durable_owner_matches
    {
        return Err(ProviderAdapterError::uncertain(
            DriverError::new(
                "runtime_owner_mismatch",
                "The provider runtime does not match the durable session authority.",
            ),
            &runtime.handle_id,
            &runtime.owner_id,
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "runtime_tests.rs"]
mod tests;
