use std::sync::Arc;

use keyring::v1::{Entry, Error as KeyringError};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Semaphore;

const SERVICE_NAME: &str = "AgentsAssemble";
const DEEPSEEK_ACCOUNT: &str = "deepseek";
const DEEPSEEK_ENVIRONMENT: &str = "DEEPSEEK_API_KEY";
const MIN_SECRET_CHARS: usize = 8;
const MAX_SECRET_CHARS: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderCredentialSource {
    Keyring,
    Environment,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProviderCredentialStatus {
    pub configured: bool,
    pub source: ProviderCredentialSource,
}

impl ProviderCredentialStatus {
    const fn from_source(source: ProviderCredentialSource) -> Self {
        Self {
            configured: !matches!(source, ProviderCredentialSource::Missing),
            source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderCredentialError {
    #[error("secure_store_unavailable")]
    SecureStoreUnavailable,
    #[error("provider_credential_invalid")]
    InvalidSecret,
}

enum BackendAvailability<T> {
    Available(T),
    Absent,
}

trait CredentialBackend: Send + Sync {
    fn configured(&self) -> Result<BackendAvailability<bool>, ProviderCredentialError>;
    fn set(&self, secret: &str) -> Result<BackendAvailability<()>, ProviderCredentialError>;
    fn delete(&self) -> Result<BackendAvailability<()>, ProviderCredentialError>;
}

struct NativeCredentialBackend;

impl CredentialBackend for NativeCredentialBackend {
    fn configured(&self) -> Result<BackendAvailability<bool>, ProviderCredentialError> {
        let BackendAvailability::Available(entry) = native_entry()? else {
            return Ok(BackendAvailability::Absent);
        };
        #[cfg(target_os = "macos")]
        let result = entry.inner.get_credential().map(|_| true);
        #[cfg(not(target_os = "macos"))]
        let result = entry.get_password().map(|secret| !secret.is_empty());
        match result {
            Ok(configured) => Ok(BackendAvailability::Available(configured)),
            Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(false)),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    fn set(&self, secret: &str) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        let BackendAvailability::Available(entry) = native_entry()? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.set_password(secret) {
            Ok(()) => Ok(BackendAvailability::Available(())),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    fn delete(&self) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        let BackendAvailability::Available(entry) = native_entry()? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(())),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }
}

fn native_entry() -> Result<BackendAvailability<Entry>, ProviderCredentialError> {
    match Entry::store_status() {
        Ok(()) => {}
        Err(KeyringError::Invalid(name, _)) if name == "platform" => {
            return Ok(BackendAvailability::Absent);
        }
        Err(_) => return Err(ProviderCredentialError::SecureStoreUnavailable),
    }
    match Entry::new(SERVICE_NAME, DEEPSEEK_ACCOUNT) {
        Ok(entry) => Ok(BackendAvailability::Available(entry)),
        Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
    }
}

#[derive(Clone)]
pub struct ProviderCredentialStore {
    backend: Arc<dyn CredentialBackend>,
    environment_secret: Arc<str>,
    access: Arc<Semaphore>,
}

impl ProviderCredentialStore {
    #[must_use]
    pub fn production() -> Self {
        let environment_secret = std::env::var(DEEPSEEK_ENVIRONMENT)
            .ok()
            .map_or_else(String::new, |value| value.trim().to_owned());
        Self {
            backend: Arc::new(NativeCredentialBackend),
            environment_secret: environment_secret.into(),
            access: Arc::new(Semaphore::new(1)),
        }
    }

    /// Returns public `DeepSeek` credential metadata without returning the secret.
    ///
    /// # Errors
    ///
    /// Returns `secure_store_unavailable` when an installed secure store fails.
    pub async fn deepseek_status(
        &self,
    ) -> Result<ProviderCredentialStatus, ProviderCredentialError> {
        match self.run_backend(|backend| backend.configured()).await? {
            BackendAvailability::Available(true) => Ok(ProviderCredentialStatus::from_source(
                ProviderCredentialSource::Keyring,
            )),
            BackendAvailability::Available(false) | BackendAvailability::Absent => Ok(
                ProviderCredentialStatus::from_source(if self.environment_secret.is_empty() {
                    ProviderCredentialSource::Missing
                } else {
                    ProviderCredentialSource::Environment
                }),
            ),
        }
    }

    /// Stores one validated `DeepSeek` credential in the platform secure store.
    ///
    /// # Errors
    ///
    /// Returns a stable validation or secure-store error without embedding the secret.
    pub async fn set_deepseek(
        &self,
        secret: &str,
    ) -> Result<ProviderCredentialStatus, ProviderCredentialError> {
        let secret = validated_secret(secret)?;
        match self
            .run_backend(move |backend| backend.set(&secret))
            .await?
        {
            BackendAvailability::Available(()) => self.deepseek_status().await,
            BackendAvailability::Absent => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    /// Deletes only the secure-store `DeepSeek` credential. An environment credential remains.
    ///
    /// # Errors
    ///
    /// Returns `secure_store_unavailable` when an installed secure store fails.
    pub async fn delete_deepseek(
        &self,
    ) -> Result<ProviderCredentialStatus, ProviderCredentialError> {
        self.run_backend(|backend| backend.delete()).await?;
        self.deepseek_status().await
    }

    async fn run_backend<T, F>(
        &self,
        operation: F,
    ) -> Result<BackendAvailability<T>, ProviderCredentialError>
    where
        T: Send + 'static,
        F: FnOnce(
                &dyn CredentialBackend,
            ) -> Result<BackendAvailability<T>, ProviderCredentialError>
            + Send
            + 'static,
    {
        let permit = Arc::clone(&self.access)
            .acquire_owned()
            .await
            .map_err(|_| ProviderCredentialError::SecureStoreUnavailable)?;
        let backend = Arc::clone(&self.backend);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation(backend.as_ref())
        })
        .await
        .map_err(|_| ProviderCredentialError::SecureStoreUnavailable)?
    }
}

fn validated_secret(value: &str) -> Result<String, ProviderCredentialError> {
    let secret = value.trim().to_owned();
    let characters = secret.chars().count();
    if !(MIN_SECRET_CHARS..=MAX_SECRET_CHARS).contains(&characters) {
        return Err(ProviderCredentialError::InvalidSecret);
    }
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        BackendAvailability, CredentialBackend, ProviderCredentialError, ProviderCredentialSource,
        ProviderCredentialStore,
    };

    #[derive(Default)]
    struct TestBackend {
        state: Mutex<TestState>,
    }

    #[derive(Default)]
    struct TestState {
        configured: bool,
        absent: bool,
        fail: bool,
        stored: String,
    }

    impl CredentialBackend for TestBackend {
        fn configured(&self) -> Result<BackendAvailability<bool>, ProviderCredentialError> {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.fail {
                return Err(ProviderCredentialError::SecureStoreUnavailable);
            }
            if state.absent {
                Ok(BackendAvailability::Absent)
            } else {
                Ok(BackendAvailability::Available(state.configured))
            }
        }

        fn set(&self, secret: &str) -> Result<BackendAvailability<()>, ProviderCredentialError> {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.fail {
                return Err(ProviderCredentialError::SecureStoreUnavailable);
            }
            if state.absent {
                return Ok(BackendAvailability::Absent);
            }
            secret.clone_into(&mut state.stored);
            state.configured = true;
            Ok(BackendAvailability::Available(()))
        }

        fn delete(&self) -> Result<BackendAvailability<()>, ProviderCredentialError> {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.fail {
                return Err(ProviderCredentialError::SecureStoreUnavailable);
            }
            if state.absent {
                return Ok(BackendAvailability::Absent);
            }
            state.stored.clear();
            state.configured = false;
            Ok(BackendAvailability::Available(()))
        }
    }

    fn store(backend: Arc<TestBackend>, environment: &str) -> ProviderCredentialStore {
        ProviderCredentialStore {
            backend,
            environment_secret: Arc::from(environment),
            access: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    #[tokio::test]
    async fn secure_store_precedes_environment_and_delete_reveals_environment() {
        let backend = Arc::new(TestBackend::default());
        let store = store(Arc::clone(&backend), "environment-secret");
        assert_eq!(
            store.deepseek_status().await.map(|status| status.source),
            Ok(ProviderCredentialSource::Environment)
        );

        let status = store
            .set_deepseek("  secure-secret  ")
            .await
            .unwrap_or_else(|error| panic!("store secret: {error}"));
        assert_eq!(status.source, ProviderCredentialSource::Keyring);
        assert_eq!(
            backend
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .stored,
            "secure-secret"
        );

        let deleted = store
            .delete_deepseek()
            .await
            .unwrap_or_else(|error| panic!("delete secret: {error}"));
        assert_eq!(deleted.source, ProviderCredentialSource::Environment);
    }

    #[tokio::test]
    async fn missing_backend_allows_environment_read_but_rejects_secure_write() {
        let backend = Arc::new(TestBackend::default());
        backend
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .absent = true;
        let store = store(backend, "environment-secret");
        assert_eq!(
            store.deepseek_status().await.map(|status| status.source),
            Ok(ProviderCredentialSource::Environment)
        );
        assert_eq!(
            store.set_deepseek("secure-secret").await,
            Err(ProviderCredentialError::SecureStoreUnavailable)
        );
    }

    #[tokio::test]
    async fn installed_store_failure_never_falls_back_or_accepts_invalid_secret() {
        let backend = Arc::new(TestBackend::default());
        backend
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fail = true;
        let store = store(backend, "environment-secret");
        assert_eq!(
            store.deepseek_status().await,
            Err(ProviderCredentialError::SecureStoreUnavailable)
        );
        assert_eq!(
            store.set_deepseek("short").await,
            Err(ProviderCredentialError::InvalidSecret)
        );
        assert_eq!(
            store.set_deepseek(&"x".repeat(8_193)).await,
            Err(ProviderCredentialError::InvalidSecret)
        );
    }
}
