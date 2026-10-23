use super::super::{CentralDirectory, StatusCode};
use super::*;
use axum::{Router, body::Bytes, extract::Path, routing::any};

#[tokio::test]
async fn idle_directory_has_no_timer_wakes_and_failure_retries_keep_generation()
-> Result<(), Box<dyn std::error::Error>> {
    for fail_online in [false, true] {
        let store = SqliteStore::open("sqlite::memory:").await?;
        store
            .bootstrap_local_authority("2d697cab-366f-4b22-82d0-2c96b67bab42", "Owner")
            .await?;
        store.set_registration_epoch(Some("event-epoch")).await?;
        let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
        let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let app = capture_requests(tx, fail_online);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let directory =
            CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
        let inner = directory.0.as_ref().ok_or("directory")?.clone();
        let mut probe = inner.publisher_probe.subscribe();
        let http_cancel = CancellationToken::new();
        let stop = http_cancel.clone();
        let http = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await
        });
        let ingress = PublicIngress::configured_manual(
            "127.0.0.1:41955".parse()?,
            "https://event.test",
            &"s".repeat(32),
        )?;
        let cancel = CancellationToken::new();
        let sender = tokio::spawn(directory.run(store.clone(), ingress, identity, cancel.clone()));
        let first = requests.recv().await.ok_or("first endpoint")?;
        assert_eq!(first.0, "endpoint");
        assert_eq!(first.1, Method::PUT);
        assert_eq!(first.2["mode"], "event_secure_v1");
        assert_eq!(first.2["account_deletion_protocol"], "v1");
        assert!(first.2.get("lease_expires_at").is_none());
        let generation = first.2["generation"].as_i64().ok_or("generation")?;
        assert_eq!(requests.recv().await.ok_or("name")?.0, "name");
        probe.wait_for(|(_, waiting)| *waiting).await?;
        if fail_online {
            for advance in [2, 6] {
                let previous = probe.borrow().0;
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(advance)).await;
                tokio::time::resume();
                let retry = requests.recv().await.ok_or("retry")?;
                assert_eq!(retry.0, "endpoint");
                assert_eq!(retry.2["generation"], generation);
                probe
                    .wait_for(|(round, waiting)| *round > previous && *waiting)
                    .await?;
            }
            let pending = store
                .pending_central_endpoint_event()
                .await?
                .ok_or("parked")?;
            assert_eq!(pending.failed_attempts, 3);
            assert_eq!(pending.next_attempt_at_ms(), None);
        } else {
            assert!(store.pending_central_endpoint_event().await?.is_none());
        }
        let before = *probe.borrow();
        tokio::time::pause();
        tokio::time::advance(Duration::from_hours(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            *probe.borrow(),
            before,
            "idle or parked publisher must not scan on a timer"
        );
        assert!(
            requests.try_recv().is_err(),
            "no heartbeat or central health writes"
        );
        tokio::time::resume();
        cancel.cancel();
        sender.await??;
        // Quit publishes nothing: the dead origin is liveness, revoked
        // implicitly when the restart PUT replaces the row.
        assert!(requests.try_recv().is_err());
        http_cancel.cancel();
        http.await??;
    }
    Ok(())
}

#[test]
fn endpoint_reserve_rule_keeps_explicit_stop_revocation_only() {
    let live = (
        Some("event-epoch".to_owned()),
        (4_u64, "https://fourth.trycloudflare.com".to_owned()),
    );
    // A live origin always reserves.
    assert!(super::should_reserve_endpoint_event(
        None,
        "event-epoch",
        "https://first.trycloudflare.com"
    ));
    assert!(super::should_reserve_endpoint_event(
        Some(&live),
        "event-epoch",
        "https://fifth.trycloudflare.com"
    ));
    // Cold boot, fresh epoch, or repeat empty reserves nothing: liveness is
    // not a revocation.
    assert!(!super::should_reserve_endpoint_event(
        None,
        "event-epoch",
        ""
    ));
    assert!(!super::should_reserve_endpoint_event(
        Some(&live),
        "replacement-epoch",
        ""
    ));
    assert!(!super::should_reserve_endpoint_event(
        Some(&(Some("event-epoch".to_owned()), (5_u64, String::new()))),
        "event-epoch",
        ""
    ));
    // Explicit owner close revokes the same-epoch origin this run observed.
    assert!(super::should_reserve_endpoint_event(
        Some(&live),
        "event-epoch",
        ""
    ));
}

#[tokio::test]
async fn cold_start_with_closed_ingress_publishes_nothing() -> Result<(), Box<dyn std::error::Error>>
{
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("2d697cab-366f-4b22-82d0-2c96b67bab42", "Owner")
        .await?;
    store.set_registration_epoch(Some("event-epoch")).await?;
    let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let app = capture_requests(tx, false);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let directory = CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
    let inner = directory.0.as_ref().ok_or("directory")?.clone();
    let mut probe = inner.publisher_probe.subscribe();
    let http_cancel = CancellationToken::new();
    let stop = http_cancel.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    // Disabled ingress holds the channel boot value (0, ""): the cold start
    // marks it observed without reserving, so no endpoint event goes out.
    // The default-name publication is ingress-independent and still fires.
    let cancel = CancellationToken::new();
    let sender = tokio::spawn(directory.run(
        store.clone(),
        PublicIngress::disabled(),
        identity,
        cancel.clone(),
    ));
    probe.wait_for(|(_, waiting)| *waiting).await?;
    assert_eq!(requests.recv().await.ok_or("name")?.0, "name");
    assert!(requests.try_recv().is_err());
    assert!(store.pending_central_endpoint_event().await?.is_none());
    cancel.cancel();
    sender.await??;
    assert!(requests.try_recv().is_err());
    http_cancel.cancel();
    http.await??;
    Ok(())
}

#[tokio::test]
async fn stale_pre_quit_online_event_is_superseded_while_offline_revocation_delivers()
-> Result<(), Box<dyn std::error::Error>> {
    for (stale_origin, expect_request) in [("https://dead.trycloudflare.com", false), ("", true)] {
        let store = SqliteStore::open("sqlite::memory:").await?;
        store
            .bootstrap_local_authority("2d697cab-366f-4b22-82d0-2c96b67bab42", "Owner")
            .await?;
        store.set_registration_epoch(Some("event-epoch")).await?;
        store
            .reserve_central_endpoint_event(
                "event-epoch",
                stale_origin,
                Utc::now().timestamp_millis(),
            )
            .await?;
        let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
        let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let app = capture_requests(tx, false);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let directory =
            CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
        let inner = directory.0.as_ref().ok_or("directory")?.clone();
        let mut probe = inner.publisher_probe.subscribe();
        let http_cancel = CancellationToken::new();
        let stop = http_cancel.clone();
        let http = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await
        });
        let cancel = CancellationToken::new();
        let sender = tokio::spawn(directory.run(
            store.clone(),
            PublicIngress::disabled(),
            identity,
            cancel.clone(),
        ));
        probe.wait_for(|(_, waiting)| *waiting).await?;
        if expect_request {
            // The pending explicit-stop revocation is never dropped: the
            // endpoint DELETE goes out first, then the ingress-independent
            // default name.
            let revoked = requests.recv().await.ok_or("offline revocation")?;
            assert_eq!(revoked.0, "endpoint");
            assert_eq!(revoked.1, Method::DELETE);
            assert_eq!(revoked.2["origin"], "");
            assert_eq!(requests.recv().await.ok_or("name")?.0, "name");
        } else {
            // The pre-quit PUT names a dead origin: superseded locally, never
            // published, so the restart costs nothing yet. Only the
            // ingress-independent default name goes out.
            assert_eq!(requests.recv().await.ok_or("name")?.0, "name");
        }
        assert!(requests.try_recv().is_err());
        assert!(store.pending_central_endpoint_event().await?.is_none());
        cancel.cancel();
        sender.await??;
        http_cancel.cancel();
        http.await??;
    }
    Ok(())
}

#[tokio::test]
async fn restart_publishes_only_the_new_origin() -> Result<(), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("2d697cab-366f-4b22-82d0-2c96b67bab42", "Owner")
        .await?;
    store.set_registration_epoch(Some("event-epoch")).await?;
    let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let app = capture_requests(tx, false);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let directory = CentralDirectory::configured(&origin)?;
    let inner = directory.0.as_ref().ok_or("directory")?.clone();
    let mut probe = inner.publisher_probe.subscribe();
    let http_cancel = CancellationToken::new();
    let stop = http_cancel.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let manual = || {
        PublicIngress::configured_manual(
            "127.0.0.1:41955"
                .parse()
                .unwrap_or_else(|e| panic!("listener: {e}")),
            "https://event.test",
            &"s".repeat(32),
        )
        .unwrap_or_else(|e| panic!("ingress: {e}"))
    };
    let cancel = CancellationToken::new();
    let first_run = tokio::spawn(directory.clone().run(
        store.clone(),
        manual(),
        identity.clone(),
        cancel.clone(),
    ));
    let first = requests.recv().await.ok_or("first endpoint")?;
    assert_eq!(first.0, "endpoint");
    assert_eq!(first.1, Method::PUT);
    assert_eq!(first.2["origin"], "https://event.test");
    let first_generation = first.2["generation"].as_i64().ok_or("generation")?;
    assert_eq!(requests.recv().await.ok_or("first name")?.0, "name");
    probe.wait_for(|(_, waiting)| *waiting).await?;
    cancel.cancel();
    first_run.await??;
    // Quit publishes nothing.
    assert!(requests.try_recv().is_err());
    // Restart publishes exactly one endpoint event: the new origin PUT with
    // a strictly higher durable generation. Quiesce before asserting.
    let restart_cancel = CancellationToken::new();
    let second_run =
        tokio::spawn(directory.run(store.clone(), manual(), identity, restart_cancel.clone()));
    let second = requests.recv().await.ok_or("restart endpoint")?;
    assert_eq!(second.0, "endpoint");
    assert_eq!(second.1, Method::PUT);
    assert_eq!(second.2["origin"], "https://event.test");
    assert!(
        second.2["generation"]
            .as_i64()
            .ok_or("restart generation")?
            > first_generation
    );
    assert_eq!(requests.recv().await.ok_or("restart name")?.0, "name");
    probe.wait_for(|(_, waiting)| *waiting).await?;
    assert!(requests.try_recv().is_err());
    restart_cancel.cancel();
    second_run.await??;
    assert!(requests.try_recv().is_err());
    http_cancel.cancel();
    http.await??;
    Ok(())
}

#[tokio::test]
async fn quota_refusal_parks_all_delivery_until_utc_midnight()
-> Result<(), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("2d697cab-366f-4b22-82d0-2c96b67bab42", "Owner")
        .await?;
    store.set_registration_epoch(Some("quota-epoch")).await?;
    let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let app = Router::new().route(
        "/v1/servers/{id}/{operation}",
        any(
            move |Path((_, operation)): Path<(String, String)>,
                  method: axum::http::Method,
                  bytes: Bytes| {
                let tx = tx.clone();
                async move {
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("JSON"));
                    tx.send((operation, method, value))
                        .unwrap_or_else(|_| panic!("capture"));
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        axum::Json(json!({"error": {"code": "actor_quota_exhausted"}})),
                    )
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let directory = CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
    let status_reader = directory.clone();
    let inner = directory.0.as_ref().ok_or("directory")?.clone();
    let mut probe = inner.publisher_probe.subscribe();
    let http_cancel = CancellationToken::new();
    let stop = http_cancel.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let ingress = PublicIngress::configured_manual(
        "127.0.0.1:41955".parse()?,
        "https://quota.test",
        &"s".repeat(32),
    )?;
    let cancel = CancellationToken::new();
    let sender = tokio::spawn(directory.run(store.clone(), ingress, identity, cancel.clone()));
    let first = requests.recv().await.ok_or("first endpoint")?;
    assert_eq!(first.0, "endpoint");
    probe.wait_for(|(_, waiting)| *waiting).await?;
    let parked_until = status_reader.status().quota_parked_until;
    let midnight = (Utc::now().timestamp().div_euclid(86_400) + 1) * 86_400;
    assert!(
        (midnight..=midnight + 60).contains(&parked_until),
        "parked_until={parked_until} midnight={midnight}"
    );
    // No 60 s polling: an hour passes with no further request (the name
    // publication never fires while parked).
    tokio::time::pause();
    tokio::time::advance(Duration::from_hours(1)).await;
    tokio::task::yield_now().await;
    assert!(
        requests.try_recv().is_err(),
        "parked publisher must send nothing meanwhile"
    );
    tokio::time::resume();
    cancel.cancel();
    // Quit publishes nothing, so a spent allowance cannot fail the shutdown.
    let result = sender.await?;
    assert!(
        matches!(result, Ok(())),
        "quit sends no offline event, got {result:?}"
    );
    assert!(requests.try_recv().is_err());
    http_cancel.cancel();
    http.await??;
    Ok(())
}

#[tokio::test]
async fn per_minute_rate_limit_honours_retry_after_without_quota_park()
-> Result<(), Box<dyn std::error::Error>> {
    use axum::http::header;
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("2d697cab-366f-4b22-82d0-2c96b67bab42", "Owner")
        .await?;
    store.set_registration_epoch(Some("rate-epoch")).await?;
    let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel::<(String, serde_json::Value)>();
    let name_tx = tx.clone();
    let endpoint_tx = tx.clone();
    let app = Router::new()
        .route(
            "/v1/servers/{id}/endpoint",
            any(move || {
                let endpoint_tx = endpoint_tx.clone();
                async move {
                    endpoint_tx
                        .send(("endpoint".to_owned(), serde_json::json!({})))
                        .ok();
                    (StatusCode::OK, axum::Json(json!({"status": "ok"})))
                }
            }),
        )
        .route(
            "/v1/servers/{id}/name",
            axum::routing::put(move |body: Bytes| {
                let name_tx = name_tx.clone();
                async move {
                    let value: serde_json::Value =
                        serde_json::from_slice(&body).unwrap_or_else(|_| panic!("JSON"));
                    name_tx.send(("name".to_owned(), value)).ok();
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [(header::RETRY_AFTER, "120")],
                        axum::Json(json!({"error": {"code": "per_minute_limited"}})),
                    )
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let directory = CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
    let status_reader = directory.clone();
    let inner = directory.0.as_ref().ok_or("directory")?.clone();
    let mut probe = inner.publisher_probe.subscribe();
    let http_cancel = CancellationToken::new();
    let stop = http_cancel.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let cancel = CancellationToken::new();
    let sender = tokio::spawn(directory.run(
        store.clone(),
        PublicIngress::disabled(),
        identity,
        cancel.clone(),
    ));
    // Disabled ingress with a closed channel reserves no endpoint event;
    // only the ingress-independent name publication fires (and 429s).
    assert_eq!(requests.recv().await.ok_or("name")?.0, "name");
    probe.wait_for(|(_, waiting)| *waiting).await?;
    assert!(requests.try_recv().is_err());
    assert_eq!(status_reader.status().quota_parked_until, 0);
    // The short 2 s backoff would already have fired; Retry-After holds it.
    tokio::time::pause();
    tokio::time::advance(Duration::from_mins(1)).await;
    tokio::task::yield_now().await;
    assert!(
        requests.try_recv().is_err(),
        "Retry-After must delay the bounded retry"
    );
    tokio::time::resume();
    cancel.cancel();
    // Quit publishes nothing, so no offline publish fires into the
    // rate limit either.
    let result = sender.await?;
    assert!(
        matches!(result, Ok(())),
        "quit sends nothing, got {result:?}"
    );
    assert!(requests.try_recv().is_err());
    http_cancel.cancel();
    http.await??;
    Ok(())
}

fn capture_requests(
    tx: tokio::sync::mpsc::UnboundedSender<(String, Method, serde_json::Value)>,
    fail_online: bool,
) -> Router {
    Router::new().route(
        "/v1/servers/{id}/{operation}",
        any(
            move |Path((_, operation)): Path<(String, String)>,
                  method: axum::http::Method,
                  bytes: Bytes| {
                let tx = tx.clone();
                async move {
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("JSON"));
                    let fail =
                        fail_online && operation == "endpoint" && method == axum::http::Method::PUT;
                    tx.send((operation, method, value))
                        .unwrap_or_else(|_| panic!("capture"));
                    (
                        if fail {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::OK
                        },
                        axum::Json(json!({"status":"ok"})),
                    )
                }
            },
        ),
    )
}

#[tokio::test]
async fn account_deleted_publisher_parks_without_time_wakes_and_fresh_epoch_resumes_same_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("b3cdc3ab-c124-41db-af1c-61a48f19e11f", "Independent")
        .await?;
    store.set_registration_epoch(Some("old")).await?;
    let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let app = capture_requests(tx, false);
    let http_cancel = CancellationToken::new();
    let http_stop = http_cancel.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(http_stop.cancelled_owned())
            .await
    });
    let directory = CentralDirectory::configured(&origin)?;
    let inner = directory.0.as_ref().ok_or("directory")?.clone();
    let mut probe = inner.publisher_probe.subscribe();
    let ingress = PublicIngress::configured_manual(
        "127.0.0.1:41955".parse()?,
        "https://resume.test",
        &"s".repeat(32),
    )?;
    let cancel = CancellationToken::new();
    let sender =
        tokio::spawn(directory.run(store.clone(), ingress.clone(), identity, cancel.clone()));
    assert_eq!(requests.recv().await.ok_or("endpoint")?.0, "endpoint");
    assert_eq!(requests.recv().await.ok_or("name")?.0, "name");
    probe.wait_for(|(_, waiting)| *waiting).await?;
    let before = probe.borrow().0;
    store
        .account_deleted_host(&origin, "old-person", "old")
        .await?;
    probe
        .wait_for(|(round, waiting)| *round > before && *waiting)
        .await?;
    let parked = *probe.borrow();
    tokio::time::pause();
    tokio::time::advance(Duration::from_hours(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(*probe.borrow(), parked);
    assert!(requests.try_recv().is_err());
    if sender.is_finished() {
        return Err(format!(
            "publisher finished while parked: {:?}, restriction={:?}",
            sender.await?,
            store.hosting_restriction().await?
        )
        .into());
    }
    tokio::time::resume();
    let state = crate::AppState::local(
        store.clone(),
        crate::TicketStore::new(Duration::from_secs(30), 32),
        agentsassemble_provider::ProviderCatalogService::fixed(
            agentsassemble_domain::ProviderCatalog::default(),
        ),
    )
    .await?;
    crate::member_removal_runtime::reconcile(
        &store,
        &state.provider_adapter,
        &state.rooms,
        &state.owner_sessions,
        &state.shutdown,
    )
    .await?;
    store
        .register_after_account_deletion(&origin, "fresh-person", "old", "fresh")
        .await?;
    ingress.resume_after_account_deletion().await?;
    let endpoint = tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await?
        .ok_or("fresh endpoint")?;
    assert_eq!(endpoint.0, "endpoint");
    assert_eq!(endpoint.2["registration_epoch"], "fresh");
    assert_eq!(
        requests.recv().await.ok_or("fresh name")?.2["registration_epoch"],
        "fresh"
    );
    cancel.cancel();
    sender.await??;
    http_cancel.cancel();
    http.await??;
    Ok(())
}
