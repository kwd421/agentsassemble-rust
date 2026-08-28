use std::{num::NonZeroU64, path::Path, sync::Arc, time::Duration};

use axum::http::{HeaderMap, HeaderValue, header};
use parking_lot::RwLock;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    ActiveGeneration, CanonicalPublicOrigin, GenerationOutcome, ManagedController,
    ManagedIngressConfig, ManagedLifecycle, ManagedProjection, ManagedPublicIngress,
    ManagedReadiness, PublicIngress, PublicIngressAuthorization, PublicIngressControlError,
    PublicIngressKind, stable_target_matches_direct,
};
use crate::product_surface::RouteExposure;

pub(crate) fn started_projection(local_url: &str) -> (ManagedProjection, u64) {
    let mut projection = ManagedProjection::new(local_url, true);
    let generation = projection.begin_start();
    (projection, generation)
}

#[test]
fn running_reconnect_replaces_origin_but_stopping_and_terminal_cannot() {
    let mut projection = ManagedProjection::new("http://127.0.0.1:41955", true);
    let generation = projection.begin_start();
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://first.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Changed
    ));
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://first.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Unchanged
    ));
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://second.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Changed
    ));
    assert_eq!(
        projection.status().public_url,
        "https://second.trycloudflare.com"
    );
    let ready = projection
        .ready_snapshot()
        .unwrap_or_else(|| panic!("running generation must expose one ready snapshot"));
    assert_eq!(ready.local_url, "http://127.0.0.1:41955");
    assert_eq!(ready.public_url, "https://second.trycloudflare.com");
    projection.begin_stop(generation);
    assert!(projection.ready_snapshot().is_none());
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://third.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Rejected
    ));
    projection.finish(
        generation,
        GenerationOutcome {
            error: Some("child exited".to_owned()),
            cleanup_failed: false,
        },
    );
    assert!(projection.status().public_url.is_empty());
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://error.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Rejected
    ));
    projection.begin_stop(generation);
    assert_eq!(projection.status().tunnel.phase, "error");

    let mut stopped = ManagedProjection::new("http://127.0.0.1:41955", true);
    let stopped_generation = stopped.begin_start();
    assert!(matches!(
        stopped.ready_managed(
            stopped_generation,
            origin("https://before-stop.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Changed
    ));
    stopped.finish(
        stopped_generation,
        GenerationOutcome {
            error: None,
            cleanup_failed: false,
        },
    );
    assert!(stopped.status().public_url.is_empty());
    assert!(matches!(
        stopped.ready_managed(
            stopped_generation,
            origin("https://stopped.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Rejected
    ));
    assert_eq!(stopped.status().tunnel.phase, "stopped");
}

#[test]
fn identity_authorization_keeps_the_origin_from_its_trust_snapshot() {
    let mut projection = ManagedProjection::new("http://127.0.0.1:41955", true);
    let generation = projection.begin_start();
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://first.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Changed
    ));
    let authorization = projection
        .authorize(&trusted_headers(), RouteExposure::IdentityProbePublic)
        .unwrap_or_else(|| panic!("managed identity request must authorize"));
    projection.finish(
        generation,
        GenerationOutcome {
            error: None,
            cleanup_failed: false,
        },
    );
    let next = projection.begin_start();
    assert!(matches!(
        projection.ready_managed(
            next,
            origin("https://second.trycloudflare.com"),
            "different.origin.invalid",
        ),
        ManagedReadiness::Changed
    ));
    let PublicIngressAuthorization::Identity(snapshot) = authorization else {
        panic!("identity exposure must carry its verified origin");
    };
    assert_eq!(snapshot.as_str(), "https://first.trycloudflare.com");
}

#[test]
fn stable_readiness_requires_the_same_direct_target_and_lifecycle() {
    let mut projection = ManagedProjection::new("http://127.0.0.1:41955", true);
    let generation = projection.begin_start();
    assert!(!stable_target_matches_direct(None, &projection.status()));
    assert!(matches!(
        projection.ready_managed(
            generation,
            origin("https://current.trycloudflare.com"),
            "secret.origin.invalid",
        ),
        ManagedReadiness::Changed
    ));
    let running = projection.status();
    assert!(stable_target_matches_direct(
        Some("https://current.trycloudflare.com"),
        &running
    ));
    assert!(!stable_target_matches_direct(
        Some("https://retired.trycloudflare.com"),
        &running
    ));
    projection.finish(
        generation,
        GenerationOutcome {
            error: Some("child exited".to_owned()),
            cleanup_failed: false,
        },
    );
    assert!(stable_target_matches_direct(None, &projection.status()));
}

#[tokio::test]
async fn cancelled_stop_keeps_the_generation_handle_owned() {
    let cancellation = CancellationToken::new();
    let owner = tokio::spawn(std::future::pending());
    let ingress = managed(Some(ActiveGeneration {
        number: 1,
        cancellation: cancellation.clone(),
        owner,
    }))
    .await;
    let stop_ingress = ingress.clone();
    let stop = tokio::spawn(async move { stop_ingress.stop(sequence(1)).await });
    tokio::time::timeout(Duration::from_secs(1), cancellation.cancelled())
        .await
        .unwrap_or_else(|_| panic!("stop did not cancel the generation"));
    stop.abort();
    let _ = stop.await;

    let PublicIngressKind::Managed(managed) = ingress.0.as_ref() else {
        unreachable!();
    };
    let mut lifecycle = managed.controller.lifecycle.lock().await;
    let generation = lifecycle
        .active
        .take()
        .unwrap_or_else(|| panic!("cancelled stop lost the generation handle"));
    generation.owner.abort();
    let _ = generation.owner.await;
}

#[tokio::test]
async fn shutdown_closes_start_and_cleanup_failure_blocks_restart() {
    let closed = managed(None).await;
    closed
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("close empty lifecycle: {error}"));
    assert!(matches!(
        closed.start(sequence(1)).await,
        Err(PublicIngressControlError::Closed)
    ));

    let failed = managed(None).await;
    let PublicIngressKind::Managed(managed) = failed.0.as_ref() else {
        unreachable!();
    };
    managed.projection.write().cleanup_failed = true;
    assert!(matches!(
        failed.start(sequence(1)).await,
        Err(PublicIngressControlError::CleanupFailed)
    ));
}

async fn managed(active: Option<ActiveGeneration>) -> PublicIngress {
    let stable_entry = crate::stable_entry::StableEntry::new(None, Path::new("."))
        .await
        .unwrap_or_else(|error| panic!("build unconfigured stable entry: {error}"));
    PublicIngress(Arc::new(PublicIngressKind::Managed(ManagedPublicIngress {
        projection: Arc::new(RwLock::new(ManagedProjection::new(
            "http://127.0.0.1:41955",
            true,
        ))),
        controller: ManagedController {
            config: ManagedIngressConfig {
                local_url: "http://127.0.0.1:41955".to_owned(),
                cloudflared: None,
                stable_entry,
            },
            lifecycle: Mutex::new(ManagedLifecycle {
                closed: false,
                latest_control_sequence: None,
                active,
            }),
        },
    })))
}

fn sequence(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap_or_else(|| panic!("control sequence must be nonzero"))
}

fn origin(value: &str) -> CanonicalPublicOrigin {
    CanonicalPublicOrigin::parse(value).unwrap_or_else(|error| panic!("parse test origin: {error}"))
}

fn trusted_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HOST,
        HeaderValue::from_static("secret.origin.invalid"),
    );
    headers.insert(
        "x-forwarded-host",
        HeaderValue::from_static("first.trycloudflare.com"),
    );
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    headers
}
