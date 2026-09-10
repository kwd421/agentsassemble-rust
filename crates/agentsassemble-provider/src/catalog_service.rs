//! Owns the provider-discovery task and bounded public catalog publication.
//!
//! Provider definitions and their discovery/launch functions remain in the
//! registration module; this module owns only the catalog service lifecycle.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use agentsassemble_domain::{ProviderAvailability, ProviderCatalog};
use chrono::Utc;
use serde_json::Value;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::Instant,
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
const PROVIDER_CACHE_TTL: Duration = Duration::from_hours(24);

#[derive(Clone)]
pub struct ProviderCatalogService {
    _sender: watch::Sender<ProviderCatalog>,
    receiver: watch::Receiver<ProviderCatalog>,
    owner: Arc<CatalogOwner>,
}

struct CatalogOwner {
    cancellation: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
    refresh: BTreeMap<&'static str, CatalogRefresh>,
}

struct CatalogRefresh {
    requested: watch::Sender<u64>,
    completed: watch::Receiver<DiscoveryCompletion>,
}

#[derive(Clone, Copy, Default)]
struct DiscoveryCompletion {
    generation: u64,
    finished_at: Option<Instant>,
}

struct DiscoveryPublisher {
    providers: Mutex<Vec<ProviderAvailability>>,
    sender: watch::Sender<ProviderCatalog>,
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
    pub fn on_demand(credentials: &crate::ProviderCredentialStore) -> Self {
        Self::discovering_registrations(provider_registrations().to_vec(), credentials, false)
    }

    /// Discovers only the explicitly selected external attendee provider.
    ///
    /// # Errors
    /// Rejects unknown or excluded providers before any discovery work starts.
    pub fn discovering_selected(
        provider_id: &str,
        credentials: &crate::ProviderCredentialStore,
    ) -> Result<Self, ProviderSelectionError> {
        let registration = provider_registration_by_id(provider_id).ok_or_else(|| {
            ProviderSelectionError::new("unsupported_provider", "Provider is not supported.")
        })?;
        Ok(Self::discovering_registrations(
            vec![registration],
            credentials,
            true,
        ))
    }

    fn discovering_registrations(
        registrations: Vec<&'static ProviderRegistration>,
        credentials: &crate::ProviderCredentialStore,
        eager: bool,
    ) -> Self {
        let mut initial = loading_catalog(&registrations);
        if !eager {
            // Registration metadata is available without probing every installed CLI/API.
            // Each provider remains non-startable until its own discovery completes.
            initial = published_catalog(initial.providers, String::new());
        }
        let providers = registrations
            .iter()
            .map(|entry| loading_provider(entry))
            .collect();
        let (sender, receiver) = watch::channel(initial);
        let publisher = Arc::new(DiscoveryPublisher {
            providers: Mutex::new(providers),
            sender: sender.clone(),
        });
        let cancellation = CancellationToken::new();
        let mut refresh = BTreeMap::new();
        let mut discoveries = Vec::new();
        for registration in registrations {
            let (requested, requests) = watch::channel(0_u64);
            let (completed, completion) = watch::channel(DiscoveryCompletion::default());
            refresh.insert(
                registration.id,
                CatalogRefresh {
                    requested,
                    completed: completion,
                },
            );
            discoveries.push(run_provider_discovery(
                registration,
                credentials.clone(),
                cancellation.clone(),
                Arc::clone(&publisher),
                requests,
                completed,
                eager,
            ));
        }
        let task = tokio::spawn(async move {
            futures_util::future::join_all(discoveries).await;
        });
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation,
                task: Mutex::new(Some(task)),
                refresh,
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
                refresh: BTreeMap::new(),
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

    /// Refreshes only the selected provider or reuses its fresh cached result.
    /// A failed catalog remains a failed result, never a refreshed old snapshot.
    ///
    /// # Errors
    /// Rejects fixed catalogs and stopped or failed discovery owners.
    pub async fn refresh_provider(
        &self,
        provider_id: &str,
        force: bool,
    ) -> Result<ProviderCatalog, CatalogRefreshError> {
        let generation = self.request_discovery(provider_id, force)?;
        self.wait_for_provider(provider_id, generation).await
    }

    /// Schedules one local provider without blocking the private control pipe.
    ///
    /// # Errors
    /// Rejects unknown/fixed providers and unavailable discovery owners.
    pub fn request_discovery(
        &self,
        provider_id: &str,
        force: bool,
    ) -> Result<u64, CatalogRefreshError> {
        let refresh = self
            .owner
            .refresh
            .get(provider_id)
            .ok_or(CatalogRefreshError::Unsupported)?;
        if self.owner.cancellation.is_cancelled() || refresh.requested.is_closed() {
            return Err(CatalogRefreshError::Unavailable);
        }
        let completed = *refresh.completed.borrow();
        let requested = *refresh.requested.borrow();
        if requested > completed.generation {
            return Ok(requested);
        }
        if !force
            && completed
                .finished_at
                .is_some_and(|at| at.elapsed() < PROVIDER_CACHE_TTL)
        {
            return Ok(completed.generation);
        }
        let next = completed
            .generation
            .checked_add(1)
            .ok_or(CatalogRefreshError::Unavailable)?;
        refresh.requested.send_if_modified(|requested| {
            if *requested == completed.generation {
                *requested = next;
                true
            } else {
                false
            }
        });
        Ok(*refresh.requested.borrow())
    }

    /// Reads the result of an already requested generation; never starts discovery.
    ///
    /// # Errors
    /// Rejects future generations and unavailable or unsupported discovery owners.
    pub async fn wait_for_provider(
        &self,
        provider_id: &str,
        generation: u64,
    ) -> Result<ProviderCatalog, CatalogRefreshError> {
        let refresh = self
            .owner
            .refresh
            .get(provider_id)
            .ok_or(CatalogRefreshError::Unsupported)?;
        if generation > *refresh.requested.borrow() {
            return Err(CatalogRefreshError::Unsupported);
        }
        let mut completion = refresh.completed.clone();
        loop {
            if self.owner.cancellation.is_cancelled() {
                return Err(CatalogRefreshError::Unavailable);
            }
            if completion.borrow_and_update().generation >= generation {
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

async fn run_provider_discovery(
    registration: &'static ProviderRegistration,
    credentials: crate::ProviderCredentialStore,
    cancellation: CancellationToken,
    publisher: Arc<DiscoveryPublisher>,
    mut requests: watch::Receiver<u64>,
    completed: watch::Sender<DiscoveryCompletion>,
    mut eager: bool,
) {
    loop {
        if !eager {
            tokio::select! {
                () = cancellation.cancelled() => break,
                changed = requests.changed() => if changed.is_err() { break; },
            }
        }
        eager = false;
        let generation = *requests.borrow_and_update();
        let access = if generation == 0 {
            credentials.clone()
        } else {
            credentials.for_user_requested_access()
        };
        let provider = discover_provider(registration, &access, &cancellation).await;
        if cancellation.is_cancelled() {
            break;
        }
        let mut providers = publisher.providers.lock().await;
        if let Some(previous) = providers
            .iter_mut()
            .find(|entry| entry.id == registration.id)
        {
            *previous = provider;
        }
        publisher.sender.send_replace(published_catalog(
            providers.clone(),
            Utc::now().to_rfc3339(),
        ));
        completed.send_replace(DiscoveryCompletion {
            generation,
            finished_at: Some(Instant::now()),
        });
    }
}

fn published_catalog(
    providers: Vec<ProviderAvailability>,
    discovered_at: String,
) -> ProviderCatalog {
    let (status, catalog_revision) = match catalog_revision(&providers) {
        Ok(revision) => ("ready".to_owned(), revision),
        Err(_) => ("failed".to_owned(), String::new()),
    };
    bound_catalog(ProviderCatalog {
        status,
        catalog_revision,
        discovered_at,
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SELECTED_CALLS: AtomicUsize = AtomicUsize::new(0);
    static UNSELECTED_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROBE_STARTED: tokio::sync::Notify = tokio::sync::Notify::const_new();
    static PROBE_RELEASE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(0);

    fn counted_discovery<'a>(
        provider: agentsassemble_domain::ProviderAvailability,
        _credentials: &'a crate::ProviderCredentialStore,
        _cancellation: &'a tokio_util::sync::CancellationToken,
    ) -> crate::registration::ProviderDiscoveryFuture<'a> {
        Box::pin(async move {
            if provider.id == "custom_api" {
                SELECTED_CALLS.fetch_add(1, Ordering::SeqCst);
                PROBE_STARTED.notify_one();
                PROBE_RELEASE
                    .acquire()
                    .await
                    .unwrap_or_else(|error| panic!("probe: {error}"))
                    .forget();
            } else {
                UNSELECTED_CALLS.fetch_add(1, Ordering::SeqCst);
            }
            crate::catalog::ready_provider(
                provider,
                "selected-model".into(),
                vec![crate::catalog::control(
                    "model",
                    "Model",
                    "combobox",
                    vec![crate::catalog::option("selected-model", "Selected model")],
                    "selected-model",
                )],
            )
        })
    }

    #[tokio::test(start_paused = true)]
    async fn selected_requests_share_work_and_cache_without_probing_other_providers() {
        let selected = Box::leak(Box::new(crate::registration::ProviderRegistration {
            discover: counted_discovery,
            login: None,
            configuration_authority: crate::registration::ProviderConfigurationAuthority::Catalog,
            remote_spec: None,
            ..crate::registration::CUSTOM_API_PROVIDER
        }));
        let other = Box::leak(Box::new(crate::registration::ProviderRegistration {
            id: "unselected",
            discover: counted_discovery,
            login: None,
            configuration_authority: crate::registration::ProviderConfigurationAuthority::Catalog,
            remote_spec: None,
            ..crate::registration::CUSTOM_API_PROVIDER
        }));
        let needs_auth = Box::leak(Box::new(crate::registration::ProviderRegistration {
            discover: counted_discovery,
            login: None,
            ..crate::registration::DEEPSEEK_PROVIDER
        }));
        let service = ProviderCatalogService::discovering_registrations(
            vec![selected, other, needs_auth],
            &crate::ProviderCredentialStore::isolated_test_store(),
            false,
        );
        tokio::task::yield_now().await;
        assert_eq!(SELECTED_CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(UNSELECTED_CALLS.load(Ordering::SeqCst), 0);
        let first = service
            .request_discovery("custom_api", false)
            .unwrap_or_else(|error| panic!("request: {error}"));
        tokio::time::timeout(std::time::Duration::from_secs(1), PROBE_STARTED.notified())
            .await
            .unwrap_or_else(|error| panic!("selected probe did not start: {error}"));
        assert_eq!(
            first,
            service
                .request_discovery("custom_api", true)
                .unwrap_or_else(|error| panic!("join: {error}"))
        );
        PROBE_RELEASE.add_permits(1);
        let catalog = service
            .wait_for_provider("custom_api", first)
            .await
            .unwrap_or_else(|error| panic!("result: {error}"));
        assert_eq!(catalog.providers[0].default_model, "selected-model");
        assert!(!catalog.providers[1].startable);
        tokio::time::advance(
            super::PROVIDER_CACHE_TTL
                .checked_sub(std::time::Duration::from_secs(1))
                .unwrap_or_else(|| panic!("cache TTL exceeds one second")),
        )
        .await;
        let cached = service
            .refresh_provider("custom_api", false)
            .await
            .unwrap_or_else(|error| panic!("cached: {error}"));
        assert_eq!(cached, catalog);
        assert_eq!(SELECTED_CALLS.load(Ordering::SeqCst), 1);
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        for force in [false, true] {
            let next = service
                .request_discovery("custom_api", force)
                .unwrap_or_else(|error| panic!("next: {error}"));
            tokio::time::timeout(std::time::Duration::from_secs(1), PROBE_STARTED.notified())
                .await
                .unwrap_or_else(|error| panic!("selected probe did not start: {error}"));
            PROBE_RELEASE.add_permits(1);
            service
                .wait_for_provider("custom_api", next)
                .await
                .unwrap_or_else(|error| panic!("result: {error}"));
        }
        assert_eq!(SELECTED_CALLS.load(Ordering::SeqCst), 3);
        let unauthenticated = service
            .refresh_provider("deepseek", true)
            .await
            .unwrap_or_else(|error| panic!("missing credential: {error}"));
        assert_eq!(
            unauthenticated.providers[2].discovery_error_code,
            "authentication_required"
        );
        assert_eq!(UNSELECTED_CALLS.load(Ordering::SeqCst), 0);
        assert!(
            service
                .wait_for_provider("custom_api", u64::MAX)
                .await
                .is_err()
        );
        service
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("shutdown: {error}"));
    }

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
            &credentials,
            false,
        );
        let first = service
            .refresh_provider("deepseek", true)
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
            .refresh_provider("deepseek", true)
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
            .refresh_provider("deepseek", true)
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
                    &crate::ProviderCredentialStore::isolated_test_store()
                )
                .is_err()
            );
        }
        // Custom API discovery is local metadata only; no CLI or remote catalog is needed.
        let service = ProviderCatalogService::discovering_selected(
            "custom_api",
            &crate::ProviderCredentialStore::isolated_test_store(),
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
        let (first, second) = tokio::join!(
            service.refresh_provider("custom_api", true),
            service.refresh_provider("custom_api", true)
        );
        let first = first.unwrap_or_else(|error| panic!("refresh: {error}"));
        let second = second.unwrap_or_else(|error| panic!("join refresh: {error}"));
        assert_eq!(first, second);
        assert_eq!(first.providers.len(), 1);
        assert_eq!(first.providers[0].id, "custom_api");
        assert_eq!(first.catalog_revision, catalog.catalog_revision);
        let refresh = service
            .owner
            .refresh
            .get("custom_api")
            .unwrap_or_else(|| panic!("discovery owner"));
        assert_eq!(refresh.completed.borrow().generation, 1);
        service
            .shutdown()
            .await
            .unwrap_or_else(|error| panic!("catalog shutdown: {error}"));
        assert!(matches!(
            service.refresh_provider("custom_api", true).await,
            Err(super::CatalogRefreshError::Unavailable)
        ));
        assert!(matches!(
            ProviderCatalogService::fixed(first)
                .refresh_provider("custom_api", true)
                .await,
            Err(super::CatalogRefreshError::Unsupported)
        ));
    }
}
