use super::*;
use std::{fmt::Write as _, process::Stdio};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn trusted_webcrypto_owner_uses_encrypted_http_sse_and_room_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = start_fixture().await;
    fixture
        .worker_state
        .late_enabled
        .store(true, Ordering::SeqCst);
    let published = next_call(&mut fixture.calls).await;
    let generation = serde_json::from_slice::<Value>(&published.body)?["generation"]
        .as_i64()
        .ok_or("generation")?;
    let info: Value = fixture
        .client
        .get(format!("http://{}/api/server-info", fixture.address))
        .send()
        .await?
        .json()
        .await?;
    let proxy = TcpListener::bind("127.0.0.1:0").await?;
    let proxy_address = proxy.local_addr()?;
    let proxy_cancel = CancellationToken::new();
    let proxy_task = start_proxy(proxy, fixture.address, proxy_cancel.clone());
    let mut peer = tokio::process::Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg(include_str!("secure_peer.mjs"))
        .env(
            "AA_REMOTE_TRANSPORT_MODULE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../frontend/src/lib/remote/remoteTransport.ts"
            ),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = peer.stdin.take().ok_or("stdin")?;
    let mut output = BufReader::new(peer.stdout.take().ok_or("stdout")?).lines();
    input.write_all(format!("{}\n",json!({"proxy":proxy_address.to_string(),"device":DEVICE,"grant":TOKEN,"late_grant":SECOND_TOKEN,"persona_png":base64::engine::general_purpose::STANDARD.encode(super::super::persona_library_boundary::png_card()),"target":{
        "server_id":info["server_id"],"registration_epoch":"secure-test-epoch","origin":ORIGIN,"generation":generation,
        "host_public_key_jwk":info["host_public_key_jwk"],"host_key_fingerprint":info["host_key_fingerprint"]}})).as_bytes()).await?;
    let completed = tokio::time::timeout(Duration::from_secs(30), output.next_line())
        .await??
        .ok_or("peer failed")?;
    let credentials: Value = serde_json::from_str(&completed)?;
    verify_plaintext_denial(&fixture, &credentials, generation).await?;
    tokio::time::timeout(
        Duration::from_secs(5),
        fixture.worker_state.late_started.notified(),
    )
    .await
    .map_err(|_| "late redemption not reached")?;
    input.write_all(b"close\n").await?;
    assert_eq!(output.next_line().await?.as_deref(), Some("closed"));
    assert!(peer.wait().await?.success());
    verify_late_admission(&fixture, &info, &credentials, generation).await?;
    fixture.cancel.cancel();
    fixture.host_task.await?;
    proxy_cancel.cancel();
    proxy_task.await?;
    fixture.worker_task.abort();
    Ok(())
}

pub(super) async fn member_redemption(
    State(state): State<WorkerState>,
    axum::Json(body): axum::Json<Value>,
) -> Json<Value> {
    assert_eq!(body["protocol"], "secure_admission_v1");
    assert_eq!(body["registration_epoch"], "secure-test-epoch");
    let mut result = json!({"projection_id":"AAAAAAAAAAAAAAAAAAAAAA","issuer":state.issuer,"person_id":"secure-member","display_name":"Secure member"});
    for field in ["protocol", "client_public_key", "channel_id", "purpose"] {
        result[field] = body[field].clone();
    }
    Json(result)
}

fn start_proxy(
    proxy: TcpListener,
    address: SocketAddr,
    proxy_stop: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut children = tokio::task::JoinSet::new();
        loop {
            let accepted = tokio::select! { () = proxy_stop.cancelled() => break, accepted = proxy.accept() => accepted };
            let Ok((mut incoming, _)) = accepted else {
                break;
            };
            children.spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    if header.len() >= 16384 { return; }
                    let Ok(byte) = incoming.read_u8().await else { return; };
                    header.push(byte);
                }
                let Ok(header) = String::from_utf8(header) else { return; };
                let mut lines = header.lines();
                let Some(first) = lines.next() else { return; };
                let mut forwarded = format!("{first}\r\n");
                for line in lines {
                    if line.is_empty() || line.to_ascii_lowercase().starts_with("host:") || line.to_ascii_lowercase().starts_with("origin:") { continue; }
                    forwarded.push_str(line); forwarded.push_str("\r\n");
                }
                let _ = write!(forwarded, "host: owner.example.test\r\norigin: https://trusted-entry.test\r\nx-forwarded-proto: https\r\nx-agentsassemble-proxy-token: {SECRET}\r\n\r\n");
                let Ok(mut outgoing) = tokio::net::TcpStream::connect(address).await else { return; };
                if outgoing.write_all(forwarded.as_bytes()).await.is_err() { return; }
                let _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing).await;
            });
        }
        while children.join_next().await.is_some() {}
    })
}

async fn verify_late_admission(
    fixture: &Fixture,
    info: &Value,
    credentials: &Value,
    generation: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    fixture.worker_state.late_release.notify_one();
    fixture.connections.close();
    tokio::time::timeout(Duration::from_secs(20), fixture.connections.wait())
        .await
        .map_err(|_| "accepted connections did not drain")?;
    assert!(
        fixture
            .store
            .owner_device_sessions(&agentsassemble_persistence::ServerOwnerAuthority::LocalOperator)
            .await?
            .is_empty(),
        "late central admission retained connected custody after channel close"
    );
    // The accepted exchange committed its durable result after the client closed,
    // then custody was ended. A cancelled handler would leave no row and allow this
    // exact entry to create a new connected session, which must be rejected here.
    let late_entry = agentsassemble_persistence::OwnerAdmission::verified(
        agentsassemble_persistence::OwnerAdmissionBinding {
            secure: Some(agentsassemble_persistence::SecureSessionBinding {
                client_key_fingerprint: Sha256::digest(
                    URL_SAFE_NO_PAD.decode(credentials["late_key"].as_str().ok_or("late key")?)?,
                )
                .into(),
                channel_id: credentials["late_channel"]
                    .as_str()
                    .ok_or("late channel")?
                    .to_owned(),
            }),
            entry_fingerprint: Sha256::digest(SECOND_TOKEN.as_bytes()).into(),
            server_id: info["server_id"].as_str().ok_or("server")?.to_owned(),
            person_id: "person-owner".into(),
            device_id: "device-owner".into(),
            browser_fingerprint: Sha256::digest(DEVICE.as_bytes()).into(),
            origin: ORIGIN.into(),
            generation,
        },
        chrono::Utc::now().timestamp() + 300,
    )?;
    assert!(
        fixture
            .store
            .create_owner_session(
                &late_entry,
                &agentsassemble_persistence::OwnerDeviceDescription::verified(
                    "Late admission".into(),
                    "Node".into(),
                    "test".into()
                )?
            )
            .await
            .is_err(),
        "accepted late admission was cancelled instead of committed and disconnected"
    );
    Ok(())
}

async fn verify_plaintext_denial(
    fixture: &Fixture,
    credentials: &Value,
    generation: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let stolen = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/directory",
        json!({"session_token":credentials["owner"],"generation":generation}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(stolen.status(), StatusCode::UNAUTHORIZED);
    let stolen_room = fixture
        .client
        .post(format!(
            "http://{}/api/session-tickets/socket",
            fixture.address
        ))
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .header("origin", ORIGIN)
        .header("x-device-token", DEVICE)
        .bearer_auth(credentials["room"].as_str().ok_or("room")?)
        .json(&json!({"device":{"device_name":"Stolen","browser":"Node","os":"test"}}))
        .send()
        .await?;
    assert_eq!(stolen_room.status(), StatusCode::UNAUTHORIZED);
    let persona = fixture
        .client
        .get(format!(
            "http://{}/api/central-owner/personas",
            fixture.address
        ))
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .header("origin", ORIGIN)
        .header("x-device-token", DEVICE)
        .header("x-central-generation", generation)
        .bearer_auth(credentials["owner"].as_str().ok_or("owner")?)
        .send()
        .await?;
    assert_eq!(persona.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}
