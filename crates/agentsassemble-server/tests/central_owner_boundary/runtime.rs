use super::*;

/// The host runtime shape every start uses: the local authority, the manual
/// public ingress the owner routes are reached through, and the fixed central
/// directory that owns host publication.
pub(super) async fn state(store: SqliteStore, address: SocketAddr, worker_url: &str) -> AppState {
    AppState::local(
        store,
        // Match the runtime capacity: small stores reserve all grants for private use.
        TicketStore::new(Duration::from_secs(30), 4096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("host state: {error:?}"))
    .with_manual_public_ingress(address, ORIGIN, SECRET)
    .unwrap_or_else(|error| panic!("public ingress: {error:?}"))
    .with_central_directory(worker_url)
    .unwrap_or_else(|error| panic!("central directory: {error:?}"))
}

pub(super) fn spawn(
    listener: TcpListener,
    runtime_state: AppState,
) -> (CancellationToken, JoinHandle<()>) {
    let cancel = CancellationToken::new();
    let owner_cancel = cancel.clone();
    let host_task = tokio::spawn(async move {
        serve(listener, runtime_state, owner_cancel, async { Ok(()) })
            .await
            .unwrap_or_else(|error| panic!("host serve: {error:?}"));
    });
    (cancel, host_task)
}

/// The central endpoint publication contract of that runtime: the live origin
/// is published once at start, quitting publishes nothing, and a restart
/// republishes it with a strictly higher durable generation.
///
/// Revocation is not a quit: an explicit owner close of external access
/// publishes the DELETE, and the managed-ingress owner covers that path.
pub(super) async fn verify_publication(mut fixture: Fixture, generation: i64, public_key: &[u8]) {
    fixture.cancel.cancel();
    fixture
        .host_task
        .await
        .unwrap_or_else(|error| panic!("host join: {error:?}"));
    // Quitting the host publishes nothing: the dead public origin is liveness
    // only, and central revokes it implicitly when the restart replaces the
    // row. Only an explicit owner close of external access revokes with a
    // DELETE, so a quit must not spend an endpoint event.
    assert!(
        matches!(
            fixture.calls.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ),
        "quitting the host must not publish an offline endpoint event"
    );

    // A restart costs exactly one endpoint event: the new origin PUT with a
    // strictly higher durable generation, so a stale event can never clear a
    // newer row and the dead origin is never left published.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("restart listener: {error:?}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("restart address: {error:?}"));
    let runtime_state = state(fixture.store.clone(), address, &fixture.worker_state.issuer).await;
    let (cancel, task) = spawn(listener, runtime_state);
    let republished = next_call(&mut fixture.calls).await;
    assert_eq!(republished.method, Method::PUT);
    let republished_body: Value = serde_json::from_slice(&republished.body)
        .unwrap_or_else(|error| panic!("republished body: {error:?}"));
    assert_eq!(republished_body["origin"], ORIGIN);
    let republished_generation = republished_body["generation"]
        .as_i64()
        .unwrap_or_else(|| panic!("republished generation"));
    assert!(republished_generation > generation);
    verify_signed_call(&republished, public_key);
    cancel.cancel();
    task.await
        .unwrap_or_else(|error| panic!("host restart: {error:?}"));
    assert!(
        matches!(
            fixture.calls.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ),
        "a host restart must publish exactly one endpoint event"
    );
    fixture.worker_task.abort();
    let _ = fixture.worker_task.await;
}
