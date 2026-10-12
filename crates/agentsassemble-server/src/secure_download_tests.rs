use super::*;
use crate::{
    attachment_download::tests::{assert_piece, fixture, response},
    download_budget,
};
use std::sync::Arc;
use tokio::sync::Semaphore;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn virtual_download_preserves_piece_bytes_and_truthful_length() -> TestResult {
    let (store, metadata, _) = fixture(65536 + 19).await;
    let downloads = download_budget::Budget::default();
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let cancel = CancellationToken::new();
    let (output, mut frames) = Output::new(global.clone(), cancel.clone());
    let response = response(&store, &metadata, &downloads, None).await;
    let task = tokio::spawn(respond(response, 1, output, cancel.clone(), cancel));
    let header: serde_json::Value =
        serde_json::from_slice(&frames.recv().await.ok_or("header")?.bytes)?;
    assert_eq!(header["status"], 200);
    assert!(
        header["headers"]
            .as_array()
            .ok_or("headers")?
            .contains(&json!(["content-length", metadata.size.to_string()]))
    );
    let mut received = 0;
    while let Some(frame) = frames.recv().await {
        let frame: serde_json::Value = serde_json::from_slice(&frame.bytes)?;
        assert_eq!(frame["op"], "data");
        if frame["end"] == true {
            break;
        }
        let bytes = URL_SAFE_NO_PAD.decode(frame["data"].as_str().ok_or("data")?)?;
        assert_piece(&bytes, received);
        received += bytes.len();
    }
    task.await?;
    assert_eq!(received, metadata.size);
    assert_eq!(downloads.available(), 512 * 1024 * 1024);
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
    Ok(())
}

#[tokio::test]
async fn stalled_virtual_download_timeout_abort_and_channel_close_release_admission() -> TestResult
{
    let (store, metadata, principal) = fixture(50 * 1024 * 1024).await;
    let downloads = download_budget::Budget::default();
    for end in ["timeout", "request_abort", "channel_close"] {
        let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
        let channel = CancellationToken::new();
        let cancel = channel.child_token();
        let (output, mut frames) = Output::new(global.clone(), channel.clone());
        let response = response(&store, &metadata, &downloads, None).await;
        let task = tokio::spawn(respond(
            response,
            1,
            output.with_cancel(cancel.clone()),
            cancel.clone(),
            channel.clone(),
        ));
        drop(frames.recv().await.ok_or("header")?);
        drop(frames.recv().await.ok_or("first piece")?);
        assert_eq!(downloads.available(), 511 * 1024 * 1024);
        store
            .execute_message(
                &principal,
                end,
                "message.send",
                &json!({"content":"write while stalled"}),
            )
            .await?;
        if end == "request_abort" {
            cancel.cancel();
        }
        if end == "channel_close" {
            channel.cancel();
        }
        if end == "timeout" {
            tokio::time::pause();
        }
        task.await?;
        if end == "timeout" {
            tokio::time::resume();
            assert!(
                channel.is_cancelled(),
                "expiry must reach the peer and drain queued records"
            );
        }
        assert_eq!(downloads.available(), 512 * 1024 * 1024);
        // The unchanged ciphertext gate covers queued records, independent of file size.
        assert!(global.available_permits() >= crate::secure_queue::GLOBAL_BYTES - 512 * 1024);
        drop(frames);
        assert_eq!(
            global.available_permits(),
            crate::secure_queue::GLOBAL_BYTES
        );
    }
    Ok(())
}
