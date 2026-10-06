use super::*;

pub(super) async fn verify(
    store: &agentsassemble_persistence::SqliteStore,
    client: &reqwest::Client,
    base: &str,
    invite: &str,
    received: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> TestResult {
    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([6_u8; 32]));
    let challenge = client
        .post(format!("{base}/api/member-connect/challenge"))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?;
    assert_eq!(challenge.status(), StatusCode::OK);
    let challenge: Value = challenge.json().await?;
    let rooms = client
        .post(format!("{base}/api/member-connect/rooms"))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":challenge["challenge_id"],"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(rooms.status(), StatusCode::OK);
    let rooms: Value = rooms.json().await?;
    assert_eq!(rooms["rooms"][0]["room_id"], "general");
    let report = received.recv().await.ok_or("connect redeem")?;
    assert_eq!(report["purpose"], "connect");
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        let client = client.clone();
        let base = base.to_owned();
        let device = device.clone();
        let challenge = challenge.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let response=client.post(format!("{base}/api/member-connect/select")).header("x-device-token",device).json(&json!({"challenge_id":challenge["challenge_id"],"room_id":"general","client_id":"connect-client"})).send().await?;
            assert_eq!(response.status(),StatusCode::OK);
            response.json::<Value>().await
        }));
    }
    let first = tasks.remove(0).await??;
    let second = tasks.remove(0).await??;
    let same_session = first == second;
    assert!(
        same_session,
        "exact concurrent selection must return one session"
    );
    let other=client.post(format!("{base}/api/member-connect/select")).header("x-device-token",&device).json(&json!({"challenge_id":challenge["challenge_id"],"room_id":"other","client_id":"connect-client"})).send().await?;
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);
    let waiting: Value = client
        .post(format!("{base}/api/member-connect/challenge"))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?
        .json()
        .await?;
    let waiting_rooms = client
        .post(format!("{base}/api/member-connect/rooms"))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":waiting["challenge_id"],"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(waiting_rooms.status(), StatusCode::OK);
    received.recv().await.ok_or("waiting redeem")?;
    kick(store, first["agent_id"].as_str().ok_or("participant")?).await?;
    let fresh_denied=client.post(format!("{base}/api/member-connect/select")).header("x-device-token",&device).json(&json!({"challenge_id":waiting["challenge_id"],"room_id":"general","client_id":"connect-client"})).send().await?;
    assert_eq!(fresh_denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        fresh_denied.json::<Value>().await?["code"],
        "member_membership_ended"
    );
    let replay=client.post(format!("{base}/api/member-connect/select")).header("x-device-token",&device).json(&json!({"challenge_id":challenge["challenge_id"],"room_id":"general","client_id":"connect-client"})).send().await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert!(
        replay.json::<Value>().await? == first,
        "completed retry must return the original response after kick"
    );
    assert_revoked_bearer(client, base, &first).await?;
    // A fresh list after removal is empty.
    let challenge: Value = client
        .post(format!("{base}/api/member-connect/challenge"))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?
        .json()
        .await?;
    let empty = client
        .post(format!("{base}/api/member-connect/rooms"))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":challenge["challenge_id"],"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(empty.status(), StatusCode::FORBIDDEN);
    assert_eq!(empty.json::<Value>().await?["code"], "member_no_rooms");
    received.recv().await.ok_or("empty redeem")?;
    replacement_after_denial(store, client, base, invite, &device, received).await
}

async fn replacement_after_denial(
    store: &agentsassemble_persistence::SqliteStore,
    client: &reqwest::Client,
    base: &str,
    invite: &str,
    device: &str,
    received: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> TestResult {
    let denied_challenge = super::challenge(client, base, invite, device).await?;
    let denied_join = super::join(
        client,
        base,
        invite,
        device,
        &denied_challenge,
        "aamg1.replacement",
    )
    .await?;
    assert_eq!(denied_join.status(), StatusCode::FORBIDDEN);
    received.recv().await.ok_or("replacement redeem")?;
    let dirty = store
        .take_member_projection_batch(chrono::Utc::now().timestamp() + 3600, 20)
        .await?;
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].projection_id, "BBBBBBBBBBBBBBBBBBBBBQ");
    assert_eq!(dirty[0].state, "removed");
    Ok(())
}

async fn kick(store: &agentsassemble_persistence::SqliteStore, participant: &str) -> TestResult {
    let manager = agentsassemble_domain::AuthenticatedPrincipal {
        room_id: "general".into(),
        principal_id: agentsassemble_domain::LOCAL_OPERATOR_USER_ID.into(),
        participant_id: agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "SeiNel".into(),
        client_kind: agentsassemble_domain::ClientKind::Browser,
        is_operator: true,
        capabilities: agentsassemble_domain::CapabilitySet::local_operator(
            agentsassemble_domain::ClientKind::Browser,
            InviteScope::ReadWrite,
        ),
        invite_scope: InviteScope::ReadWrite,
    };
    store
        .execute_participant_removal(
            agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&manager),
            "connect-kick",
            "participant.kick",
            &json!({"participant_id":participant}),
        )
        .await?;
    Ok(())
}

async fn assert_revoked_bearer(
    client: &reqwest::Client,
    base: &str,
    response: &Value,
) -> TestResult {
    let denied = client
        .post(format!("{base}/api/session-tickets/socket"))
        .bearer_auth(response["session_token"].as_str().ok_or("session token")?)
        .send()
        .await?;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}
