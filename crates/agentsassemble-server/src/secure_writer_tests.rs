use super::send_response;
use crate::{
    http_body_policy::{IDLE, Progress},
    secure_body_record,
    secure_channel::write_records,
    secure_queue::{CHUNK_BYTES, Feedback, GLOBAL_BYTES, Output},
};
use axum::{
    body::{Body, Bytes},
    extract::ws::Message,
    response::Response,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

struct Writer {
    output: Output,
    feedback: mpsc::Sender<Feedback>,
    records: mpsc::UnboundedReceiver<Message>,
    closed: CancellationToken,
    global: Arc<Semaphore>,
    task: JoinHandle<()>,
}
impl Writer {
    fn new(panic: bool) -> Self {
        let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
        let closed = CancellationToken::new();
        let (output, outgoing) = Output::new(global.clone(), closed.clone());
        let (sent, records) = mpsc::unbounded_channel();
        let sink = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
            tx.send(record).map_err(|_| ())?;
            Ok::<_, ()>(tx)
        }));
        let (feedback, rx) = mpsc::channel(1);
        let task = tokio::spawn(write_records(
            sink,
            move |bytes| {
                assert!(!panic, "injected writer task panic");
                Ok(bytes.to_vec())
            },
            outgoing,
            rx,
            closed.clone(),
            output.budget.clone(),
        ));
        Self {
            output,
            feedback,
            records,
            closed,
            global,
            task,
        }
    }
    async fn record(&mut self) -> Vec<u8> {
        // Idle keepalive Pings share the sink; application assertions skip
        // them so timed tests stay focused on encrypted records.
        loop {
            let record = self.records.recv().await.expect("writer record");
            match record {
                Message::Binary(bytes) => return bytes.to_vec(),
                Message::Ping(_) | Message::Pong(_) => continue,
                _ => panic!("binary record"),
            }
        }
    }
    async fn next_message(&mut self) -> Message {
        self.records.recv().await.expect("writer message")
    }
    async fn credit(&self, id: u32, offset: u64) {
        self.feedback
            .send(Feedback::Credit { id, offset })
            .await
            .unwrap();
    }
    async fn ack(&self, sequence: u64) {
        self.feedback.send(Feedback::Ack(sequence)).await.unwrap();
    }
    async fn finish(self) {
        drop(self.output);
        self.task.await.unwrap();
        assert!(self.closed.is_cancelled());
        assert_eq!(self.global.available_permits(), GLOBAL_BYTES);
    }
}

#[tokio::test]
async fn body_failure_before_data_or_after_every_record_was_credited_keeps_channel_usable() {
    for prior_piece in [false, true] {
        let mut writer = Writer::new(false);
        let (parts, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
        let stream = futures_util::stream::unfold(rx, |mut rx| async {
            rx.recv().await.map(|part| (part, rx))
        });
        let response = Response::new(Body::from_stream(stream));
        let cancel = writer.closed.child_token();
        let response_task = tokio::spawn(send_response(
            response,
            1,
            writer.output.with_http(1, true).with_cancel(cancel.clone()),
            cancel,
            true,
            None,
            true,
            None,
        ));
        let header: serde_json::Value = serde_json::from_slice(&writer.record().await).unwrap();
        assert_eq!(header["op"], "response");
        writer.ack(0).await;
        if prior_piece {
            parts.send(Ok(Bytes::from_static(b"piece"))).await.unwrap();
            let bytes = writer.record().await;
            assert_eq!(
                secure_body_record::decode(&bytes, secure_body_record::RESPONSE)
                    .unwrap()
                    .bytes,
                b"piece"
            );
            writer.credit(1, 5).await;
        }
        // Models the lazy reader's authorization/storage error, at either boundary.
        parts
            .send(Err(std::io::Error::other("fixture storage refusal")))
            .await
            .unwrap();
        drop(parts);
        response_task.await.unwrap();
        let error: serde_json::Value = serde_json::from_slice(&writer.record().await).unwrap();
        assert_eq!(error["code"], "response_failed");
        let error_sequence = if prior_piece { 2 } else { 1 };
        writer.ack(error_sequence).await;
        writer
            .output
            .send(serde_json::json!({"op":"response","id":2}))
            .await
            .unwrap();
        let next: serde_json::Value = serde_json::from_slice(&writer.record().await).unwrap();
        assert_eq!(next["id"], 2);
        writer.ack(error_sequence + 1).await;
        assert!(!writer.closed.is_cancelled());
        writer.finish().await;
    }
}

#[tokio::test(start_paused = true)]
async fn partial_and_other_body_credits_cannot_extend_oldest_emission() {
    let mut writer = Writer::new(false);
    let cancelled = writer.closed.child_token();
    let first = writer.output.with_cancel(cancelled.clone());
    let second = writer.output.with_cancel(writer.closed.child_token());
    let piece = vec![7; CHUNK_BYTES];
    first
        .send_window(1, 0, secure_body_record::encode(1, 0, &piece, false))
        .await
        .unwrap();
    writer.record().await;
    tokio::time::advance(Duration::from_secs(10)).await;
    first
        .send_window(
            1,
            CHUNK_BYTES as u64,
            secure_body_record::encode(1, CHUNK_BYTES as u64, &piece, false),
        )
        .await
        .unwrap();
    writer.record().await;
    tokio::time::advance(Duration::from_secs(10)).await;
    for offset in [0, CHUNK_BYTES as u64] {
        second
            .send_window(
                2,
                offset,
                secure_body_record::encode(2, offset, &piece, false),
            )
            .await
            .unwrap();
        writer.record().await;
    }
    tokio::time::advance(Duration::from_secs(9)).await;
    writer.credit(1, CHUNK_BYTES as u64).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    writer.credit(2, CHUNK_BYTES as u64).await;
    assert!(!cancelled.is_cancelled());
    tokio::time::advance(Duration::from_secs(1)).await;
    let error: serde_json::Value = serde_json::from_slice(&writer.record().await).unwrap();
    assert_eq!(error["id"], 1);
    assert_eq!(error["code"], "response_timeout");
    assert!(cancelled.is_cancelled());
    assert!(!writer.closed.is_cancelled());
    writer.ack(4).await;
    writer.credit(2, 2 * CHUNK_BYTES as u64).await;
    drop(first);
    drop(second);
    writer.finish().await;
}

#[tokio::test(start_paused = true)]
async fn local_ack_backlog_before_emission_preserves_the_full_peer_allowance() {
    let mut writer = Writer::new(false);
    for _ in 0..3 {
        writer
            .output
            .send(serde_json::json!({"op":"socket_data","id":9,"data":"fixture"}))
            .await
            .unwrap();
    }
    writer.record().await;
    let download = writer
        .output
        .with_http(1, true)
        .with_cancel(writer.closed.child_token())
        .with_progress(Some(Progress::new()));
    download
        .send(serde_json::json!({"op":"response","id":1}))
        .await
        .unwrap();
    download
        .send_window(1, 0, secure_body_record::encode(1, 0, b"piece", false))
        .await
        .unwrap();
    for sequence in 0..3 {
        tokio::time::advance(IDLE - Duration::from_secs(1)).await;
        assert!(!writer.closed.is_cancelled());
        writer.ack(sequence).await;
        writer.record().await;
    }
    writer.ack(3).await;
    writer.record().await;
    tokio::time::advance(IDLE - Duration::from_secs(1)).await;
    assert!(!writer.closed.is_cancelled());
    writer.credit(1, 5).await;
    drop(download);
    writer.finish().await;
}

#[tokio::test(start_paused = true)]
async fn idle_channel_pings_every_thirty_seconds_and_stops_after_close() {
    use crate::secure_channel::SECURE_CHANNEL_KEEPALIVE;

    assert_eq!(SECURE_CHANNEL_KEEPALIVE, Duration::from_secs(30));
    let mut writer = Writer::new(false);
    // An idle channel gets exactly one control Ping per interval and stays
    // open past five minutes (ten intervals, the ~100 s tunnel idle limit).
    for _ in 0..10 {
        tokio::time::advance(SECURE_CHANNEL_KEEPALIVE).await;
        let message = writer.next_message().await;
        assert!(
            matches!(message, Message::Ping(_)),
            "idle keepalive must be a control Ping, never an encrypted record"
        );
        assert!(!writer.closed.is_cancelled());
    }
    // Closing the connection stops the keepalive: dropping the last output
    // ends the writer without further control frames.
    drop(writer.output);
    writer.task.await.unwrap();
    assert!(writer.closed.is_cancelled());
    tokio::time::advance(SECURE_CHANNEL_KEEPALIVE * 2).await;
    assert!(writer.records.try_recv().is_err());
}

#[tokio::test]
async fn abnormal_writer_exit_closes_channel_and_releases_charged_records() {
    for panic in [true, false] {
        let mut writer = Writer::new(panic);
        writer
            .output
            .send(serde_json::json!({"op":"ready"}))
            .await
            .unwrap();
        if !panic {
            writer.record().await;
            writer.task.abort();
        }
        assert!(writer.task.await.is_err());
        assert!(writer.closed.is_cancelled());
        assert_eq!(writer.global.available_permits(), GLOBAL_BYTES);
    }
}
