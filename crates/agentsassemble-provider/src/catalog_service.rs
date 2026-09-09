//! Owns the provider-discovery task and bounded public catalog publication.
//!
//! Provider definitions and their discovery/launch functions remain in the
//! registration module; this module owns only the catalog service lifecycle.

use std::sync::Arc;

use agentsassemble_domain::ProviderCatalog;
use chrono::Utc;
use serde_json::Value;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::catalog_revision,
    registration::{
        ProviderRegistration, discover_provider, loading_provider, provider_registration_by_id,
        provider_registrations,
    },
    selection::{ProviderSelection, ProviderSelectionError},
};

// The complete public catalog must leave a quarter of the 256 KiB WebSocket
// frame for room metadata; oversized catalogs fail closed before publication.
const MAX_PUBLIC_CATALOG_BYTES: usize = 192 * 1024;

#[derive(Clone)]
pub struct ProviderCatalogService {
    _sender: watch::Sender<ProviderCatalog>,
    receiver: watch::Receiver<ProviderCatalog>,
    owner: Arc<CatalogOwner>,
}

struct CatalogOwner {
    cancellation: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
    refresh: Option<CatalogRefresh>,
}

struct CatalogRefresh {
    requested: watch::Sender<u64>,
    completed: watch::Receiver<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogRefreshError {
    #[error("This catalog does not own provider discovery.")]
    Unsupported,
    #[error("Provider catalog discovery is unavailable.")]
    Unavailable,
}

impl Drop for CatalogOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl ProviderCatalogService {
    #[must_use]
    pub fn discovering(credentials: crate::ProviderCredentialStore) -> Self {
        Self::discovering_registrations(provider_registrations().to_vec(), credentials)
    }

    /// Discovers only the explicitly selected external attendee provider.
    ///
    /// # Errors
    /// Rejects unknown or excluded providers before any discovery work starts.
    pub fn discovering_selected(
        provider_id: &str,
        credentials: crate::ProviderCredentialStore,
    ) -> Result<Self, ProviderSelectionError> {
        let registration = provider_registration_by_id(provider_id).ok_or_else(|| {
            ProviderSelectionError::new("unsupported_provider", "Provider is not supported.")
        })?;
        Ok(Self::discovering_registrations(
            vec![registration],
            credentials,
        ))
    }

    fn discovering_registrations(
        registrations: Vec<&'static ProviderRegistration>,
        credentials: crate::ProviderCredentialStore,
    ) -> Self {
        let initial = loading_catalog(&registrations);
        let (sender, receiver) = watch::channel(initial);
        let refresh_sender = sender.clone();
        let cancellation = CancellationToken::new();
        let discovery_cancellation = cancellation.clone();
        let (requested, mut requests) = watch::channel(0_u64);
        let (completed, completion) = watch::channel(0_u64);
        let task = tokio::spawn(async move {
            loop {
                let generation = *requests.borrow_and_update();
                let catalog =
                    discover_catalog(&registrations, &credentials, &discovery_cancellation).await;
                if discovery_cancellation.is_cancelled() {
                    break;
                }
                refresh_sender.send_replace(catalog);
                completed.send_replace(generation);
                tokio::select! {
                    () = discovery_cancellation.cancelled() => break,
                    changed = requests.changed() => if changed.is_err() { break; },
                }
            }
        });
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation,
                task: Mutex::new(Some(task)),
                refresh: Some(CatalogRefresh {
                    requested,
                    completed: completion,
                }),
            }),
        }
    }

    #[must_use]
    pub fn fixed(catalog: ProviderCatalog) -> Self {
        let (sender, receiver) = watch::channel(bound_catalog(catalog));
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation: CancellationToken::new(),
                task: Mutex::new(None),
                refresh: None,
            }),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ProviderCatalog {
        self.receiver.borrow().clone()
    }

    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<ProviderCatalog> {
        self.receiver.clone()
    }

    /// Joins one explicit discovery generation and returns its published result.
    /// A failed catalog remains a failed result, never a refreshed old snapshot.
    ///
    /// # Errors
    /// Rejects fixed catalogs and stopped or failed discovery owners.
    pub async fn refresh(&self) -> Result<ProviderCatalog, CatalogRefreshError> {
        let refresh = self
            .owner
            .refresh
            .as_ref()
            .ok_or(CatalogRefreshError::Unsupported)?;
        if self.owner.cancellation.is_cancelled() {
            return Err(CatalogRefreshError::Unavailable);
        }
        let mut completion = refresh.completed.clone();
        let completed = *completion.borrow_and_update();
        let next = completed
            .checked_add(1)
            .ok_or(CatalogRefreshError::Unavailable)?;
        refresh.requested.send_if_modified(|requested| {
            if *requested == completed {
                *requested = next;
                true
            } else {
                false
            }
        });
        let requested = *refresh.requested.borrow();
        loop {
            if *completion.borrow_and_update() >= requested {
                return Ok(self.snapshot());
            }
            tokio::select! {
                () = self.owner.cancellation.cancelled() => return Err(CatalogRefreshError::Unavailable),
                changed = completion.changed() => changed.map_err(|_| CatalogRefreshError::Unavailable)?,
            }
        }
    }

    /// Cancels provider discovery and waits for its task to exit.
    ///
    /// # Errors
    ///
    /// Returns the discovery task's join error instead of hiding a panic or cancellation.
    pub async fn shutdown(&self) -> Result<(), tokio::task::JoinError> {
        self.owner.cancellation.cancel();
        if let Some(task) = self.owner.task.lock().await.take() {
            task.await?;
        }
        Ok(())
    }

    /// Validates a raw `agent.create` request against one exact catalog revision.
    ///
    /// # Errors
    ///
    /// Returns a stable fail-closed selection error.
    pub async fn validate_creation(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        payload: &Value,
    ) -> Result<ProviderSelection, ProviderSelectionError> {
        ProviderSelection::from_catalog(
            room_id,
            principal_id,
            request_id,
            payload,
            &self.snapshot(),
        )
        .await
    }
}

async fn discover_catalog(
    registrations: &[&'static ProviderRegistration],
    credentials: &crate::ProviderCredentialStore,
    cancellation: &CancellationToken,
) -> ProviderCatalog {
    let providers = futures_util::future::join_all(
        registrations
            .iter()
            .map(|registration| discover_provider(registration, credentials, cancellation)),
    )
    .await;
    let (status, catalog_revision) = match catalog_revision(&providers) {
        Ok(revision) => ("ready".to_owned(), revision),
        Err(_) => ("failed".to_owned(), String::new()),
    };
    bound_catalog(ProviderCatalog {
        status,
        catalog_revision,
        discovered_at: Utc::now().to_rfc3339(),
        providers,
    })
}

fn bound_catalog(catalog: ProviderCatalog) -> ProviderCatalog {
    if serde_json::to_vec(&catalog).map_or(true, |encoded| encoded.len() > MAX_PUBLIC_CATALOG_BYTES)
    {
        return ProviderCatalog {
            status: "failed".to_owned(),
            catalog_revision: String::new(),
            discovered_at: catalog.discovered_at,
            providers: Vec::new(),
        };
    }
    catalog
}

fn loading_catalog(registrations: &[&'static ProviderRegistration]) -> ProviderCatalog {
    ProviderCatalog {
        status: "loading".to_owned(),
        catalog_revision: String::new(),
        discovered_at: String::new(),
        providers: registrations
            .iter()
            .map(|registration| loading_provider(registration))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderCatalogService;

    fn credentialed_discovery<'a>(
        provider: agentsassemble_domain::ProviderAvailability,
        credentials: &'a crate::ProviderCredentialStore,
        _cancellation: &'a tokio_util::sync::CancellationToken,
    ) -> crate::registration::ProviderDiscoveryFuture<'a> {
        Box::pin(async move {
            if credentials
                .status(crate::ProviderCredentialId::DeepSeek)
                .await
                .is_ok_and(|status| status.configured)
            {
                crate::catalog::ready_provider(
                    provider,
                    "new-model".to_owned(),
                    vec![crate::catalog::control(
                        "model",
                        "Model",
                        "combobox",
                        vec![crate::catalog::option("new-model", "New model")],
                        "new-model",
                    )],
                )
            } else {
                crate::catalog::failed_provider(
                    provider,
                    crate::process::ProbeFailure::Authentication,
                )
            }
        })
    }

    #[tokio::test]
    async fn refresh_observes_credential_changes_and_replaces_a_previous_ready_snapshot() {
        let credentials = crate::ProviderCredentialStore::isolated_test_store();
        let registration = Box::leak(Box::new(crate::registration::ProviderRegistration {
            discover: credentialed_discovery,
            login: None,
            ..crate::registration::DEEPSEEK_PROVIDER
        }));
        let service = ProviderCatalogService::discovering_registrations(
            vec![registration],
            credentials.clone(),
        );
        let first = service
            .refresh()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!first.providers[0].startable);
        credentials
            .set(
                crate::ProviderCredentialId::DeepSeek,
                "isolated-fixture-value",
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let fresh = service
            .refresh()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(fresh.providers[0].startable);
        assert_eq!(fresh.providers[0].default_model, "new-model");
        assert_ne!(first.catalog_revision, fresh.catalog_revision);
        credentials
            .delete(crate::ProviderCredentialId::DeepSeek)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let failed = service
            .refresh()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!failed.providers[0].startable);
        assert!(failed.providers[0].controls.is_empty());
        assert_ne!(failed.catalog_revision, fresh.catalog_revision);
        service
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[tokio::test]
    async fn selected_discovery_rejects_excluded_providers_and_only_publishes_selection() {
        for provider in ["freebuff", "antigravity", "unknown"] {
            assert!(
                ProviderCatalogService::discovering_selected(
                    provider,
                    crate::ProviderCredentialStore::isolated_test_store()
                )
                .is_err()
            );
        }
        // Custom API discovery is local metadata only; no CLI or remote catalog is needed.
        let service = ProviderCatalogService::discovering_selected(
            "custom_api",
            crate::ProviderCredentialStore::isolated_test_store(),
        )
        .unwrap_or_else(|error| panic!("select custom API: {error}"));
        let mut updates = service.subscribe();
        let catalog = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let catalog = updates.borrow_and_update().clone();
                if catalog.status != "loading" {
                    break catalog;
                }
                updates
                    .changed()
                    .await
                    .unwrap_or_else(|error| panic!("catalog: {error}"));
            }
        })
        .await
        .unwrap_or_else(|error| panic!("catalog deadline: {error}"));
        assert_eq!(catalog.status, "ready");
        assert_eq!(catalog.providers.len(), 1);
        assert_eq!(catalog.providers[0].id, "custom_api");
        let (first, second) = tokio::join!(service.refresh(), service.refresh());
        let first = first.unwrap_or_else(|error| panic!("refresh: {error}"));
        let second = second.unwrap_or_else(|error| panic!("join refresh: {error}"));
        assert_eq!(first, second);
        assert_eq!(first.providers.len(), 1);
        assert_eq!(first.providers[0].id, "custom_api");
        assert_eq!(first.catalog_revision, catalog.catalog_revision);
        let refresh = service
            .owner
            .refresh
            .as_ref()
            .unwrap_or_else(|| panic!("discovery owner"));
        assert_eq!(*refresh.completed.borrow(), 1);
        service
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("catalog shutdown: {error}"));
        assert!(matches!(
            service.refresh().await,
            Err(super::CatalogRefreshError::Unavailable)
        ));
        assert!(matches!(
            ProviderCatalogService::fixed(first).refresh().await,
            Err(super::CatalogRefreshError::Unsupported)
        ));
    }
}
