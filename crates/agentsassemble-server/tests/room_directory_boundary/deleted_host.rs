use super::*;

#[tokio::test]
async fn fresh_local_registration_is_explicit_tuple_bound_and_never_clears_permanent_retirement()
-> Result<(), Box<dyn std::error::Error>> {
    let store = zero_room_fixture().await;
    let id = store.host_identity().await?.server_id().to_string();
    store.set_registration_epoch(Some("old")).await?;
    store
        .account_deleted_host("http://127.0.0.1:1", "per_old_owner", "old")
        .await?;
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let server = start_deleted(store.clone(), tickets.clone()).await;
    let client = Client::new();
    let route = format!(
        "{}/api/central-directory/registration-proof",
        server.base_url
    );
    let post =
        |body: Value, ticket: String| client.post(&route).bearer_auth(ticket).json(&body).send();
    let fresh = json!({"server_id":id,"new_owner_person_id":"per_new_owner","expected_registration_epoch":"old","registration_epoch":"new"});
    assert_eq!(
        client.post(&route).json(&fresh).send().await?.status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let response = post(
        json!({"server_id":id,"registration_epoch":"new"}),
        issue_registration_ticket(&tickets).await,
    )
    .await?;
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let proof: Value = post(
        json!({"owner_person_id":"per_new_owner","new_account_registration":true}),
        issue_registration_ticket(&tickets).await,
    )
    .await?
    .json()
    .await?;
    assert!(proof.get("registration_epoch").is_none());
    assert_eq!(
        proof["host_registration_proof"]["owner_person_id"],
        "per_new_owner"
    );
    assert_eq!(store.registration_epoch().await?.as_deref(), Some("old"));
    let rejected=post(json!({"server_id":id,"new_owner_person_id":"per_new_owner","expected_registration_epoch":"wrong","registration_epoch":"new"}),issue_registration_ticket(&tickets).await).await?;
    assert_eq!(rejected.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        store.hosting_restriction().await?.as_deref(),
        Some("account_deleted")
    );
    let ticket = issue_registration_ticket(&tickets).await;
    assert_eq!(
        post(fresh.clone(), ticket.clone()).await?.status(),
        reqwest::StatusCode::OK
    );
    assert_eq!(
        post(fresh, ticket).await?.status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(store.registration_epoch().await?.as_deref(), Some("new"));
    assert!(store.hosting_restriction().await?.is_none());
    store.restrict_hosting(true).await?;
    let proof = post(
        json!({"owner_person_id":"per_another_owner","new_account_registration":true}),
        issue_registration_ticket(&tickets).await,
    )
    .await?;
    assert_eq!(proof.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        store.hosting_restriction().await?.as_deref(),
        Some("retired")
    );
    server.stop().await;
    Ok(())
}

async fn start_deleted(store: SqliteStore, tickets: TicketStore) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("bind: {e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| panic!("address: {e}"));
    let state = AppState::local(
        store,
        tickets,
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|e| panic!("state: {e}"))
    .with_central_registration()
    .with_central_directory("http://127.0.0.1:1")
    .unwrap_or_else(|e| panic!("directory: {e}"));
    let cancellation = CancellationToken::new();
    let shutdown = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, shutdown, async { Ok(()) })
            .await
            .unwrap_or_else(|e| panic!("serve: {e}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        cancellation,
        task,
    }
}
