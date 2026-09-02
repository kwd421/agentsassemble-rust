use std::sync::Arc;

use keyring::v1::{Entry, Error as KeyringError};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::driver::DriverError;

#[cfg(target_os = "macos")]
use security_framework::item::{ItemClass, ItemSearchOptions};
#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::{
    KeychainUserInteractionLock, SecKeychain, SecPreferencesDomain,
};
#[cfg(target_os = "macos")]
use security_framework_sys::base::errSecItemNotFound;

const SERVICE_NAME: &str = "AgentsAssemble";
const DEEPSEEK_ACCOUNT: &str = "deepseek";
const MIN_SECRET_CHARS: usize = 8;
const MAX_SECRET_CHARS: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderCredentialSource {
    Keyring,
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
    #[error("provider_credential_missing")]
    MissingSecret,
    #[error("provider_credential_invalid")]
    InvalidSecret,
}

enum BackendAvailability<T> {
    Available(T),
    Absent,
}

trait CredentialBackend: Send + Sync {
    fn configured(&self) -> Result<BackendAvailability<bool>, ProviderCredentialError>;
    fn read(&self) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError>;
    fn set(&self, secret: &str) -> Result<BackendAvailability<()>, ProviderCredentialError>;
    fn delete(&self) -> Result<BackendAvailability<()>, ProviderCredentialError>;
}

struct NativeCredentialBackend;

impl CredentialBackend for NativeCredentialBackend {
    fn configured(&self) -> Result<BackendAvailability<bool>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        {
            let BackendAvailability::Available(()) = native_store_available()? else {
                return Ok(BackendAvailability::Absent);
            };
            match macos_keyring_item_exists(SERVICE_NAME, DEEPSEEK_ACCOUNT) {
                Ok(configured) => Ok(BackendAvailability::Available(configured)),
                Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let BackendAvailability::Available(entry) = native_entry()? else {
                return Ok(BackendAvailability::Absent);
            };
            match entry.get_password() {
                Ok(secret) => Ok(BackendAvailability::Available(!secret.is_empty())),
                Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(false)),
                Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
            }
        }
    }

    fn read(&self) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        let _interaction = macos_disable_keychain_ui()?;
        let BackendAvailability::Available(entry) = native_entry()? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.get_password() {
            Ok(secret) => Ok(BackendAvailability::Available(Some(secret))),
            Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(None)),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    fn set(&self, secret: &str) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        let _interaction = macos_disable_keychain_ui()?;
        let BackendAvailability::Available(entry) = native_entry()? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.set_password(secret) {
            Ok(()) => Ok(BackendAvailability::Available(())),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    fn delete(&self) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        let _interaction = macos_disable_keychain_ui()?;
        let BackendAvailability::Available(entry) = native_entry()? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(())),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_keyring_item_exists(
    service: &str,
    account: &str,
) -> security_framework::base::Result<bool> {
    let keychain = SecKeychain::default_for_domain(SecPreferencesDomain::User)?;
    let keychains = [keychain];
    let mut query = ItemSearchOptions::new();
    query
        .keychains(&keychains)
        .class(ItemClass::generic_password())
        .service(service)
        .account(account)
        .fail_on_authentication_ui(true);
    macos_item_exists_from_search(query.search().map(|_| ()))
}

#[cfg(target_os = "macos")]
fn macos_disable_keychain_ui() -> Result<KeychainUserInteractionLock, ProviderCredentialError> {
    SecKeychain::disable_user_interaction()
        .map_err(|_| ProviderCredentialError::SecureStoreUnavailable)
}

#[cfg(target_os = "macos")]
fn macos_item_exists_from_search(
    result: security_framework::base::Result<()>,
) -> security_framework::base::Result<bool> {
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.code() == errSecItemNotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn native_entry() -> Result<BackendAvailability<Entry>, ProviderCredentialError> {
    let BackendAvailability::Available(()) = native_store_available()? else {
        return Ok(BackendAvailability::Absent);
    };
    match Entry::new(SERVICE_NAME, DEEPSEEK_ACCOUNT) {
        Ok(entry) => Ok(BackendAvailability::Available(entry)),
        Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
    }
}

fn native_store_available() -> Result<BackendAvailability<()>, ProviderCredentialError> {
    match Entry::store_status() {
        Ok(()) => Ok(BackendAvailability::Available(())),
        Err(KeyringError::Invalid(name, _)) if name == "platform" => {
            Ok(BackendAvailability::Absent)
        }
        Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
    }
}

#[derive(Clone)]
pub struct ProviderCredentialStore {
    backend: Arc<dyn CredentialBackend>,
    access: Arc<Semaphore>,
}

pub(crate) struct DeepSeekCredential(String);

impl DeepSeekCredential {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

pub(crate) const fn deepseek_credential_error(error: ProviderCredentialError) -> DriverError {
    match error {
        ProviderCredentialError::MissingSecret => DriverError::new(
            "provider_credential_missing",
            "A DeepSeek API credential is required.",
        ),
        ProviderCredentialError::InvalidSecret => DriverError::new(
            "provider_credential_invalid",
            "The configured DeepSeek credential is invalid.",
        ),
        ProviderCredentialError::SecureStoreUnavailable => DriverError::new(
            "secure_store_unavailable",
            "The secure credential store is unavailable.",
        ),
    }
}

impl ProviderCredentialStore {
    #[must_use]
    pub fn production() -> Self {
        Self {
            backend: Arc::new(NativeCredentialBackend),
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
                ProviderCredentialStatus::from_source(ProviderCredentialSource::Missing),
            ),
        }
    }

    /// Resolves one runtime-only `DeepSeek` credential without projecting it publicly.
    ///
    /// # Errors
    ///
    /// Fails closed when the installed store fails or contains an invalid nonempty secret.
    pub(crate) async fn deepseek_secret(
        &self,
    ) -> Result<DeepSeekCredential, ProviderCredentialError> {
        let keyring = self.run_backend(|backend| backend.read()).await?;
        match keyring {
            BackendAvailability::Available(Some(secret)) => {
                validated_secret(&secret).map(DeepSeekCredential)
            }
            BackendAvailability::Available(None) | BackendAvailability::Absent => {
                Err(ProviderCredentialError::MissingSecret)
            }
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

    /// Deletes the secure-store `DeepSeek` credential.
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
    #[cfg(target_os = "macos")]
    use super::{macos_item_exists_from_search, macos_keyring_item_exists};
    #[cfg(target_os = "macos")]
    use security_framework::base::Error as SecurityFrameworkError;
    #[cfg(target_os = "macos")]
    use security_framework_sys::base::{
        errSecAuthFailed, errSecInteractionNotAllowed, errSecItemNotFound,
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

        fn read(&self) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError> {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.fail {
                return Err(ProviderCredentialError::SecureStoreUnavailable);
            }
            if state.absent {
                return Ok(BackendAvailability::Absent);
            }
            Ok(BackendAvailability::Available(
                state.configured.then(|| state.stored.clone()),
            ))
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

    fn store(backend: Arc<TestBackend>) -> ProviderCredentialStore {
        ProviderCredentialStore {
            backend,
            access: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    #[tokio::test]
    async fn secure_store_round_trip_and_delete_becomes_missing() {
        let backend = Arc::new(TestBackend::default());
        let store = store(Arc::clone(&backend));
        assert_eq!(
            store.deepseek_status().await.map(|status| status.source),
            Ok(ProviderCredentialSource::Missing)
        );

        let status = store
            .set_deepseek("  secure-secret  ")
            .await
            .unwrap_or_else(|error| panic!("store secret: {error}"));
        assert_eq!(status.source, ProviderCredentialSource::Keyring);
        assert_eq!(
            store
                .deepseek_secret()
                .await
                .map(|secret| secret.expose().to_owned()),
            Ok("secure-secret".to_owned())
        );
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
        assert_eq!(deleted.source, ProviderCredentialSource::Missing);
        assert!(matches!(
            store.deepseek_secret().await,
            Err(ProviderCredentialError::MissingSecret)
        ));
    }

    #[tokio::test]
    async fn missing_backend_reports_missing_and_rejects_secure_write() {
        let backend = Arc::new(TestBackend::default());
        backend
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .absent = true;
        let store = store(backend);
        assert_eq!(
            store.deepseek_status().await.map(|status| status.source),
            Ok(ProviderCredentialSource::Missing)
        );
        assert!(matches!(
            store.deepseek_secret().await,
            Err(ProviderCredentialError::MissingSecret)
        ));
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
        let store = store(backend);
        assert_eq!(
            store.deepseek_status().await,
            Err(ProviderCredentialError::SecureStoreUnavailable)
        );
        assert!(matches!(
            store.deepseek_secret().await,
            Err(ProviderCredentialError::SecureStoreUnavailable)
        ));
        assert_eq!(
            store.set_deepseek("short").await,
            Err(ProviderCredentialError::InvalidSecret)
        );
        assert_eq!(
            store.set_deepseek(&"x".repeat(8_193)).await,
            Err(ProviderCredentialError::InvalidSecret)
        );
    }

    #[tokio::test]
    async fn runtime_secret_distinguishes_missing_and_invalid_authority() {
        let backend = Arc::new(TestBackend::default());
        let missing_store = store(Arc::clone(&backend));
        assert!(matches!(
            missing_store.deepseek_secret().await,
            Err(ProviderCredentialError::MissingSecret)
        ));
        let store = store(Arc::clone(&backend));
        {
            let mut state = backend
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.configured = true;
            state.stored = "short".to_owned();
        }
        assert!(matches!(
            store.deepseek_secret().await,
            Err(ProviderCredentialError::InvalidSecret)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_status_query_handles_absence_without_secret_material() {
        let service = format!("AgentsAssemble-metadata-probe-{}", uuid::Uuid::new_v4());
        assert!(matches!(
            macos_keyring_item_exists(&service, "missing"),
            Ok(false)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_status_only_treats_item_not_found_as_absent() {
        assert!(matches!(macos_item_exists_from_search(Ok(())), Ok(true)));
        assert!(matches!(
            macos_item_exists_from_search(Err(SecurityFrameworkError::from_code(
                errSecItemNotFound
            ))),
            Ok(false)
        ));
        for code in [errSecInteractionNotAllowed, errSecAuthFailed, -1] {
            assert!(matches!(
                macos_item_exists_from_search(Err(SecurityFrameworkError::from_code(code))),
                Err(error) if error.code() == code
            ));
        }
    }
}
