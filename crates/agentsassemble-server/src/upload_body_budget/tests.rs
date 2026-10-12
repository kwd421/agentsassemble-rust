use super::*;
use crate::{http_api::decode_upload_body, http_body_policy::Policy};
use axum::body::{Body, Bytes};
use serde_json::Value;
use std::convert::Infallible;

#[tokio::test]
async fn slow_bodies_are_bounded_across_principals_and_refusal_does_not_consume() {
    let budget = Budget::default();
    let maximum = crate::http_api::MAX_BASE64_UPLOAD_BODY_BYTES;
    let mut tasks = Vec::new();
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    for index in 0..80 {
        let budget = budget.clone();
        let started = started.clone();
        let principal = if index < 40 {
            "one".to_owned()
        } else {
            format!("principal-{index}")
        };
        tasks.push(tokio::spawn(async move {
            let body = Body::from_stream(futures_util::stream::unfold(false, move |sent| {
                let started = started.clone();
                async move {
                    if sent {
                        started.send(()).expect("body retained");
                        std::future::pending::<()>().await;
                    }
                    Some((
                        Ok::<_, Infallible>(Bytes::from(vec![b' '; 1024 * 1024])),
                        true,
                    ))
                }
            }));
            decode_upload_body::<Value>(
                Request::new(body),
                maximum,
                &Policy::new(),
                &budget,
                &principal,
            )
            .await
        }));
    }
    let admitted = HOST_BYTES / (maximum * COPIES);
    for _ in 0..admitted {
        starts.recv().await.expect("body polled");
    }
    // Admission is acquired synchronously before any body can be polled.
    assert_eq!(
        budget.bytes.available_permits(),
        HOST_BYTES - admitted * maximum * COPIES
    );
    assert_eq!(budget.principals.lock().expect("lock").get("one"), Some(&2));
    let before = budget.bytes.available_permits();
    assert!(matches!(
        budget.reserve(Some("refused"), &Request::new(Body::empty()), maximum),
        Err(Error::Exhausted)
    ));
    assert_eq!(budget.bytes.available_permits(), before);
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    assert_eq!(budget.bytes.available_permits(), HOST_BYTES);
    assert!(budget.principals.lock().expect("lock").is_empty());
}

#[tokio::test(start_paused = true)]
async fn reservations_survive_decode_and_return_on_success_error_timeout_and_cancellation() {
    let budget = Budget::default();
    let (value, held) = decode_upload_body::<Value>(
        Request::new(Body::from("null")),
        32,
        &Policy::new(),
        &budget,
        "one",
    )
    .await
    .expect("decoded");
    assert_eq!(value, Value::Null);
    assert_eq!(budget.bytes.available_permits(), HOST_BYTES - 32 * COPIES);
    drop(held); // storage completion
    for body in ["no", "a body longer than four bytes"] {
        assert!(
            decode_upload_body::<Value>(
                Request::new(Body::from(body)),
                4,
                &Policy::new(),
                &budget,
                "one"
            )
            .await
            .is_err()
        );
        assert_eq!(budget.bytes.available_permits(), HOST_BYTES);
    }
    let request = Request::builder()
        .header("content-length", "1")
        .body(Body::from("null"))
        .expect("request");
    assert!(
        decode_upload_body::<Value>(request, 32, &Policy::new(), &budget, "one")
            .await
            .is_err()
    );
    assert_eq!(budget.bytes.available_permits(), HOST_BYTES);
    // Exercise the actual progress wrapper and its timeout cleanup.
    let router = axum::Router::new()
        .route(
            "/",
            axum::routing::post({
                let budget = budget.clone();
                move |policy: axum::Extension<Policy>, request: Request| async move {
                    assert!(matches!(
                        decode_upload_body::<Value>(request, 32, &policy, &budget, "one").await,
                        Err(Error::Body(BodyDecodeError::RequestTimeout))
                    ));
                    axum::http::StatusCode::REQUEST_TIMEOUT
                }
            }),
        )
        .layer(axum::middleware::from_fn(crate::http_body_policy::apply));
    use tower::ServiceExt;
    let body = Body::from_stream(futures_util::stream::pending::<Result<Bytes, Infallible>>());
    router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .body(body)
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(budget.bytes.available_permits(), HOST_BYTES);
    assert!(budget.principals.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn storage_work_retains_capacity_after_request_cancellation() {
    let budget = Budget::default();
    let (started, start) = tokio::sync::oneshot::channel();
    let (finish, finished) = tokio::sync::oneshot::channel();
    let (released, release) = tokio::sync::oneshot::channel();
    let request = tokio::spawn({
        let budget = budget.clone();
        async move {
            let (value, reservation) = decode_upload_body::<Value>(
                Request::new(Body::from("null")),
                32,
                &Policy::new(),
                &budget,
                "one",
            )
            .await
            .expect("decode");
            tokio::spawn(async move {
                let _reservation = reservation;
                let _value = value;
                started.send(()).expect("start");
                finished.await.expect("storage completion");
                drop(_reservation);
                released.send(()).expect("released");
            })
            .await
            .expect("worker");
        }
    });
    start.await.expect("started");
    request.abort();
    let _ = request.await;
    assert_eq!(budget.bytes.available_permits(), HOST_BYTES - 32 * COPIES);
    finish.send(()).expect("finish");
    release.await.expect("released");
    assert_eq!(budget.bytes.available_permits(), HOST_BYTES);
}

#[test]
fn maximum_room_upload_fits_unchanged_host_budget_and_releases_capacity() {
    let budget = Budget::default();
    let maximum =
        crate::http_api::base64_upload_body_limit(agentsassemble_domain::MAX_ROOM_FILE_BYTES);
    let request = Request::new(Body::empty());
    let (_, held) = budget
        .reserve(Some("large"), &request, maximum)
        .expect("50 MiB admitted");
    assert!(matches!(
        budget.reserve(Some("other"), &request, maximum),
        Err(Error::Exhausted)
    ));
    drop(held);
    assert!(budget.reserve(Some("other"), &request, maximum).is_ok());
    let too_long = Request::builder()
        .header("content-length", maximum)
        .body(Body::empty())
        .expect("request");
    assert!(matches!(
        budget.reserve(
            Some("small-room"),
            &too_long,
            crate::http_api::MAX_BASE64_UPLOAD_BODY_BYTES
        ),
        Err(Error::Body(BodyDecodeError::PayloadTooLarge))
    ));
}
