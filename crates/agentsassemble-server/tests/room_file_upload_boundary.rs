use super::*;
use agentsassemble_domain::public_settings;
use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn set_limit(server: &LocalServer, limit: usize, request_id: &str) -> TestResult {
    let snapshot = server.store.snapshot("general", 0, 100).await?;
    server
        .store
        .execute_room_settings_update(
            TrustedPrincipal(&local_principal()),
            request_id,
            &json!({
                "expected_revision": public_settings(&snapshot.settings)?.settings_revision,
                "attachment_file_limit_bytes": limit,
            }),
        )
        .await?;
    Ok(())
}

async fn upload_ticket(server: &LocalServer) -> String {
    server
        .tickets
        .issue_message_attachment_upload(
            "general".into(),
            LOCAL_OPERATOR_USER_ID.into(),
            LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        )
        .await
        .expect("ticket")
        .ticket
}

#[tokio::test]
async fn room_limit_bounds_plain_bodies_and_grandfathers_bound_downloads() -> TestResult {
    let server = start_local().await;
    let authority = server.base_url.strip_prefix("http://").ok_or("address")?;
    let mut connection = TcpStream::connect(authority).await?;
    let ticket = upload_ticket(&server).await;
    let size = 25 * 1024 * 1024_usize;
    let encoded_size = size.div_ceil(3) * 4;
    connection.write_all(format!("POST /api/message-attachments HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {ticket}\r\nContent-Type: application/json\r\nContent-Length: {encoded_size}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut response = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        connection.read_to_end(&mut response),
    )
    .await??;
    assert!(
        response.starts_with(b"HTTP/1.1 413 "),
        "reject from headers without reading a body"
    );
    assert!(String::from_utf8_lossy(&response).contains("payload_too_large"));

    // Chunked requests cannot evade the same room bound. The sender offers an
    // unfinished body; after crossing the limit the host must answer without EOF.
    let mut connection = TcpStream::connect(authority).await?;
    let ticket = upload_ticket(&server).await;
    connection.write_all(format!("POST /api/message-attachments HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {ticket}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let (mut reader, mut writer) = connection.into_split();
    let sender = tokio::spawn(async move {
        for _ in 0..220 {
            if writer.write_all(b"10000\r\n").await.is_err() {
                return;
            }
            if writer.write_all(&[b' '; 65536]).await.is_err() {
                return;
            }
            if writer.write_all(b"\r\n").await.is_err() {
                return;
            }
        }
        std::future::pending::<()>().await; // Deliberately withhold the rest and EOF.
    });
    let mut response = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            response.push(reader.read_u8().await?);
            if response.ends_with(b"\r\n\r\n") {
                return Ok::<(), std::io::Error>(());
            }
        }
    })
    .await;
    sender.abort();
    let _ = sender.await;
    read??;
    assert!(response.starts_with(b"HTTP/1.1 413 "));

    let client = Client::new();
    // One decoded byte over the policy still fits its base64/envelope bound.
    let oversized = client.post(format!("{}/api/message-attachments", server.base_url))
        .bearer_auth(upload_ticket(&server).await)
        .json(&json!({"filename":"edge.bin", "content_type":"application/octet-stream", "data_base64":STANDARD.encode(vec![1; 10 * 1024 * 1024 + 1])}))
        .send().await?;
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let error: Value = oversized.json().await?;
    assert_eq!(error["code"], "attachment_too_large");
    assert!(
        error["error"]
            .as_str()
            .ok_or("error detail")?
            .contains("10485760")
    );

    set_limit(&server, 50 * 1024 * 1024, "raise").await?;
    let bytes: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    let uploaded = upload_local(
        &client,
        &server,
        "large.bin",
        "application/octet-stream",
        &bytes,
    )
    .await;
    set_limit(&server, 10 * 1024 * 1024, "lower").await?;
    server
        .store
        .execute_message_with_turn(
            &local_principal(),
            "bind-after-lowering",
            "message.send",
            &json!({"content":"", "attachment_ids":[uploaded["id"]]}),
        )
        .await?;
    let ticket = issue_local_read(&server.tickets, &uploaded["id"]).await;
    let response = read_attachment(
        &client,
        &server.base_url,
        &uploaded["download_url"],
        &ticket,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await?.as_ref(), bytes.as_slice());
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn lowered_policy_is_rechecked_after_body_admission() -> TestResult {
    let server = start_local().await;
    set_limit(&server, 50 * 1024 * 1024, "raise-race").await?;
    let authority = server.base_url.strip_prefix("http://").ok_or("address")?;
    let mut connection = TcpStream::connect(authority).await?;
    let ticket = upload_ticket(&server).await;
    // Expect/continue is a protocol barrier: Hyper requests body data only after
    // the authorized handler has resolved its policy and reserved the body budget.
    let body = serde_json::to_vec(
        &json!({"filename":"race.bin", "content_type":"application/octet-stream", "data_base64":STANDARD.encode(vec![7; 25 * 1024 * 1024])}),
    )?;
    connection.write_all(format!("POST /api/message-attachments HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {ticket}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
    let mut continued = [0_u8; 25];
    tokio::time::timeout(
        Duration::from_secs(2),
        connection.read_exact(&mut continued),
    )
    .await??;
    assert_eq!(&continued, b"HTTP/1.1 100 Continue\r\n\r\n");
    set_limit(&server, 10 * 1024 * 1024, "lower-race").await?;
    connection.write_all(&body).await?;
    let mut response = Vec::new();
    connection.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;
    assert!(response.starts_with("HTTP/1.1 413 "));
    assert!(response.contains("attachment_too_large"));
    assert!(
        response.contains("10485760"),
        "current limit is safe error detail"
    );
    assert_eq!(
        server.store.attachment_storage_usage().await?.total.files,
        0
    );
    server.stop().await;
    Ok(())
}
