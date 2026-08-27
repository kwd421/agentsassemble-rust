use std::{sync::Arc, time::Duration};

use axum::http::{HeaderMap, HeaderValue, header};
use parking_lot::RwLock;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    ActiveGeneration, CanonicalPublicOrigin, GenerationOutcome, ManagedController,
    ManagedIngressConfig, ManagedLifecycle, ManagedProjection, ManagedPublicIngress, PublicIngress,
    PublicIngressAuthorization, PublicIngressControlError, PublicIngressKind,
};
use crate::product_surface::RouteExposure;

#[test]
fn running_reconnect_replaces_origin_but_stopping_and_terminal_cannot() {
    let mut projection = ManagedProjection::new("http://127.0.0.1:41955", true);
    let generation = projection.begin_start();
    assert!(projection.ready_managed(
        generation,
        origin("https://first.trycloudflare.com"),
        "secret.origin.invalid",
    ));
    assert!(projection.ready_managed(
        generation,
        origin("https://second.trycloudflare.com"),
        "secret.origin.invalid",
    ));
    assert_eq!(
        projection.status().public_url,
        "https://second.trycloudflare.com"
    );
    projection.begin_stop(generation);
    assert!(!projection.ready_managed(
        generation,
        origin("https://third.trycloudflare.com"),
        "secret.origin.invalid",
    ));
    projection.finish(
        generation,
        GenerationOutcome {
            error: Some("child exited".to_owned()),
            cleanup_failed: false,
        },
    );
    assert!(projection.status().public_url.is_empty());
    assert!(!projection.ready_managed(
        generation,
        origin("https://error.trycloudflare.com"),
        "secret.origin.invalid",
    ));
    projection.begin_stop(generation);
    assert_eq!(projection.status().tunnel.phase, "error");

    let mut stopped = ManagedProjection::new("http://127.0.0.1:41955", true);
    let stopped_generation = stopped.begin_start();
    assert!(stopped.ready_managed(
        stopped_generation,
        origin("https://before-stop.trycloudflare.com"),
        "secret.origin.invalid",
    ));
    stopped.finish(
        stopped_generation,
        GenerationOutcome {
            error: None,
            cleanup_failed: false,
        },
    );
    assert!(stopped.status().public_url.is_empty());
    assert!(!stopped.ready_managed(
        stopped_generation,
        origin("https://stopped.trycloudflare.com"),
        "secret.origin.invalid",
    ));
    assert_eq!(stopped.status().tunnel.phase, "stopped");
}

#[test]
fn identity_authorization_keeps_the_origin_from_its_trust_snapshot() {
    let mut projection = ManagedProjection::new("http://127.0.0.1:41955", true);
    let generation = projection.begin_start();
    assert!(projection.ready_managed(
        generation,
        origin("https://first.trycloudflare.com"),
        "secret.origin.invalid",
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
    assert!(projection.ready_managed(
        next,
        origin("https://second.trycloudflare.com"),
        "different.origin.invalid",
    ));
    let PublicIngressAuthorization::Identity(snapshot) = authorization else {
        panic!("identity exposure must carry its verified origin");
    };
    assert_eq!(snapshot.as_str(), "https://first.trycloudflare.com");
}

#[tokio::test]
async fn cancelled_stop_keeps_the_generation_handle_owned() {
    let cancellation = CancellationToken::new();
    let owner = tokio::spawn(std::future::pending());
    let ingress = managed(Some(ActiveGeneration {
        number: 1,
        cancellation: cancellation.clone(),
        owner,
    }));
    let stop_ingress = ingress.clone();
    let stop = tokio::spawn(async move { stop_ingress.stop().await });
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
    let closed = managed(None);
    closed
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("close empty lifecycle: {error}"));
    assert!(matches!(
        closed.start().await,
        Err(PublicIngressControlError::Closed)
    ));

    let failed = managed(None);
    let PublicIngressKind::Managed(managed) = failed.0.as_ref() else {
        unreachable!();
    };
    managed.projection.write().cleanup_failed = true;
    assert!(matches!(
        failed.start().await,
        Err(PublicIngressControlError::CleanupFailed)
    ));
}

fn managed(active: Option<ActiveGeneration>) -> PublicIngress {
    PublicIngress(Arc::new(PublicIngressKind::Managed(ManagedPublicIngress {
        projection: Arc::new(RwLock::new(ManagedProjection::new(
            "http://127.0.0.1:41955",
            true,
        ))),
        controller: ManagedController {
            config: ManagedIngressConfig {
                local_url: "http://127.0.0.1:41955".to_owned(),
                cloudflared: None,
            },
            lifecycle: Mutex::new(ManagedLifecycle {
                closed: false,
                active,
            }),
        },
    })))
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
