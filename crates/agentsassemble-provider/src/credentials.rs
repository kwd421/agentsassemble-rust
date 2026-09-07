use std::sync::Arc;

use keyring::v1::{Entry, Error as KeyringError};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::credential_provider::ProviderCredentialId;

#[cfg(target_os = "macos")]
use security_framework::item::{ItemClass, ItemSearchOptions};
#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::{
    KeychainUserInteractionLock, SecKeychain, SecPreferencesDomain,
};
#[cfg(target_os = "macos")]
use security_framework_sys::base::errSecItemNotFound;

const SERVICE_NAME: &str = "AgentsAssemble";
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
    fn configured(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<bool>, ProviderCredentialError>;
    fn read(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError>;
    fn set(
        &self,
        provider: ProviderCredentialId,
        secret: &str,
    ) -> Result<BackendAvailability<()>, ProviderCredentialError>;
    fn delete(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<()>, ProviderCredentialError>;
}

struct NativeCredentialBackend;

impl CredentialBackend for NativeCredentialBackend {
    fn configured(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<bool>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        {
            let BackendAvailability::Available(()) = native_store_available()? else {
                return Ok(BackendAvailability::Absent);
            };
            match macos_keyring_item_exists(SERVICE_NAME, provider.as_str()) {
                Ok(configured) => Ok(BackendAvailability::Available(configured)),
                Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let BackendAvailability::Available(entry) = native_entry(provider)? else {
                return Ok(BackendAvailability::Absent);
            };
            match entry.get_password() {
                Ok(secret) => Ok(BackendAvailability::Available(!secret.is_empty())),
                Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(false)),
                Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
            }
        }
    }

    fn read(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        let _interaction = macos_disable_keychain_ui()?;
        let BackendAvailability::Available(entry) = native_entry(provider)? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.get_password() {
            Ok(secret) => Ok(BackendAvailability::Available(Some(secret))),
            Err(KeyringError::NoEntry) => Ok(BackendAvailability::Available(None)),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    fn set(
        &self,
        provider: ProviderCredentialId,
        secret: &str,
    ) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        let _interaction = macos_disable_keychain_ui()?;
        let BackendAvailability::Available(entry) = native_entry(provider)? else {
            return Ok(BackendAvailability::Absent);
        };
        match entry.set_password(secret) {
            Ok(()) => Ok(BackendAvailability::Available(())),
            Err(_) => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    fn delete(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        #[cfg(target_os = "macos")]
        let _interaction = macos_disable_keychain_ui()?;
        let BackendAvailability::Available(entry) = native_entry(provider)? else {
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

fn native_entry(
    provider: ProviderCredentialId,
) -> Result<BackendAvailability<Entry>, ProviderCredentialError> {
    let BackendAvailability::Available(()) = native_store_available()? else {
        return Ok(BackendAvailability::Absent);
    };
    match Entry::new(SERVICE_NAME, provider.as_str()) {
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

pub(crate) struct ProviderCredential(String);

impl ProviderCredential {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl ProviderCredentialStore {
    #[cfg(test)]
    pub(crate) fn isolated_test_store() -> Self {
        tests::isolated_store()
    }

    #[must_use]
    pub fn production() -> Self {
        Self {
            backend: Arc::new(NativeCredentialBackend),
            access: Arc::new(Semaphore::new(1)),
        }
    }

    /// Returns public credential metadata without returning the secret.
    ///
    /// # Errors
    ///
    /// Returns `secure_store_unavailable` when an installed secure store fails.
    pub async fn status(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<ProviderCredentialStatus, ProviderCredentialError> {
        match self
            .run_backend(move |backend| backend.configured(provider))
            .await?
        {
            BackendAvailability::Available(true) => Ok(ProviderCredentialStatus::from_source(
                ProviderCredentialSource::Keyring,
            )),
            BackendAvailability::Available(false) | BackendAvailability::Absent => Ok(
                ProviderCredentialStatus::from_source(ProviderCredentialSource::Missing),
            ),
        }
    }

    /// Resolves one runtime-only provider credential without projecting it publicly.
    pub(crate) async fn secret(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<ProviderCredential, ProviderCredentialError> {
        let keyring = self
            .run_backend(move |backend| backend.read(provider))
            .await?;
        match keyring {
            BackendAvailability::Available(Some(secret)) => {
                validated_secret(&secret).map(ProviderCredential)
            }
            BackendAvailability::Available(None) | BackendAvailability::Absent => {
                Err(ProviderCredentialError::MissingSecret)
            }
        }
    }

    /// Stores one validated provider credential in the platform secure store.
    ///
    /// # Errors
    ///
    /// Returns a stable validation or secure-store error without embedding the secret.
    pub async fn set(
        &self,
        provider: ProviderCredentialId,
        secret: &str,
    ) -> Result<ProviderCredentialStatus, ProviderCredentialError> {
        let secret = validated_secret(secret)?;
        match self
            .run_backend(move |backend| backend.set(provider, &secret))
            .await?
        {
            BackendAvailability::Available(()) => self.status(provider).await,
            BackendAvailability::Absent => Err(ProviderCredentialError::SecureStoreUnavailable),
        }
    }

    /// Deletes one provider credential from the secure store.
    ///
    /// # Errors
    ///
    /// Returns `secure_store_unavailable` when an installed secure store fails.
    pub async fn delete(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<ProviderCredentialStatus, ProviderCredentialError> {
        self.run_backend(move |backend| backend.delete(provider))
            .await?;
        self.status(provider).await
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
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use super::{
        BackendAvailability, CredentialBackend, ProviderCredentialError, ProviderCredentialId,
        ProviderCredentialSource, ProviderCredentialStore,
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
        absent: bool,
        fail: bool,
        stored: BTreeMap<ProviderCredentialId, String>,
    }

    impl CredentialBackend for TestBackend {
        fn configured(
            &self,
            provider: ProviderCredentialId,
        ) -> Result<BackendAvailability<bool>, ProviderCredentialError> {
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
                Ok(BackendAvailability::Available(
                    state.stored.contains_key(&provider),
                ))
            }
        }

        fn read(
            &self,
            provider: ProviderCredentialId,
        ) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError> {
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
                state.stored.get(&provider).cloned(),
            ))
        }

        fn set(
            &self,
            provider: ProviderCredentialId,
            secret: &str,
        ) -> Result<BackendAvailability<()>, ProviderCredentialError> {
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
            state.stored.insert(provider, secret.to_owned());
            Ok(BackendAvailability::Available(()))
        }

        fn delete(
            &self,
            provider: ProviderCredentialId,
        ) -> Result<BackendAvailability<()>, ProviderCredentialError> {
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
            state.stored.remove(&provider);
            Ok(BackendAvailability::Available(()))
        }
    }

    fn store(backend: Arc<TestBackend>) -> ProviderCredentialStore {
        ProviderCredentialStore {
            backend,
            access: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(super) fn isolated_store() -> ProviderCredentialStore {
        store(Arc::new(TestBackend::default()))
    }

    #[tokio::test]
    async fn secure_store_round_trip_and_delete_becomes_missing() {
        let backend = Arc::new(TestBackend::default());
        let store = store(Arc::clone(&backend));
        assert_eq!(
            store
                .status(ProviderCredentialId::DeepSeek)
                .await
                .map(|status| status.source),
            Ok(ProviderCredentialSource::Missing)
        );

        let status = store
            .set(ProviderCredentialId::DeepSeek, "  secure-secret  ")
            .await
            .unwrap_or_else(|error| panic!("store secret: {error}"));
        assert_eq!(status.source, ProviderCredentialSource::Keyring);
        assert_eq!(
            store
                .secret(ProviderCredentialId::DeepSeek)
                .await
                .map(|secret| secret.expose().to_owned()),
            Ok("secure-secret".to_owned())
        );
        assert_eq!(
            backend
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .stored
                .get(&ProviderCredentialId::DeepSeek)
                .cloned(),
            Some("secure-secret".to_owned())
        );

        let deleted = store
            .delete(ProviderCredentialId::DeepSeek)
            .await
            .unwrap_or_else(|error| panic!("delete secret: {error}"));
        assert_eq!(deleted.source, ProviderCredentialSource::Missing);
        assert!(matches!(
            store.secret(ProviderCredentialId::DeepSeek).await,
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
            store
                .status(ProviderCredentialId::DeepSeek)
                .await
                .map(|status| status.source),
            Ok(ProviderCredentialSource::Missing)
        );
        assert!(matches!(
            store.secret(ProviderCredentialId::DeepSeek).await,
            Err(ProviderCredentialError::MissingSecret)
        ));
        assert_eq!(
            store
                .set(ProviderCredentialId::DeepSeek, "secure-secret")
                .await,
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
            store.status(ProviderCredentialId::DeepSeek).await,
            Err(ProviderCredentialError::SecureStoreUnavailable)
        );
        assert!(matches!(
            store.secret(ProviderCredentialId::DeepSeek).await,
            Err(ProviderCredentialError::SecureStoreUnavailable)
        ));
        assert_eq!(
            store.set(ProviderCredentialId::DeepSeek, "short").await,
            Err(ProviderCredentialError::InvalidSecret)
        );
        assert_eq!(
            store
                .set(ProviderCredentialId::DeepSeek, &"x".repeat(8_193))
                .await,
            Err(ProviderCredentialError::InvalidSecret)
        );
    }

    #[tokio::test]
    async fn runtime_secret_distinguishes_missing_and_invalid_authority() {
        let backend = Arc::new(TestBackend::default());
        let missing_store = store(Arc::clone(&backend));
        assert!(matches!(
            missing_store.secret(ProviderCredentialId::DeepSeek).await,
            Err(ProviderCredentialError::MissingSecret)
        ));
        let store = store(Arc::clone(&backend));
        {
            let mut state = backend
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .stored
                .insert(ProviderCredentialId::DeepSeek, "short".to_owned());
        }
        assert!(matches!(
            store.secret(ProviderCredentialId::DeepSeek).await,
            Err(ProviderCredentialError::InvalidSecret)
        ));
    }

    #[tokio::test]
    async fn provider_accounts_are_isolated() {
        let store = store(Arc::new(TestBackend::default()));
        store
            .set(ProviderCredentialId::DeepSeek, "deepseek-secret")
            .await
            .unwrap_or_else(|error| panic!("store DeepSeek secret: {error}"));
        store
            .set(ProviderCredentialId::OpenRouter, "openrouter-secret")
            .await
            .unwrap_or_else(|error| panic!("store OpenRouter secret: {error}"));

        store
            .delete(ProviderCredentialId::DeepSeek)
            .await
            .unwrap_or_else(|error| panic!("delete DeepSeek secret: {error}"));

        assert!(matches!(
            store.secret(ProviderCredentialId::DeepSeek).await,
            Err(ProviderCredentialError::MissingSecret)
        ));
        assert_eq!(
            store
                .secret(ProviderCredentialId::OpenRouter)
                .await
                .map(|secret| secret.expose().to_owned()),
            Ok("openrouter-secret".to_owned())
        );
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
