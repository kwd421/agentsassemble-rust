use super::*;

struct MemberHost {
    store: agentsassemble_persistence::SqliteStore,
    invite: agentsassemble_server::IssuedHumanInviteCredentials,
    base: String,
    received: tokio::sync::mpsc::UnboundedReceiver<Value>,
    cancellation: CancellationToken,
    host_task: tokio::task::JoinHandle<()>,
    worker_cancellation: CancellationToken,
    worker_task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl MemberHost {
    async fn stop(self) -> TestResult {
        self.cancellation.cancel();
        self.host_task.await?;
        self.worker_cancellation.cancel();
        self.worker_task.await??;
        Ok(())
    }
}

async fn start_member_host(
    invite_scope: InviteScope,
) -> Result<MemberHost, Box<dyn std::error::Error>> {
    let (store, invite) = fixture(invite_scope).await;
    store.set_registration_epoch(Some("member-epoch")).await?;
    let identity = store.host_identity().await?;
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(identity.private_key_pkcs8())
        .map_err(|_| "fixture signing key")?;
    let key = pair.public_key().as_ref().to_vec();
    let server_id = identity.server_id().to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let central = format!("http://{}", listener.local_addr()?);
    let issuer = central.clone();
    let (calls, received) = tokio::sync::mpsc::unbounded_channel();
    let worker = worker_router(key, server_id, issuer, calls);
    let worker_cancellation = CancellationToken::new();
    let worker_shutdown = worker_cancellation.clone();
    let worker_task = tokio::spawn(async move {
        axum::serve(listener, worker)
            .with_graceful_shutdown(worker_shutdown.cancelled_owned())
            .await
    });
    let host = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", host.local_addr()?);
    let state = AppState::local(
        store.clone(),
        TicketStore::new(Duration::from_secs(30), 4096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await?
    .with_central_directory(&central)?;
    let cancellation = CancellationToken::new();
    let host_shutdown = cancellation.clone();
    let host_task = tokio::spawn(async move {
        serve(host, state, host_shutdown, async { Ok(()) })
            .await
            .unwrap_or_else(|error| panic!("member host failed: {error}"));
    });
    Ok(MemberHost {
        store,
        invite,
        base,
        received,
        cancellation,
        host_task,
        worker_cancellation,
        worker_task,
    })
}

#[tokio::test]
async fn member_join_profile_sets_name_and_avatar_and_connect_keeps_them() -> TestResult {
    let mut host = start_member_host(InviteScope::ReadWrite).await?;
    let client = reqwest::Client::new();
    let avatar = avatar_png_base64(8, 8);

    let profile_challenge = challenge_with_query(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        "?member_join_profile=1",
    )
    .await?;
    assert_eq!(profile_challenge["member_join_profile"], true);
    // A client that did not advertise the capability keeps the old exact shape,
    // and an old host never advertises it.
    let legacy_challenge = challenge(&client, &host.base, host.invite.join_code(), DEVICE).await?;
    assert!(legacy_challenge.get("member_join_profile").is_none());

    let joined = join_with_profile(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        &profile_challenge,
        "aamg1.first",
        Some(json!({"display_name":"  Edited\tName  ","avatar_png_base64":avatar})),
    )
    .await?;
    assert_eq!(joined.status(), StatusCode::OK);
    let first: Value = joined.json().await?;
    host.received.recv().await.ok_or("first redeem")?;
    assert_eq!(first["display_name"], "Edited Name");
    let avatar_url = first["avatar_image_url"].as_str().ok_or("avatar url")?;
    assert!(avatar_url.starts_with("/api/attachments/"));
    let participant_id = first["agent_id"].as_str().ok_or("agent id")?;
    let participant = host.store.participant("general", participant_id).await?;
    assert_eq!(participant.display_name, "Edited Name");
    assert_eq!(participant.avatar_image_url, avatar_url);
    let fingerprint: [u8; 32] =
        Sha256::digest(first["session_token"].as_str().ok_or("token")?.as_bytes()).into();
    let authorization = host.store.authorize_human_session(&fingerprint).await?;
    let profile = host.store.human_session_profile(&authorization).await?;
    assert_eq!(profile.display_name, "Edited Name");
    assert_eq!(profile.avatar_image_url, avatar_url);

    // Re-joining through a fresh invite challenge updates the supplied name and
    // keeps the omitted avatar. The central snapshot must not win.
    let profile_challenge = challenge_with_query(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        "?member_join_profile=1",
    )
    .await?;
    let rejoined = join_with_profile(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        &profile_challenge,
        "aamg1.second",
        Some(json!({"display_name":"Second Name"})),
    )
    .await?;
    assert_eq!(rejoined.status(), StatusCode::OK);
    let second: Value = rejoined.json().await?;
    host.received.recv().await.ok_or("rejoin redeem")?;
    assert_eq!(second["display_name"], "Second Name");
    assert_eq!(second["avatar_image_url"], avatar_url);
    assert_eq!(second["agent_id"], first["agent_id"]);
    let participant = host.store.participant("general", participant_id).await?;
    assert_eq!(participant.display_name, "Second Name");
    assert_eq!(participant.avatar_image_url, avatar_url);

    // Connect never applies a profile or a central snapshot over the stored one.
    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([6_u8; 32]));
    let connect = client
        .post(format!("{}/api/member-connect/challenge", host.base))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?;
    assert_eq!(connect.status(), StatusCode::OK);
    let connect: Value = connect.json().await?;
    let rooms = client
        .post(format!("{}/api/member-connect/rooms", host.base))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":connect["challenge_id"],"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(rooms.status(), StatusCode::OK);
    host.received.recv().await.ok_or("connect redeem")?;
    let selected = client
        .post(format!("{}/api/member-connect/select", host.base))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":connect["challenge_id"],"room_id":"general","client_id":"connect-client"}))
        .send()
        .await?;
    assert_eq!(selected.status(), StatusCode::OK);
    let selected: Value = selected.json().await?;
    assert_eq!(selected["display_name"], "Second Name");
    assert_eq!(selected["avatar_image_url"], avatar_url);

    host.stop().await
}

#[tokio::test]
async fn member_join_without_profile_keeps_the_central_name() -> TestResult {
    let mut host = start_member_host(InviteScope::ReadWrite).await?;
    let client = reqwest::Client::new();
    let challenge = challenge(&client, &host.base, host.invite.join_code(), DEVICE).await?;
    assert!(challenge.get("member_join_profile").is_none());
    let joined = join(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        &challenge,
        "aamg1.first",
    )
    .await?;
    assert_eq!(joined.status(), StatusCode::OK);
    let joined: Value = joined.json().await?;
    host.received.recv().await.ok_or("first redeem")?;
    assert_eq!(joined["display_name"], "Member snapshot");
    assert!(joined.get("avatar_image_url").is_none());
    let participant_id = joined["agent_id"].as_str().ok_or("agent id")?;
    let participant = host.store.participant("general", participant_id).await?;
    assert_eq!(participant.display_name, "Member snapshot");
    assert!(participant.avatar_image_url.is_empty());
    let fingerprint: [u8; 32] =
        Sha256::digest(joined["session_token"].as_str().ok_or("token")?.as_bytes()).into();
    let authorization = host.store.authorize_human_session(&fingerprint).await?;
    let profile = host.store.human_session_profile(&authorization).await?;
    assert_eq!(profile.display_name, "Member snapshot");
    assert!(profile.avatar_image_url.is_empty());
    host.stop().await
}

#[tokio::test]
async fn member_join_refuses_invalid_profile_name_without_redeeming() -> TestResult {
    let mut host = start_member_host(InviteScope::ReadWrite).await?;
    let client = reqwest::Client::new();
    let before = host
        .store
        .snapshot("general", 0, 200)
        .await?
        .participants
        .len();
    let too_long = "x".repeat(129);
    for name in ["   ".to_owned(), too_long] {
        let challenge = challenge(&client, &host.base, host.invite.join_code(), DEVICE).await?;
        let response = join_with_profile(
            &client,
            &host.base,
            host.invite.join_code(),
            DEVICE,
            &challenge,
            "aamg1.first",
            Some(json!({"display_name": name})),
        )
        .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: Value = response.json().await?;
        assert_eq!(error["code"], "member_profile_name_invalid");
    }
    assert!(
        host.received.try_recv().is_err(),
        "an invalid profile name must not redeem a central grant"
    );
    assert_eq!(
        host.store
            .snapshot("general", 0, 200)
            .await?
            .participants
            .len(),
        before
    );
    let challenge = challenge(&client, &host.base, host.invite.join_code(), DEVICE).await?;
    let joined = join(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        &challenge,
        "aamg1.first",
    )
    .await?;
    assert_eq!(joined.status(), StatusCode::OK);
    let joined: Value = joined.json().await?;
    host.received.recv().await.ok_or("valid redeem")?;
    assert_eq!(joined["display_name"], "Member snapshot");
    host.stop().await
}

#[tokio::test]
async fn member_join_refuses_invalid_or_oversize_avatar_without_storing_profile() -> TestResult {
    let mut host = start_member_host(InviteScope::ReadWrite).await?;
    let client = reqwest::Client::new();
    let before = host
        .store
        .snapshot("general", 0, 200)
        .await?
        .participants
        .len();
    for avatar in [STANDARD.encode(b"not a png"), avatar_png_base64(5000, 1)] {
        let challenge = challenge_with_query(
            &client,
            &host.base,
            host.invite.join_code(),
            DEVICE,
            "?member_join_profile=1",
        )
        .await?;
        let response = join_with_profile(
            &client,
            &host.base,
            host.invite.join_code(),
            DEVICE,
            &challenge,
            "aamg1.first",
            Some(json!({"display_name":"Never Stored","avatar_png_base64":avatar})),
        )
        .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: Value = response.json().await?;
        assert_eq!(error["code"], "member_profile_avatar_invalid");
        host.received.recv().await.ok_or("avatar redeem")?;
        assert_eq!(
            host.store
                .snapshot("general", 0, 200)
                .await?
                .participants
                .len(),
            before
        );
    }
    // A following first admission proves the rejected profiles committed no
    // binding or profile row: the central name still wins.
    let challenge = challenge(&client, &host.base, host.invite.join_code(), DEVICE).await?;
    let joined = join(
        &client,
        &host.base,
        host.invite.join_code(),
        DEVICE,
        &challenge,
        "aamg1.first",
    )
    .await?;
    assert_eq!(joined.status(), StatusCode::OK);
    let joined: Value = joined.json().await?;
    host.received.recv().await.ok_or("valid redeem")?;
    assert_eq!(joined["display_name"], "Member snapshot");
    assert!(joined.get("avatar_image_url").is_none());
    host.stop().await
}

fn avatar_png_base64(width: u32, height: u32) -> String {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .unwrap_or_else(|error| panic!("write avatar PNG header: {error}"));
        let pixels = vec![
            0_u8;
            usize::try_from(u64::from(width) * u64::from(height) * 4)
                .unwrap_or_else(|error| panic!("avatar pixel size: {error}"))
        ];
        writer
            .write_image_data(&pixels)
            .unwrap_or_else(|error| panic!("write avatar PNG pixels: {error}"));
    }
    STANDARD.encode(encoded)
}
