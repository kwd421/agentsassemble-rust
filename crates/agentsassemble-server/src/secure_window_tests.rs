use crate::{secure_body_record, secure_channel::write_records, secure_queue::*};
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

#[tokio::test(start_paused = true)]
async fn shared_window_cancel_behind_unacknowledged_control_and_idle_cleanup() {
    let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
    let closed = CancellationToken::new();
    let (output, outgoing) = Output::new(global.clone(), closed.clone());
    let (sent, mut records) = mpsc::unbounded_channel();
    let sink = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
        tx.send(record).map_err(|_| ())?;
        Ok::<_, ()>(tx)
    }));
    let (feedback, rx) = mpsc::channel(1);
    let writer = tokio::spawn(write_records(
        sink,
        |bytes| Ok(bytes.to_vec()),
        outgoing,
        rx,
        closed.clone(),
        output.budget.clone(),
    ));
    output
        .send(serde_json::json!({"op":"response"}))
        .await
        .unwrap();
    records.recv().await.unwrap();
    let cancelled = CancellationToken::new();
    let first = output.with_cancel(cancelled.clone());
    let bytes = vec![17; CHUNK_BYTES];
    for i in 0..5 {
        first
            .send_window(
                1,
                i * CHUNK_BYTES as u64,
                secure_body_record::encode(1, i * CHUNK_BYTES as u64, &bytes, false),
            )
            .await
            .unwrap();
        records.recv().await.unwrap();
    }
    output
        .send_window(3, 0, secure_body_record::encode(3, 0, &bytes, false))
        .await
        .unwrap();
    records.recv().await.unwrap();
    let waiting = output.send_window(2, 0, secure_body_record::encode(2, 0, &bytes, false));
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    feedback
        .send(Feedback::Cancel {
            id: 1,
            receipt: true,
        })
        .await
        .unwrap();
    let receipt = records.recv().await.unwrap();
    assert!(matches!(receipt, axum::extract::ws::Message::Binary(bytes) if bytes[0] == b'{'));
    feedback.send(Feedback::CancelConfirm(1)).await.unwrap();
    waiting.await.unwrap(); // Does not require the unrelated control ACK.
    records.recv().await.unwrap();
    assert!(global.available_permits() >= GLOBAL_BYTES - 2 * CHUNK_BYTES - 160);
    feedback.send(Feedback::Ack(0)).await.unwrap();
    tokio::time::advance(crate::http_body_policy::IDLE).await;
    writer.await.unwrap();
    assert!(closed.is_cancelled());
    assert_eq!(global.available_permits(), GLOBAL_BYTES);
}

#[tokio::test]
async fn cumulative_credit_refuses_overflow_underflow_partial_and_replay() {
    for offsets in [
        vec![CHUNK_BYTES as u64 + 1],
        vec![0],
        vec![1],
        vec![CHUNK_BYTES as u64, CHUNK_BYTES as u64],
    ] {
        let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
        let closed = CancellationToken::new();
        let (output, outgoing) = Output::new(global.clone(), closed.clone());
        let (sent, mut records) = mpsc::unbounded_channel();
        let sink = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
            tx.send(record).map_err(|_| ())?;
            Ok::<_, ()>(tx)
        }));
        let (feedback, rx) = mpsc::channel(1);
        let writer = tokio::spawn(write_records(
            sink,
            |bytes| Ok(bytes.to_vec()),
            outgoing,
            rx,
            closed.clone(),
            output.budget.clone(),
        ));
        output
            .send_window(
                1,
                0,
                secure_body_record::encode(1, 0, &vec![1; CHUNK_BYTES], false),
            )
            .await
            .unwrap();
        records.recv().await.unwrap();
        for offset in offsets {
            feedback
                .send(Feedback::Credit { id: 1, offset })
                .await
                .unwrap();
        }
        writer.await.unwrap();
        assert!(closed.is_cancelled());
        assert_eq!(global.available_permits(), GLOBAL_BYTES);
    }
}

#[tokio::test]
async fn advertised_ingress_is_reserved_before_bytes_and_cannot_exceed_shared_window() {
    let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let window = IncomingWindow::reserve(&budget).unwrap();
    assert_eq!(global.available_permits(), GLOBAL_BYTES - WINDOW_BYTES);
    let leases = (0..6)
        .map(|_| {
            window
                .available
                .clone()
                .try_acquire_many_owned(CHUNK_BYTES as u32)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(window.available.clone().try_acquire_owned().is_err());
    drop(leases);
    drop(window);
    assert_eq!(global.available_permits(), GLOBAL_BYTES);
}

#[tokio::test(start_paused = true)]
async fn stalled_body_releases_share_while_healthy_consumers_and_control_continue() {
    let global = Arc::new(Semaphore::new(GLOBAL_BYTES));
    let closed = CancellationToken::new();
    let (output, outgoing) = Output::new(global.clone(), closed.clone());
    let (sent, mut records) = mpsc::unbounded_channel();
    let sink = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
        tx.send(record).map_err(|_| ())?;
        Ok::<_, ()>(tx)
    }));
    let (feedback, rx) = mpsc::channel(1);
    let writer = tokio::spawn(write_records(
        sink,
        |bytes| Ok(bytes.to_vec()),
        outgoing,
        rx,
        closed.clone(),
        output.budget.clone(),
    ));
    let stalled = CancellationToken::new();
    output
        .with_cancel(stalled.clone())
        .send_window(
            1,
            0,
            secure_body_record::encode(1, 0, &vec![1; CHUNK_BYTES], false),
        )
        .await
        .unwrap();
    records.recv().await.unwrap();
    let mut error_handled = false;
    // Idle keepalive Pings share the sink; application assertions skip them
    // so timed tests stay focused on encrypted records.
    async fn recv_data(
        records: &mut tokio::sync::mpsc::UnboundedReceiver<axum::extract::ws::Message>,
    ) -> axum::extract::ws::Message {
        loop {
            let message = records.recv().await.unwrap();
            match &message {
                axum::extract::ws::Message::Ping(_) | axum::extract::ws::Message::Pong(_) => {
                    continue;
                }
                _ => return message,
            }
        }
    }
    for id in 2..5 {
        tokio::time::advance(std::time::Duration::from_secs(10)).await;
        output
            .send_window(
                id,
                0,
                secure_body_record::encode(id, 0, &vec![2; CHUNK_BYTES], false),
            )
            .await
            .unwrap();
        // At t=30, the timeout control can arrive before the healthy piece.
        let mut piece = recv_data(&mut records).await;
        if matches!(&piece, axum::extract::ws::Message::Binary(bytes) if bytes[0] == b'{') {
            feedback
                .send(Feedback::Credit {
                    id: 1,
                    offset: CHUNK_BYTES as u64,
                })
                .await
                .unwrap();
            feedback.send(Feedback::Ack(id as u64 - 1)).await.unwrap();
            error_handled = true;
            piece = recv_data(&mut records).await;
        }
        assert!(
            matches!(piece, axum::extract::ws::Message::Binary(bytes) if bytes[0] == secure_body_record::RESPONSE)
        );
        feedback
            .send(Feedback::Credit {
                id,
                offset: CHUNK_BYTES as u64,
            })
            .await
            .unwrap();
    }
    if !error_handled {
        let error = recv_data(&mut records).await;
        assert!(matches!(error, axum::extract::ws::Message::Binary(bytes) if bytes[0] == b'{'));
        feedback.send(Feedback::Ack(4)).await.unwrap();
    }
    assert!(stalled.is_cancelled());
    assert!(!closed.is_cancelled());
    drop(output);
    writer.await.unwrap();
    assert_eq!(global.available_permits(), GLOBAL_BYTES);
}
