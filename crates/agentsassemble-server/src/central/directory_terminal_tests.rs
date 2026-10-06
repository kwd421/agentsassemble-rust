use super::*;
use axum::{Router, routing::any};

#[tokio::test]
async fn terminal_responses_preserve_variants_and_stop_further_publication()
-> Result<(), Box<dyn std::error::Error>> {
    for (code, epoch, terminal) in [
        ("server_retired", Some("stored-epoch"), true),
        ("server_retired", None, true),
        ("registration_absent", Some("stored-epoch"), true),
        ("registration_absent", None, false),
        ("other_rejection", Some("stored-epoch"), false),
    ] {
        let store = SqliteStore::open("sqlite::memory:").await?;
        store
            .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "Owner")
            .await?;
        store.set_registration_epoch(epoch).await?;
        let identity = CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
        let reply = json!({"error": {"code": code, "server_id": identity.server_id(), "registration_epoch": epoch.unwrap_or("retired-epoch")}});
        let (tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let app = Router::new().route(
            "/{*path}",
            any(move || {
                let (reply, tx) = (reply.clone(), tx.clone());
                async move {
                    let _ = tx.send(());
                    (StatusCode::GONE, axum::Json(reply))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let directory =
            CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
        let cancellation = CancellationToken::new();
        let shutdown = cancellation.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        let ingress = PublicIngress::configured_manual(
            "127.0.0.1:41955".parse()?,
            "https://host.test",
            &"s".repeat(32),
        )?;
        directory.bind_ingress(ingress.clone());
        let inner = directory.0.as_ref().ok_or("directory disabled")?;
        let result = publish_online(inner, &store, &identity, "https://host.test", false).await;
        match code {
            "server_retired" => {
                assert!(matches!(result, Err(CentralDirectoryError::ServerRetired)));
            }
            "registration_absent" if epoch.is_some() => assert!(matches!(
                result,
                Err(CentralDirectoryError::RegistrationAbsent)
            )),
            _ => assert!(matches!(result, Err(CentralDirectoryError::Rejected))),
        }
        requests.try_recv()?;
        assert_eq!(ingress.ready_snapshot().is_none(), terminal);
        assert_eq!(store.hosting_restriction().await?.is_some(), terminal);
        assert_eq!(store.registration_epoch().await?.as_deref(), epoch);
        if terminal {
            assert!(publish_offline(inner, &store, &identity).await.is_err());
            assert!(requests.try_recv().is_err());
        }
        cancellation.cancel();
        server.await??;
    }
    Ok(())
}

#[tokio::test]
async fn demotion_is_durable_idempotent_and_cannot_be_downgraded()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let url = format!(
        "sqlite://{}?mode=rwc",
        root.path().join("host.sqlite").display()
    );
    let store = SqliteStore::open(&url).await?;
    let ingress = PublicIngress::disabled();
    demote_host(&store, &ingress, false).await?;
    assert_eq!(
        store.hosting_restriction().await?.as_deref(),
        Some("device")
    );
    demote_host(&store, &ingress, true).await?;
    demote_host(&store, &ingress, true).await?;
    demote_host(&store, &ingress, false).await?;
    store.close().await?;
    let reopened = SqliteStore::open(&url).await?;
    assert_eq!(
        reopened.hosting_restriction().await?.as_deref(),
        Some("retired")
    );
    Ok(())
}
