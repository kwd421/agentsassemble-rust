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
    start_with_directory(store, tickets, "http://127.0.0.1:1").await
}

async fn start_with_directory(
    store: SqliteStore,
    tickets: TicketStore,
    central: &str,
) -> RunningServer {
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
    .with_central_directory(central)
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

#[tokio::test]
async fn private_own_stop_uses_signed_actual_owner_and_retries_completed_custody_without_network()
-> Result<(), Box<dyn std::error::Error>> {
    use axum::{Router, body::Bytes, http::HeaderMap, routing::post};
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let store = zero_room_fixture().await;
    let persistent = store.host_identity().await?;
    let id = persistent.server_id().to_string();
    let signer =
        Ed25519KeyPair::from_pkcs8(persistent.private_key_pkcs8()).map_err(|_| "fixture key")?;
    let public = signer.public_key().as_ref().to_vec();
    store.set_registration_epoch(Some("stop-epoch")).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let expected_id = id.clone();
    let central = Router::new().route(&format!("/v1/servers/{id}/owner"), post(move |headers: HeaderMap, bytes: Bytes| {
        let public = public.clone(); let count = count.clone(); let id = expected_id.clone();
        async move {
            let body: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("owner body"));
            assert_eq!(body, json!({"registration_epoch":"stop-epoch"}));
            let timestamp = headers["x-aa-host-timestamp"].to_str().unwrap_or_else(|_| panic!("timestamp"));
            let nonce = headers["x-aa-host-nonce"].to_str().unwrap_or_else(|_| panic!("nonce"));
            let signature = URL_SAFE_NO_PAD.decode(headers["x-aa-host-signature"].as_bytes()).unwrap_or_else(|_| panic!("signature"));
            let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes));
            let transcript = format!("AA-HOST-1\nPOST\n/v1/servers/{id}/owner\n{timestamp}\n{nonce}\n{hash}");
            assert!(UnparsedPublicKey::new(&ED25519, public).verify(transcript.as_bytes(), &signature).is_ok());
            count.fetch_add(1, Ordering::SeqCst);
            axum::Json(json!({"server_id":id,"registration_epoch":"stop-epoch","owner_person_id":"per_actual_owner"}))
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let http_cancel = CancellationToken::new();
    let http_stop = http_cancel.clone();
    let http = tokio::spawn(async move {
        axum::serve(listener, central)
            .with_graceful_shutdown(http_stop.cancelled_owned())
            .await
    });
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let server = start_with_directory(store.clone(), tickets.clone(), &origin).await;
    let client = Client::new();
    let route = format!(
        "{}/api/central-directory/registration-proof",
        server.base_url
    );
    let body = json!({"server_id":id,"account_deletion":true,"expected_owner_person_id":"per_actual_owner","registration_epoch":"stop-epoch"});
    assert_eq!(
        client.post(&route).json(&body).send().await?.status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let mut wrong = body.clone();
    wrong["expected_owner_person_id"] = "per_other_person".into();
    assert_eq!(
        client
            .post(&route)
            .bearer_auth(issue_registration_ticket(&tickets).await)
            .json(&wrong)
            .send()
            .await?
            .status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(store.hosting_restriction().await?.is_none());
    for _ in 0..2 {
        let reply = client
            .post(&route)
            .bearer_auth(issue_registration_ticket(&tickets).await)
            .json(&body)
            .send()
            .await?;
        assert_eq!(reply.status(), reqwest::StatusCode::OK);
        assert_eq!(
            reply.json::<Value>().await?,
            json!({"status":"account_host_stopped"})
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        store.hosting_restriction().await?.as_deref(),
        Some("account_deleted")
    );
    let key = store
        .account_deleted_host_job(&origin, "per_actual_owner", "stop-epoch")
        .await?
        .ok_or("stop custody")?;
    assert_eq!(store.member_removal_phase(&key).await?, "complete");
    assert_eq!(store.local_operator_profile().await?.display_name, "SeiNel");
    server.stop().await;
    http_cancel.cancel();
    http.await??;
    Ok(())
}
