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
}

impl Drop for CatalogOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl ProviderCatalogService {
    #[must_use]
    pub fn discovering() -> Self {
        Self::discovering_registrations(provider_registrations().to_vec())
    }

    /// Discovers only the explicitly selected external attendee provider.
    ///
    /// # Errors
    /// Rejects unknown or excluded providers before any discovery work starts.
    pub fn discovering_selected(provider_id: &str) -> Result<Self, ProviderSelectionError> {
        let registration = provider_registration_by_id(provider_id).ok_or_else(|| {
            ProviderSelectionError::new("unsupported_provider", "Provider is not supported.")
        })?;
        Ok(Self::discovering_registrations(vec![registration]))
    }

    fn discovering_registrations(registrations: Vec<&'static ProviderRegistration>) -> Self {
        let initial = loading_catalog(&registrations);
        let (sender, receiver) = watch::channel(initial);
        let refresh_sender = sender.clone();
        let cancellation = CancellationToken::new();
        let discovery_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let catalog = discover_catalog(&registrations, &discovery_cancellation).await;
            if !discovery_cancellation.is_cancelled() {
                let _ = refresh_sender.send(catalog);
            }
        });
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation,
                task: Mutex::new(Some(task)),
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
    cancellation: &CancellationToken,
) -> ProviderCatalog {
    let providers = futures_util::future::join_all(
        registrations
            .iter()
            .map(|registration| discover_provider(registration, cancellation)),
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

    #[tokio::test]
    async fn selected_discovery_rejects_excluded_providers_and_only_publishes_selection() {
        for provider in ["freebuff", "antigravity", "unknown"] {
            assert!(ProviderCatalogService::discovering_selected(provider).is_err());
        }
        // Custom API discovery is local metadata only; no CLI or remote catalog is needed.
        let service = ProviderCatalogService::discovering_selected("custom_api")
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
        service
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("catalog shutdown: {error}"));
    }
}
