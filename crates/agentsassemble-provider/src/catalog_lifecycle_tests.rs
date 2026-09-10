//! Discovery cleanup custody across refresh and shutdown, using controlled owners.
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{CatalogShutdownError, ProviderCatalogService, ProviderCredentialStore};

static UNCERTAIN_CALLS: AtomicUsize = AtomicUsize::new(0);
static CANCEL_STARTED: tokio::sync::Notify = tokio::sync::Notify::const_new();
static CANCEL_RELEASE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(0);
static RETRY_CALLS: AtomicUsize = AtomicUsize::new(0);

fn controlled_discovery<'a>(
    provider: agentsassemble_domain::ProviderAvailability,
    _credentials: &'a ProviderCredentialStore,
    cancellation: &'a tokio_util::sync::CancellationToken,
) -> crate::registration::ProviderDiscoveryFuture<'a> {
    Box::pin(async move {
        let failure = match provider.id.as_str() {
            "task_loss" => panic!("controlled discovery task loss"),
            "uncertain" => {
                UNCERTAIN_CALLS.fetch_add(1, Ordering::SeqCst);
                crate::process::ProbeFailure::CleanupUnconfirmed
            }
            "cancel_uncertain" => {
                CANCEL_STARTED.notify_one();
                cancellation.cancelled().await;
                CANCEL_RELEASE
                    .acquire()
                    .await
                    .unwrap_or_else(|error| panic!("release: {error}"))
                    .forget();
                crate::process::ProbeFailure::CleanupUnconfirmed
            }
            "retry" => {
                if RETRY_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
                    crate::process::ProbeFailure::Timeout
                } else {
                    crate::process::ProbeFailure::Failed
                }
            }
            _ => panic!("unexpected fixture"),
        };
        crate::catalog::failed_provider(provider, failure)
    })
}

fn service(id: &'static str) -> ProviderCatalogService {
    let registration = Box::leak(Box::new(crate::registration::ProviderRegistration {
        id,
        discover: controlled_discovery,
        login: None,
        configuration_authority: crate::registration::ProviderConfigurationAuthority::Catalog,
        remote_spec: None,
        ..crate::registration::CUSTOM_API_PROVIDER
    }));
    ProviderCatalogService::discovering_registrations(
        vec![registration],
        &ProviderCredentialStore::isolated_test_store(),
        false,
    )
}

#[tokio::test]
async fn unconfirmed_cleanup_blocks_force_and_remains_a_shutdown_failure() {
    let service = service("uncertain");
    assert!(service.refresh_provider("uncertain", true).await.is_err());
    let provider = service.snapshot().providers.remove(0);
    assert_eq!(
        provider.discovery_error_code,
        "model_discovery_cleanup_failed"
    );
    assert!(!provider.startable);
    for force in [false, true] {
        assert!(service.refresh_provider("uncertain", force).await.is_err());
    }
    assert_eq!(UNCERTAIN_CALLS.load(Ordering::SeqCst), 1);
    for _ in 0..2 {
        assert!(matches!(
            service.shutdown().await,
            Err(CatalogShutdownError::CleanupUnconfirmed)
        ));
    }
}

#[tokio::test]
async fn cancellation_and_dropped_shutdown_waiter_retain_cleanup_uncertainty() {
    let service = service("cancel_uncertain");
    service
        .request_discovery("cancel_uncertain", true)
        .unwrap_or_else(|error| panic!("request: {error}"));
    CANCEL_STARTED.notified().await;
    // Poll shutdown until the discovery owner observes cancellation, then abandon
    // only this waiter. The retained shared result must still join that owner.
    let mut first = Box::pin(service.shutdown());
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    drop(first);
    CANCEL_RELEASE.add_permits(1);
    let (left, right) = tokio::join!(service.shutdown(), service.shutdown());
    assert!(matches!(
        left,
        Err(CatalogShutdownError::CleanupUnconfirmed)
    ));
    assert!(matches!(
        right,
        Err(CatalogShutdownError::CleanupUnconfirmed)
    ));
    assert_eq!(
        service.snapshot().providers[0].discovery_error_code,
        "model_discovery_cleanup_failed"
    );
    assert!(service.request_discovery("cancel_uncertain", true).is_err());
    assert!(matches!(
        service.shutdown().await,
        Err(CatalogShutdownError::CleanupUnconfirmed)
    ));
}

#[tokio::test]
async fn confirmed_cleanup_failures_allow_explicit_retry_and_clean_shutdown() {
    let service = service("retry");
    let first = service
        .refresh_provider("retry", true)
        .await
        .unwrap_or_else(|error| panic!("timeout projection: {error}"));
    assert_eq!(
        first.providers[0].discovery_error_code,
        "model_discovery_timeout"
    );
    let second = service
        .refresh_provider("retry", true)
        .await
        .unwrap_or_else(|error| panic!("retry projection: {error}"));
    assert_eq!(
        second.providers[0].discovery_error_code,
        "model_discovery_failed"
    );
    assert_eq!(RETRY_CALLS.load(Ordering::SeqCst), 2);
    service
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("clean shutdown: {error}"));
    service
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("repeated clean shutdown: {error}"));
}

#[tokio::test]
async fn lost_discovery_task_cannot_be_retried_or_hidden_by_repeated_shutdown() {
    let service = service("task_loss");
    assert!(service.refresh_provider("task_loss", true).await.is_err());
    assert!(service.request_discovery("task_loss", true).is_err());
    for _ in 0..2 {
        assert!(matches!(
            service.shutdown().await,
            Err(CatalogShutdownError::TaskFailed)
        ));
    }
}
