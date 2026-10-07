use super::*;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn trusted_webcrypto_owner_uses_encrypted_http_sse_and_room_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = start_fixture().await;
    let published = next_call(&mut fixture.calls).await;
    let generation = serde_json::from_slice::<Value>(&published.body)?["generation"]
        .as_i64()
        .ok_or("generation")?;
    fixture
        .store
        .set_registration_epoch(Some("secure-test-epoch"))
        .await?;
    let info: Value = fixture
        .client
        .get(format!("http://{}/api/server-info", fixture.address))
        .send()
        .await?
        .json()
        .await?;
    let proxy = TcpListener::bind("127.0.0.1:0").await?;
    let proxy_address = proxy.local_addr()?;
    let address = fixture.address;
    let proxy_cancel = CancellationToken::new();
    let proxy_stop = proxy_cancel.clone();
    let proxy_task = tokio::spawn(async move {
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
                forwarded.push_str(&format!("host: owner.example.test\r\norigin: https://trusted-entry.test\r\nx-forwarded-proto: https\r\nx-agentsassemble-proxy-token: {SECRET}\r\n\r\n"));
                let Ok(mut outgoing) = tokio::net::TcpStream::connect(address).await else { return; };
                if outgoing.write_all(forwarded.as_bytes()).await.is_err() { return; }
                let _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing).await;
            });
        }
        while children.join_next().await.is_some() {}
    });
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
    input.write_all(format!("{}\n",json!({"proxy":proxy_address.to_string(),"device":DEVICE,"grant":TOKEN,"target":{
        "server_id":info["server_id"],"registration_epoch":"secure-test-epoch","origin":ORIGIN,"generation":generation,
        "host_public_key_jwk":info["host_public_key_jwk"],"host_key_fingerprint":info["host_key_fingerprint"]}})).as_bytes()).await?;
    let completed = tokio::time::timeout(Duration::from_secs(30), output.next_line())
        .await??
        .ok_or("peer failed")?;
    let credentials: Value = serde_json::from_str(&completed)?;
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
    input.write_all(b"close\n").await?;
    assert_eq!(output.next_line().await?.as_deref(), Some("closed"));
    assert!(peer.wait().await?.success());
    fixture.cancel.cancel();
    fixture.host_task.await?;
    proxy_cancel.cancel();
    proxy_task.await?;
    fixture.worker_task.abort();
    Ok(())
}
