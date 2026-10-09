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
        let offline = requests.recv().await.ok_or("offline")?;
        assert_eq!(offline.1, Method::DELETE);
        assert_eq!(offline.2["origin"], "");
        assert!(
            offline.2["generation"]
                .as_i64()
                .ok_or("offline generation")?
                > generation
        );
        assert!(requests.try_recv().is_err());
        http_cancel.cancel();
        http.await??;
    }
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
