use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    codex::CodexDriver,
    filesystem::{canonical_workspace, executable_identity},
    profile::runtime_profile_key,
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
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverError>>;
}

struct ProductionDriverFactory;

impl DriverFactory for ProductionDriverFactory {
    fn launch<'a>(
        &'a self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverError>> {
        Box::pin(async move {
            match (
                session.public.provider_kind.as_str(),
                session.public.transport.as_str(),
            ) {
                ("codex_live_session", "stdio_jsonl") => {
                    Ok(Box::new(CodexDriver::spawn(session).await?) as Box<dyn ProviderDriver>)
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
pub enum ProviderRuntimeObservation {
    Adopted {
        handle_id: String,
        previous_owner_id: String,
        new_owner_id: String,
        runtime_profile_key: String,
        provider_session_active: bool,
    },
    Gone,
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
    StopConfirmed { handle_id: String, owner_id: String },
}

struct RuntimeSlot {
    state: RuntimeState,
}

struct OwnedRuntime {
    handle_id: String,
    owner_id: String,
    profile_key: String,
    driver: Box<dyn ProviderDriver>,
}

impl ProviderAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_factory(Arc::new(ProductionDriverFactory))
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
            RuntimeState::Running(runtime) => {
                let (result, vacated) = reuse_owned_runtime(session, runtime).await;
                if vacated {
                    slot.state = RuntimeState::Vacant;
                }
                result
            }
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
                let driver = self
                    .owner
                    .factory
                    .launch(session)
                    .await
                    .map_err(ProviderAdapterError::safe)?;
                let mut runtime = OwnedRuntime {
                    handle_id: format!("runtime-v1-{}", Uuid::new_v4()),
                    owner_id: self.owner.supervisor_id.clone(),
                    profile_key: session.runtime_profile_key.clone(),
                    driver,
                };
                match runtime.driver.ensure_ready().await {
                    Ok(()) => {
                        let result = started(session, &runtime, false);
                        slot.state = RuntimeState::Running(runtime);
                        Ok(result)
                    }
                    Err(error) => {
                        let alive = runtime.driver.is_alive().unwrap_or(true);
                        if alive {
                            let failure = ProviderAdapterError::uncertain(
                                error,
                                &runtime.handle_id,
                                &runtime.owner_id,
                            );
                            slot.state = RuntimeState::Running(runtime);
                            Err(failure)
                        } else {
                            Err(ProviderAdapterError::safe(error))
                        }
                    }
                }
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
                match runtime.driver.is_alive() {
                    Ok(false) => {
                        slot.state = RuntimeState::StopConfirmed {
                            handle_id: handle_id.to_owned(),
                            owner_id: owner_id.to_owned(),
                        };
                        return Ok(());
                    }
                    Ok(true) => {}
                    Err(error) => {
                        return Err(ProviderAdapterError::uncertain(error, handle_id, owner_id));
                    }
                }
                if let Err(error) = runtime.driver.stop().await {
                    return Err(ProviderAdapterError::uncertain(error, handle_id, owner_id));
                }
                slot.state = RuntimeState::StopConfirmed {
                    handle_id: handle_id.to_owned(),
                    owner_id: owner_id.to_owned(),
                };
                Ok(())
            }
            RuntimeState::StopConfirmed {
                handle_id: confirmed_handle,
                owner_id: confirmed_owner,
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
            } if confirmed_handle == handle_id && confirmed_owner == owner_id
        ) {
            slot.state = RuntimeState::Vacant;
        }
    }

    pub async fn observe(&self, session: &DurableAgentSession) -> ProviderRuntimeObservation {
        let Some(slot) = self
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
        else {
            return ProviderRuntimeObservation::Ambiguous {
                reason_code: "runtime_not_owned".to_owned(),
            };
        };
        let mut slot = slot.lock().await;
        if let RuntimeState::StopConfirmed {
            handle_id,
            owner_id,
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
            Ok(true) => ProviderRuntimeObservation::Adopted {
                handle_id: runtime.handle_id.clone(),
                previous_owner_id: session.runtime_owner_id.clone(),
                new_owner_id: runtime.owner_id.clone(),
                runtime_profile_key: runtime.profile_key.clone(),
                provider_session_active: session.public.provider_session_active,
            },
            Ok(false) => ProviderRuntimeObservation::Gone,
            Err(_) => ProviderRuntimeObservation::Ambiguous {
                reason_code: "runtime_health_unknown".to_owned(),
            },
        }
    }

    /// Stops and reaps every runtime created by this adapter owner.
    ///
    /// # Errors
    ///
    /// Returns when one or more owned processes could not be confirmed stopped.
    pub async fn shutdown(&self) -> Result<(), ProviderAdapterError> {
        let slots = self
            .owner
            .runtimes
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut failure = None;
        for slot in slots {
            let mut slot = slot.lock().await;
            if let RuntimeState::Running(runtime) = &mut slot.state {
                let stopped =
                    tokio::time::timeout(DRIVER_STOP_TIMEOUT, runtime.driver.stop()).await;
                match stopped {
                    Ok(Ok(())) => slot.state = RuntimeState::Vacant,
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
            }
        }
        failure.map_or(Ok(()), Err)
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
) -> (Result<ProviderRuntimeStarted, ProviderAdapterError>, bool) {
    if let Err(error) = validate_owned_runtime(session, runtime) {
        return (Err(error), false);
    }
    match runtime.driver.is_alive() {
        Ok(true) => {}
        Ok(false) => {
            return (
                Err(ProviderAdapterError::safe(DriverError::new(
                    "provider_runtime_exited",
                    "The owned provider runtime exited before it became ready.",
                ))),
                true,
            );
        }
        Err(error) => {
            return (
                Err(ProviderAdapterError::uncertain(
                    error,
                    &runtime.handle_id,
                    &runtime.owner_id,
                )),
                false,
            );
        }
    }
    if let Err(error) = revalidate_runtime_authority(session).await {
        return (
            Err(ProviderAdapterError::uncertain(
                error,
                &runtime.handle_id,
                &runtime.owner_id,
            )),
            false,
        );
    }
    match runtime.driver.ensure_ready().await {
        Ok(()) => (Ok(started(session, runtime, true)), false),
        Err(error) if matches!(runtime.driver.is_alive(), Ok(false)) => {
            (Err(ProviderAdapterError::safe(error)), true)
        }
        Err(error) => (
            Err(ProviderAdapterError::uncertain(
                error,
                &runtime.handle_id,
                &runtime.owner_id,
            )),
            false,
        ),
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

async fn revalidate_runtime_authority(session: &DurableAgentSession) -> Result<(), DriverError> {
    if session.runtime_profile_version != CURRENT_RUNTIME_PROFILE_VERSION {
        return Err(DriverError::new(
            "profile_migration_required",
            "The provider runtime profile version is unsupported.",
        ));
    }
    let expected_profile_key = runtime_profile_key([
        session.public.provider_kind.as_str(),
        session.public.runtime_kind.as_str(),
        session.executable.as_str(),
        session.executable_identity.as_str(),
        session.workspace.as_str(),
        session.workspace_identity.as_str(),
        session.public.model.as_str(),
        session.public.reasoning_effort.as_str(),
        session.public.service_tier.as_str(),
        session.public.variant.as_str(),
        session.public.execution_harness.as_str(),
        session.public.permission_mode.as_str(),
        session.public.transport.as_str(),
    ]);
    if expected_profile_key != session.runtime_profile_key {
        return Err(DriverError::new(
            "runtime_profile_changed",
            "The provider runtime profile no longer matches its durable identity.",
        ));
    }
    let workspace = canonical_workspace(session.workspace.clone())
        .await
        .map_err(|_| {
            DriverError::new(
                "workspace_authority_changed",
                "The provider workspace authority could not be revalidated.",
            )
        })?;
    if workspace.0 != session.workspace || workspace.1 != session.workspace_identity {
        return Err(DriverError::new(
            "workspace_authority_changed",
            "The provider workspace authority changed after selection.",
        ));
    }
    let executable = executable_identity(session.executable.clone())
        .await
        .map_err(|_| {
            DriverError::new(
                "executable_authority_changed",
                "The provider executable authority could not be revalidated.",
            )
        })?;
    if executable != session.executable_identity {
        return Err(DriverError::new(
            "executable_authority_changed",
            "The provider executable authority changed after selection.",
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "runtime_tests.rs"]
mod tests;
