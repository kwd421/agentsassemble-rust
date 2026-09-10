use super::*;
use agentsassemble_server::RoomAttendeeClient;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn paired_companion_packet_retains_device_origin_and_revocable_parent() -> TestResult {
    let server = PairingServer::start().await;
    let client = Client::new();
    let created =
        create_with_boundary_checks(&client, &server.state, &server.base, &server.authority).await;
    let token = created["pairing_url"]
        .as_str()
        .ok_or("pair URL")?
        .strip_prefix(&format!("{ORIGIN}/pair?token="))
        .ok_or("pair origin")?;
    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x73; 32]));
    let session = redeem_with_boundary_checks(
        &client,
        &server.base,
        &json!({"pairing_token":token}),
        &device,
    )
    .await;
    let endpoint = format!("{}/api/room-attendee/companion-invite", server.base);
    let request =
        json!({"request_id":Uuid::new_v4(),"provider":"codex","display_name":"Paired companion"});
    assert_companion_boundaries(&client, &endpoint, &session, &device, &request).await?;
    let issue = |body: &Value| {
        public(client.post(&endpoint))
            .bearer_auth(&session)
            .header("x-device-token", &device)
            .json(body)
    };
    let response = issue(&request).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let packet: Value = response.json().await?;
    assert_eq!(packet["room_uid"], server.authority["room_uid"]);
    assert_eq!(packet, issue(&request).send().await?.json::<Value>().await?);
    let pending_request =
        json!({"request_id":Uuid::new_v4(),"provider":"codex","display_name":"Pending companion"});
    let pending: Value = issue(&pending_request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut attendee = packet_client(&server, &packet)?;
    let admitted = attendee.join().await?;
    assert_eq!(admitted.provider_kind, "codex_live_session");
    let snapshot = server.state.store.snapshot("general", 0, 200).await?;
    assert_eq!(snapshot.agent_sessions.len(), 1);
    assert!(snapshot.agent_sessions[0].external_owned);
    client
        .post(format!("{}/api/operator-pairing/revoke", server.base))
        .bearer_auth(operator_ticket(&server.state).await)
        .json(&json!({"authority":server.authority,"pairing_id":created["pairing_id"]}))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(
        issue(&request).send().await?.status(),
        StatusCode::FORBIDDEN
    );
    assert!(attendee.connect().await.is_err());
    let mut unused = packet_client(&server, &pending)?;
    assert!(unused.join().await.is_err());
    // Parent revocation denies ordinary access but does not discard cleanup custody.
    agentsassemble_server::shutdown_attendee(&attendee, None, None).await?;
    assert_eq!(
        server
            .state
            .store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .len(),
        1
    );
    server.shutdown.cancel();
    server.running.await??;
    Ok(())
}

async fn assert_companion_boundaries(
    client: &Client,
    endpoint: &str,
    session: &str,
    device: &str,
    request: &Value,
) -> TestResult {
    let other = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x74; 32]));
    for candidate in [None, Some(other.as_str())] {
        let mut call = public(client.post(endpoint))
            .bearer_auth(session)
            .json(request);
        if let Some(candidate) = candidate {
            call = call.header("x-device-token", candidate);
        }
        assert_eq!(
            call.send().await?.status(),
            if candidate.is_some() {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
    }
    let foreign = public(client.post(endpoint))
        .bearer_auth(session)
        .header("x-device-token", device)
        .header("origin", "https://foreign.example.test")
        .json(request)
        .send()
        .await?;
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    let private = client
        .post(endpoint)
        .bearer_auth(session)
        .header("x-device-token", device)
        .header("origin", ORIGIN)
        .json(request)
        .send()
        .await?;
    assert_eq!(private.status(), StatusCode::FORBIDDEN);
    Ok(())
}

fn packet_client(
    server: &PairingServer,
    packet: &Value,
) -> Result<RoomAttendeeClient, Box<dyn std::error::Error>> {
    let url = url::Url::parse(packet["join_url"].as_str().ok_or("join URL")?)?;
    assert_eq!(url.origin().ascii_serialization(), ORIGIN);
    let token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .ok_or("invite token")?
        .1
        .into_owned();
    Ok(RoomAttendeeClient::new(
        &format!("{}/join?token={token}", server.base),
        "codex",
        "Paired local AI",
    )?)
}
