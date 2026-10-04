use super::{connection::*, invitations::authorized, *};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type TestResult = Result<(), Box<dyn std::error::Error>>;
async fn native(fixture: &Fixture, body: Option<Value>) -> reqwest::Response {
    let proof = fixture
        .tickets
        .issue_server_operator(agentsassemble_domain::LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("native proof: {error}"));
    let builder = if let Some(body) = body {
        fixture
            .client
            .post(format!(
                "http://{}/api/owner-sessions/revoke",
                fixture.address
            ))
            .json(&body)
    } else {
        fixture
            .client
            .get(format!("http://{}/api/owner-sessions", fixture.address))
    };
    builder
        .bearer_auth(proof.ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("native request: {error}"))
}

async fn pair_and_companions(
    fixture: &mut Fixture,
    generation: i64,
) -> Result<(Value, Value, String), Box<dyn std::error::Error>> {
    let created = fixture
        .store
        .create_room_for_local_operator(
            "10000000-0000-4000-8000-000000000031",
            "general",
            "General",
        )
        .await?;
    fixture.room_uid = created.room.room_uid.to_string();
    let room: Value = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/room",
        json!({"session_token": fixture.session_token, "generation": generation,
            "room_id": "general", "room_uid": fixture.room_uid}),
        ORIGIN,
        DEVICE,
    )
    .await
    .error_for_status()?
    .json()
    .await?;
    let base = format!("http://{}", fixture.address);
    let pairing: Value = authorized(fixture.client.post(format!("{base}/api/central-owner/operator-pairing/create")),
        room["session_token"].as_str().ok_or("root room bearer")?, DEVICE)
        .json(&json!({"server_id": room["server_id"], "authority_lineage_id": room["authority_lineage_id"],
            "room_id": "general", "room_uid": room["room_uid"]}))
        .send().await?.error_for_status()?.json().await?;
    let pairing_url = url::Url::parse(pairing["pairing_url"].as_str().ok_or("pair URL")?)?;
    let token = pairing_url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .ok_or("pair token")?
        .1
        .into_owned();
    let paired: Value = authorized(fixture.client.post(format!("{base}/api/operator-pairing/redeem")), "", DEVICE)
        .json(&json!({"pairing_token": token, "device": {"device_name":"Firefox · Linux", "browser":"Firefox", "os":"Linux"}}))
        .send().await?.error_for_status()?.json().await?;
    let bearer = paired["session_token"].as_str().ok_or("paired bearer")?;
    let mut packets = Vec::new();
    for request in [
        "10000000-0000-4000-8000-000000000032",
        "10000000-0000-4000-8000-000000000033",
    ] {
        let packet: Value = authorized(
            fixture
                .client
                .post(format!("{base}/api/room-attendee/companion-invite")),
            bearer,
            DEVICE,
        )
        .json(&json!({"request_id":request, "provider":"codex", "display_name":"Device companion"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
        packets.push(packet);
    }
    Ok((packets.remove(0), packets.remove(0), bearer.to_owned()))
}

fn invite_token(packet: &Value) -> Result<String, Box<dyn std::error::Error>> {
    Ok(
        url::Url::parse(packet["join_url"].as_str().ok_or("join URL")?)?
            .query_pairs()
            .find(|(key, _)| key == "token")
            .ok_or("invite token")?
            .1
            .into_owned(),
    )
}

async fn join(
    fixture: &Fixture,
    packet: &Value,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    Ok(authorized(fixture.client.post(format!("http://{}/api/room-attendee/join", fixture.address)), &invite_token(packet)?, DEVICE)
        .json(&json!({"request_id":"10000000-0000-4000-8000-000000000034", "client_secret":URL_SAFE_NO_PAD.encode([9; 32]),
            "provider":"codex", "display_name":"Device companion"})).send().await?)
}

async fn attendee(fixture: &Fixture, packet: &Value) -> Result<Peer, Box<dyn std::error::Error>> {
    let joined: Value = join(fixture, packet)
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut request =
        format!("ws://{}/api/room-attendee/ws", fixture.address).into_client_request()?;
    for (name, value) in [
        ("host", "owner.example.test"),
        ("x-forwarded-proto", "https"),
        ("x-agentsassemble-proxy-token", SECRET),
        ("origin", ORIGIN),
    ] {
        request.headers_mut().insert(name, value.parse()?);
    }
    request.headers_mut().insert(
        "authorization",
        format!(
            "Bearer {}",
            joined["session_bearer"].as_str().ok_or("attendee bearer")?
        )
        .parse()?,
    );
    let (wire, _) = tokio_tungstenite::connect_async(request).await?;
    let mut peer = Peer::new(wire);
    assert_eq!(peer.receive_json().await["type"], "connected");
    Ok(peer)
}

#[tokio::test]
async fn native_all_revokes_idle_owner_paired_device_and_companion_and_retains_host() -> TestResult
{
    let (mut fixture, generation) = admitted_fixture().await;
    let mut root = stream(&fixture, generation).await;
    let (packet, pending, bearer) = pair_and_companions(&mut fixture, generation).await?;
    let mut companion = attendee(&fixture, &packet).await?;
    let mut paired = socket_for_session(&fixture, &bearer).await;
    let listing: Value = native(&fixture, None)
        .await
        .error_for_status()?
        .json()
        .await?;
    let rows = listing["sessions"].as_array().ok_or("native sessions")?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.iter().filter(|row| row["current"] == true).count(), 1);
    assert_eq!(rows[0]["session_id"], "host");
    assert_eq!(rows[0]["revocable"], false);
    let device = rows
        .iter()
        .find(|row| row["kind"] == "pairing")
        .ok_or("paired device")?;
    assert_eq!(device["browser"], "Firefox");
    assert_eq!(device["os"], "Linux");
    assert!(device["last_connected_at"].is_i64());
    assert_eq!(device["connected"], Value::Null);
    // Room-only pairing cannot gain server-wide device authority.
    assert_eq!(
        authorized(
            fixture.client.get(format!(
                "http://{}/api/central-owner/sessions",
                fixture.address
            )),
            &bearer,
            DEVICE
        )
        .header("x-central-generation", generation)
        .send()
        .await?
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        native(&fixture, Some(json!({"scope":"all"})))
            .await
            .status(),
        StatusCode::OK
    );
    status(&mut root, "ended").await;
    for peer in [&mut paired, &mut companion] {
        assert!(
            tokio::time::timeout(Duration::from_secs(2), peer.wait_closed()).await?,
            "idle dependent transport survived host revocation"
        );
    }
    assert!(!join(&fixture, &pending).await?.status().is_success());
    assert_eq!(
        authorized(
            fixture.client.post(format!(
                "http://{}/api/session-tickets/socket",
                fixture.address
            )),
            &bearer,
            DEVICE
        )
        .send()
        .await?
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let after: Value = native(&fixture, None)
        .await
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(after["sessions"].as_array().ok_or("host session")?.len(), 1);
    assert_eq!(after["sessions"][0]["session_id"], "host");
    drop(root);
    drop(paired);
    drop(companion);
    stop(fixture).await;
    Ok(())
}
