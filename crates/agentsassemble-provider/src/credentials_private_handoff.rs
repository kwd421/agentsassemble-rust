//! A managed child can read only the credential selected by its parent launch.
use super::{
    BackendAvailability, CredentialBackend, ProviderCredentialError, ProviderCredentialId,
};

// Deliberately no Debug: this value crosses only the private startup pipe.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectedCredential {
    pub(crate) provider: ProviderCredentialId,
    pub(crate) secret: Result<String, ProviderCredentialError>,
}

pub(super) struct PrivateCredentialBackend(pub(super) Option<SelectedCredential>);

impl CredentialBackend for PrivateCredentialBackend {
    fn configured(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<bool>, ProviderCredentialError> {
        self.read(provider).map(|result| match result {
            BackendAvailability::Available(secret) => {
                BackendAvailability::Available(secret.is_some())
            }
            BackendAvailability::Absent => BackendAvailability::Absent,
        })
    }

    fn read(
        &self,
        provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<Option<String>>, ProviderCredentialError> {
        match self
            .0
            .as_ref()
            .filter(|selected| selected.provider == provider)
        {
            Some(selected) => selected
                .secret
                .clone()
                .map(|secret| BackendAvailability::Available(Some(secret))),
            None => Ok(BackendAvailability::Available(None)),
        }
    }

    fn set(
        &self,
        _provider: ProviderCredentialId,
        _secret: &str,
    ) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        Err(ProviderCredentialError::SecureStoreUnavailable)
    }

    fn delete(
        &self,
        _provider: ProviderCredentialId,
    ) -> Result<BackendAvailability<()>, ProviderCredentialError> {
        Err(ProviderCredentialError::SecureStoreUnavailable)
    }
}
