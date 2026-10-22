use super::*;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[tokio::test]
async fn secure_upload_consumes_more_than_direction_capacity() {
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let (tx, rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let mut input = Input {
        tx,
        queue: None,
        cancel: cancel.clone(),
        ended: false,
        binary: false,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    let (output, mut credits) = Output::new(global.clone(), cancel.clone());
    let request = Request::new(
        request_body(
            rx,
            cancel,
            1,
            true,
            output,
            crate::http_body_policy::Policy::new(),
        )
        .0,
    );
    let collect = crate::http_api::decode_json_body::<String>(
        request,
        crate::http_api::MAX_BASE64_UPLOAD_BODY_BYTES,
    );
    let collect = tokio::spawn(collect);
    let payload = format!("\"{}\"", "a".repeat(10 * 1024 * 1024));
    for (index, chunk) in payload.as_bytes().chunks(CHUNK_BYTES).enumerate() {
        assert!(
            input
                .push(&URL_SAFE_NO_PAD.encode(chunk), false, &budget)
                .is_ok(),
            "chunk {index} rejected after prior chunks were consumed"
        );
        let credit = tokio::time::timeout(std::time::Duration::from_secs(5), credits.recv())
            .await
            .unwrap_or_else(|_| panic!("receiver consumption deadline"))
            .unwrap_or_else(|| panic!("receiver consumption credit"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&credit.bytes)
                .unwrap_or_else(|error| panic!("{error:?}")),
            json!({"op":"consumed","id":1})
        );
        drop(credit);
        assert_eq!(
            global.available_permits(),
            crate::secure_queue::GLOBAL_BYTES
        );
    }
    assert!(input.push("", true, &budget).is_ok());
    assert_eq!(
        collect
            .await
            .unwrap_or_else(|_| panic!("collector task"))
            .unwrap_or_else(|_| panic!("complete body"))
            .len(),
        10 * 1024 * 1024
    );
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
}
#[tokio::test]
async fn cancellation_releases_only_its_request_queue() {
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let (output, mut credits) = Output::new(global.clone(), CancellationToken::new());
    let (tx_a, rx_a) = mpsc::channel(16);
    let (tx_b, rx_b) = mpsc::channel(16);
    let cancel_a = CancellationToken::new();
    let cancel_b = CancellationToken::new();
    let mut a = Input {
        tx: tx_a,
        queue: None,
        cancel: cancel_a.clone(),
        ended: false,
        binary: false,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    let mut b = Input {
        tx: tx_b,
        queue: None,
        cancel: cancel_b.clone(),
        ended: false,
        binary: false,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    let body_a = request_body(
        rx_a,
        cancel_a.clone(),
        1,
        true,
        output.with_cancel(cancel_a.clone()),
        crate::http_body_policy::Policy::new(),
    )
    .0;
    let body_b = request_body(
        rx_b,
        cancel_b.clone(),
        2,
        true,
        output.with_cancel(cancel_b),
        crate::http_body_policy::Policy::new(),
    )
    .0;
    let chunk = URL_SAFE_NO_PAD.encode(vec![b' '; CHUNK_BYTES]);
    for _ in 0..4 {
        assert!(a.push(&chunk, false, &budget).is_ok());
        assert!(b.push(&chunk, false, &budget).is_ok());
    }
    assert!(budget.charge(1).is_err());
    cancel_a.cancel();
    a.tx.closed().await; // Drain while the HTTP consumer has never been polled.
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES - 4 * CHUNK_BYTES
    );
    assert!(
        crate::http_api::ensure_empty_body(Request::new(body_a), 10 * CHUNK_BYTES)
            .await
            .is_err()
    );
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES - 4 * CHUNK_BYTES
    );
    let released = budget
        .charge(4 * CHUNK_BYTES)
        .unwrap_or_else(|()| panic!("cancelled queue released"));
    assert!(
        budget.charge(1).is_err(),
        "other request still owns its queue"
    );
    drop(released);
    assert!(
        a.push(&chunk, false, &budget).is_ok(),
        "late data for ended request is discarded"
    );
    assert!(b.push("", true, &budget).is_ok());
    assert_eq!(
        crate::http_api::ensure_empty_body(Request::new(body_b), 10 * CHUNK_BYTES).await,
        Err(crate::http_api::BodyDecodeError::NonEmpty)
    );
    while let Ok(credit) = credits.try_recv() {
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&credit.bytes)
                .unwrap_or_else(|error| panic!("{error:?}"))["id"],
            2
        );
    }
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
}
#[tokio::test(start_paused = true)]
async fn virtual_upload_timeout_drops_its_queue_and_ignores_late_data() {
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let (output, mut credits) = Output::new(global.clone(), CancellationToken::new());
    let (tx, rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let mut input = Input {
        tx,
        queue: None,
        cancel: cancel.clone(),
        ended: false,
        binary: false,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    let body = request_body(
        rx,
        cancel,
        1,
        true,
        output,
        crate::http_body_policy::Policy::new(),
    )
    .0;
    let other_request = budget
        .charge(CHUNK_BYTES)
        .unwrap_or_else(|()| panic!("independent request"));
    let router = axum::Router::new()
        .route(
            "/",
            axum::routing::post(
                |policy: axum::Extension<crate::http_body_policy::Policy>,
                 request: Request<Body>| async move {
                    match crate::http_api::decode_upload_body::<serde_json::Value>(
                        request,
                        1024,
                        &policy,
                        &crate::upload_body_budget::Budget::default(),
                        "test",
                    )
                    .await
                    {
                        Err(crate::upload_body_budget::Error::Body(
                            crate::http_api::BodyDecodeError::RequestTimeout,
                        )) => axum::http::StatusCode::REQUEST_TIMEOUT,
                        _ => axum::http::StatusCode::BAD_REQUEST,
                    }
                },
            ),
        )
        .layer(axum::middleware::from_fn(crate::http_body_policy::apply));
    assert!(
        input
            .push(&URL_SAFE_NO_PAD.encode(b" "), false, &budget)
            .is_ok()
    );
    let response = tokio::spawn(
        router.oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .body(body)
                .unwrap_or_else(|error| panic!("{error:?}")),
        ),
    );
    drop(
        credits
            .recv()
            .await
            .unwrap_or_else(|| panic!("consumed first chunk")),
    );
    assert_eq!(
        response
            .await
            .unwrap_or_else(|_| panic!("handler"))
            .unwrap_or_else(|error| panic!("{error:?}"))
            .status(),
        axum::http::StatusCode::REQUEST_TIMEOUT
    );
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES - CHUNK_BYTES
    );
    assert!(
        input
            .push(&URL_SAFE_NO_PAD.encode(b"late"), false, &budget)
            .is_ok()
    );
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES - CHUNK_BYTES
    );
    drop(other_request);
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
}
#[tokio::test]
async fn lost_credit_path_cannot_complete_a_body_without_its_end_marker() {
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let cancel = CancellationToken::new();
    let (output, credits) = Output::new(global.clone(), cancel.clone());
    drop(credits);
    let (tx, rx) = mpsc::channel(16);
    let mut input = Input {
        tx,
        queue: None,
        cancel: cancel.clone(),
        ended: false,
        binary: false,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    assert!(
        input
            .push(&URL_SAFE_NO_PAD.encode(b"null"), false, &budget)
            .is_ok()
    );
    let request = Request::new(
        request_body(
            rx,
            cancel,
            1,
            true,
            output,
            crate::http_body_policy::Policy::new(),
        )
        .0,
    );
    assert!(
        crate::http_api::decode_json_body::<serde_json::Value>(request, 1024)
            .await
            .is_err()
    );
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
}

#[tokio::test]
async fn unpaced_request_releases_charges_without_credit_frames()
-> Result<(), Box<dyn std::error::Error>> {
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let cancel = CancellationToken::new();
    let (output, mut frames) = Output::new(global.clone(), cancel.clone());
    let (tx, rx) = mpsc::channel(16);
    let mut input = Input {
        tx,
        queue: None,
        cancel: cancel.clone(),
        ended: false,
        binary: false,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    let mut body = request_body(
        rx,
        cancel,
        1,
        false,
        output,
        crate::http_body_policy::Policy::new(),
    )
    .0
    .into_data_stream();
    for _ in 0..16 {
        assert!(
            input
                .push(
                    &URL_SAFE_NO_PAD.encode(vec![79; CHUNK_BYTES]),
                    false,
                    &budget
                )
                .is_ok()
        );
        assert_eq!(
            body.next().await.ok_or("missing chunk")??.len(),
            CHUNK_BYTES
        );
        assert_eq!(
            global.available_permits(),
            crate::secure_queue::GLOBAL_BYTES
        );
    }
    assert!(input.push("", true, &budget).is_ok());
    assert!(body.next().await.ok_or("missing chunk")??.is_empty());
    assert!(body.next().await.is_none());
    assert!(frames.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn unpaced_virtual_bodies_share_host_admission_and_release_on_cancel() {
    use crate::{http_api::decode_upload_body, http_body_policy::Policy, upload_body_budget};
    let queues = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let queue = Budget::new(queues.clone());
    let bodies = upload_body_budget::Budget::default();
    let (output, _credits) = Output::new(queues.clone(), CancellationToken::new());
    let policy = Policy::new();
    let mut inputs = Vec::new();
    let mut reads = Vec::new();
    for id in 0..3 {
        let (tx, rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();
        let mut input = Input {
            tx,
            queue: None,
            cancel: cancel.clone(),
            ended: false,
            binary: false,
            offset: 0,
            window: None,
            body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
        };
        input
            .push(&URL_SAFE_NO_PAD.encode(b" "), false, &queue)
            .expect("queue chunk");
        // No credit opt-in: admission cannot depend on cooperative pacing.
        let request = Request::new(
            request_body(
                rx,
                cancel,
                id,
                false,
                output.clone(),
                crate::http_body_policy::Policy::new(),
            )
            .0,
        );
        let mut read = Box::pin(decode_upload_body::<serde_json::Value>(
            request,
            1024,
            &policy,
            &bodies,
            "same-principal",
        ));
        if id < 2 {
            assert!(futures_util::poll!(&mut read).is_pending());
            reads.push(read);
        } else {
            assert!(matches!(
                read.await,
                Err(upload_body_budget::Error::Exhausted)
            ));
        }
        inputs.push(input);
    }
    assert_eq!(
        queues.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
    inputs[0].cancel.cancel();
    assert!(reads.remove(0).await.is_err());
    // Cancellation returned exactly one principal slot, while the other body
    // is still retained after draining its encrypted queue charge.
    let (_, held) = decode_upload_body::<serde_json::Value>(
        Request::new(Body::from("null")),
        1024,
        &policy,
        &bodies,
        "same-principal",
    )
    .await
    .expect("released slot");
    assert!(matches!(
        decode_upload_body::<serde_json::Value>(
            Request::new(Body::from("null")),
            1024,
            &policy,
            &bodies,
            "same-principal"
        )
        .await,
        Err(upload_body_budget::Error::Exhausted)
    ));
    drop(held);
    inputs[1]
        .push(&URL_SAFE_NO_PAD.encode(b"null"), true, &queue)
        .expect("finish");
    let (_, held) = reads.remove(0).await.expect("successful upload");
    drop(held);
    let (_, first) = bodies
        .reserve(Some("same-principal"), &Request::new(Body::empty()), 1024)
        .expect("first");
    let (_, second) = bodies
        .reserve(Some("same-principal"), &Request::new(Body::empty()), 1024)
        .expect("second");
    drop((first, second));
}

#[tokio::test]
async fn binary_offsets_and_legacy_shape_cannot_mix_and_cancel_releases_bytes() {
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let budget = Budget::new(global.clone());
    let (tx, rx) = mpsc::channel(16);
    let mut input = Input {
        tx,
        queue: None,
        cancel: CancellationToken::new(),
        ended: false,
        binary: true,
        offset: 0,
        window: None,
        body_window: Arc::new(Semaphore::new(crate::secure_queue::BODY_WINDOW_BYTES)),
    };
    assert!(input.push("YQ", false, &budget).is_err());
    let push = |input: &mut Input, offset, end| {
        let plaintext = secure_body_record::encode(1, offset, if end { &[] } else { b"abc" }, end);
        input.push_binary(
            &secure_body_record::decode(&plaintext, secure_body_record::RESPONSE).expect("body"),
            &budget,
        )
    };
    assert!(push(&mut input, 1, false).is_err());
    assert!(push(&mut input, 0, false).is_ok());
    assert!(push(&mut input, 0, false).is_err());
    assert!(push(&mut input, 4, false).is_err());
    assert!(push(&mut input, 3, true).is_ok());
    assert!(push(&mut input, 3, true).is_err());
    drop(rx);
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
}

#[tokio::test(start_paused = true)]
async fn quiet_event_stream_uses_a_fresh_credit_deadline_when_data_returns() {
    use crate::secure_queue::Feedback;
    let global = Arc::new(Semaphore::new(crate::secure_queue::GLOBAL_BYTES));
    let closed = CancellationToken::new();
    let (output, outgoing) = Output::new(global.clone(), closed.clone());
    let (sent, mut records) = mpsc::unbounded_channel();
    let sink = Box::pin(futures_util::sink::unfold(sent, |tx, record| async move {
        tx.send(record).map_err(|_| ())?;
        Ok::<_, ()>(tx)
    }));
    let (feedback, rx) = mpsc::channel(1);
    let writer = tokio::spawn(crate::secure_channel::write_records(
        sink,
        |bytes| Ok(bytes.to_vec()),
        outgoing,
        rx,
        closed.clone(),
        output.budget.clone(),
    ));
    let (data, source) = mpsc::channel::<Bytes>(1);
    let body = Body::from_stream(futures_util::stream::unfold(source, |mut rx| async move {
        rx.recv()
            .await
            .map(|bytes| (Ok::<_, std::io::Error>(bytes), rx))
    }));
    let response = axum::response::Response::new(body);
    let response = tokio::spawn(respond_mode(
        response,
        1,
        output.with_http(1, true),
        closed.child_token(),
        closed.clone(),
        true,
        None,
        true,
        None,
    ));
    records.recv().await.unwrap();
    feedback.send(Feedback::Ack(0)).await.unwrap();
    data.send(Bytes::from_static(b"one")).await.unwrap();
    records.recv().await.unwrap();
    feedback
        .send(Feedback::Credit { id: 1, offset: 3 })
        .await
        .unwrap();
    output.send(json!({"op":"barrier"})).await.unwrap();
    records.recv().await.unwrap();
    feedback.send(Feedback::Ack(2)).await.unwrap();
    tokio::time::advance(crate::http_body_policy::IDLE * 2).await;
    data.send(Bytes::from_static(b"two")).await.unwrap();
    records.recv().await.unwrap();
    assert!(
        !closed.is_cancelled(),
        "quiet SSE is not an unconsumed file body"
    );
    feedback
        .send(Feedback::Credit { id: 1, offset: 6 })
        .await
        .unwrap();
    drop(data);
    records.recv().await.unwrap();
    response.await.unwrap();
    drop(output);
    writer.await.unwrap();
    assert_eq!(
        global.available_permits(),
        crate::secure_queue::GLOBAL_BYTES
    );
}
