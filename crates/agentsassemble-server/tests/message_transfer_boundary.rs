use super::room_file_upload_boundary::{set_limit, upload_ticket};
use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn plain_raw_transfer_is_exact_resumable_idempotent_and_bindable() -> TestResult {
    let server = start_local().await;
    set_limit(&server, 50 * 1024 * 1024, "transfer-limit").await?;
    let client = Client::new();
    let discovery = client
        .get(format!("{}/api/message-attachments", server.base_url))
        .header("Origin", "tauri://localhost")
        .send()
        .await?;
    assert_eq!(discovery.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(discovery.headers()["x-agentsassemble-upload-transfer"], "1");
    assert_eq!(
        discovery.headers()["x-agentsassemble-message-attachment-limit"],
        "10"
    );
    assert!(
        discovery.headers()["access-control-expose-headers"]
            .to_str()?
            .contains("x-agentsassemble-message-attachment-limit")
    );
    for (index, bytes, content_type) in [
        (
            0,
            (0..50 * 1024 * 1024)
                .map(|n| u8::try_from(n % 251).unwrap())
                .collect(),
            "application/octet-stream",
        ),
        (1, STANDARD.decode(PNG_BASE64)?, "image/png"),
    ] {
        let endpoint = format!("{}/api/message-attachment-transfers", server.base_url);
        let declaration = json!({"room_id":"general","operation_id":format!("transfer-{index}"),"filename":format!("../file-{index}"),"content_type":content_type,"declared_size":bytes.len()});
        let begun = client
            .post(&endpoint)
            .bearer_auth(upload_ticket(&server).await)
            .json(&declaration)
            .send()
            .await?;
        assert_eq!(begun.status(), StatusCode::OK);
        assert_eq!(begun.headers()["x-agentsassemble-upload-transfer"], "1");
        let begun: Value = begun.json().await?;
        let handle = begun["transfer"]["handle"].as_str().ok_or("handle")?;
        let endpoint = format!("{endpoint}/{handle}");
        let repeat: Value = client
            .post(format!(
                "{}/api/message-attachment-transfers",
                server.base_url
            ))
            .bearer_auth(upload_ticket(&server).await)
            .json(&declaration)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(repeat, begun);
        // A partial request tail is retained; the next request starts at exactly that byte.
        let first = bytes.len().min(19);
        for (offset, end) in [(0, first), (first, bytes.len())] {
            let pieces: Vec<_> = bytes[offset..end]
                .chunks(65536)
                .map(|piece| Ok::<_, std::io::Error>(piece.to_vec()))
                .collect();
            let data: Value = client
                .post(format!("{endpoint}/data?offset={offset}"))
                .bearer_auth(upload_ticket(&server).await)
                .header("Content-Type", "application/octet-stream")
                .body(reqwest::Body::wrap_stream(futures_util::stream::iter(
                    pieces,
                )))
                .send()
                .await?
                .json()
                .await?;
            assert_eq!(data["transfer"]["accepted_offset"], end);
        }
        let finished = client
            .post(format!("{endpoint}/finish"))
            .bearer_auth(upload_ticket(&server).await)
            .send()
            .await?;
        assert_eq!(finished.status(), StatusCode::OK);
        let finished: Value = finished.json().await?;
        let repeated: Value = client
            .post(format!("{endpoint}/finish"))
            .bearer_auth(upload_ticket(&server).await)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(finished, repeated);
        let status: Value = client
            .get(&endpoint)
            .bearer_auth(upload_ticket(&server).await)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(status, finished);
        let attachment = &finished["transfer"]["attachment"];
        assert_eq!(attachment["is_image"], index == 1);
        assert_eq!(attachment["size"], bytes.len());
        server
            .store
            .execute_message(
                &local_principal(),
                &format!("bind-transfer-{index}"),
                "message.send",
                &json!({"content":"","attachment_ids":[attachment["id"]]}),
            )
            .await?;
        let id = attachment["id"].as_str().ok_or("id")?;
        let response = client
            .get(format!(
                "{}{}",
                server.base_url,
                attachment["download_url"].as_str().ok_or("download")?
            ))
            .bearer_auth(issue_local_read_id(&server.tickets, id).await)
            .send()
            .await?;
        assert_eq!(response.bytes().await?.as_ref(), bytes);
    }
    server.stop().await;
    Ok(())
}
