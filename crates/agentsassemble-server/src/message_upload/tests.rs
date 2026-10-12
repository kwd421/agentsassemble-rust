use super::*;
use crate::http_body_policy;
use axum::{body::Bytes, extract::Request, response::IntoResponse};
use futures_util::stream;
use tokio::sync::mpsc;

type TestResult = Result<(), Box<dyn std::error::Error>>;
async fn fixture() -> (
    Service,
    MessageUploadAuthority,
    agentsassemble_domain::AuthenticatedPrincipal,
) {
    let (store, _, principal) = crate::attachment_download::tests::fixture(1).await;
    let auth = MessageUploadAuthority::Local {
        room_id: principal.room_id.clone(),
        user_id: principal.principal_id.clone(),
        participant_id: principal.participant_id.clone(),
    };
    (Service::new(store), auth, principal)
}
async fn begin(
    service: &Service,
    auth: &MessageUploadAuthority,
    op: &str,
    size: usize,
) -> Result<MessageUploadStatus, ProfileHttpError> {
    service
        .begin(
            auth.clone(),
            op.into(),
            "stream.bin".into(),
            "application/octet-stream".into(),
            size,
        )
        .await
}

#[tokio::test]
async fn admission_counts_stalled_transfers_and_cancel_releases_their_lease() -> TestResult {
    let (service, auth, principal) = fixture().await;
    let first = begin(&service, &auth, "one", 50 * 1024 * 1024).await?;
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    let body = Body::from_stream(stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }));
    let s = service.clone();
    let a = auth.clone();
    let h = first.handle.clone();
    let data = tokio::spawn(async move { s.data(a, h, 0, body, Policy::new()).await });
    tx.send(Ok(Bytes::from_static(b"accepted partial"))).await?;
    // Status observes durable progress while the data request is still stalled.
    loop {
        if service
            .status(auth.clone(), first.handle.clone())
            .await?
            .accepted_offset
            > 0
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    begin(&service, &auth, "two", 50 * 1024 * 1024).await?;
    begin(&service, &auth, "three", 50 * 1024 * 1024).await?;
    let deferred = begin(&service, &auth, "four", 50 * 1024 * 1024)
        .await
        .err()
        .ok_or("fourth must defer")?
        .into_response();
    assert_eq!(deferred.status(), StatusCode::TOO_MANY_REQUESTS);
    let deferred: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(deferred.into_body(), 4096).await?)?;
    assert_eq!(deferred["disposition"], "defer");
    assert!(deferred["retry_after_ms"].as_u64().unwrap_or(0) > 0);
    tokio::time::timeout(
        Duration::from_secs(1),
        service.store.execute_message(
            &principal,
            "stalled-write",
            "message.send",
            &serde_json::json!({"content":"unrelated"}),
        ),
    )
    .await??;
    service.cancel(auth.clone(), first.handle).await?;
    assert!(data.await?.is_err());
    begin(&service, &auth, "four", 50 * 1024 * 1024).await?;
    service.cleanup(true).await;
    assert!(service.entries.lock().await.is_empty());
    begin(&service, &auth, "room-loss", 1).await?;
    let snapshot = service.store.snapshot("general", 0, 100).await?;
    service
        .store
        .execute_room_lifecycle(
            agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&principal),
            "archive-upload-room",
            "room.archive",
            &serde_json::json!({"room_uid":snapshot.room.room_uid,"archived":true}),
        )
        .await?;
    service.cleanup(false).await;
    assert!(
        service.entries.lock().await.is_empty(),
        "room authority loss discards private staging"
    );
    Ok(())
}

#[tokio::test]
async fn interrupted_body_reports_offset_and_resumes_without_whole_file_assembly() -> TestResult {
    let (service, auth, _) = fixture().await;
    let first = begin(&service, &auth, "interrupted", 65536 + 19).await?;
    let body = Body::from_stream(stream::iter([
        Ok(Bytes::from(vec![7; 65536])),
        Err(std::io::Error::other("disconnected")),
    ]));
    let response = service
        .data(auth.clone(), first.handle.clone(), 0, body, Policy::new())
        .await
        .err()
        .ok_or("interrupted")?
        .into_response();
    let response: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 4096).await?)?;
    assert_eq!(response["accepted_offset"], 65536);
    assert_eq!(
        service
            .status(auth.clone(), first.handle.clone())
            .await?
            .accepted_offset,
        65536
    );
    service
        .data(
            auth.clone(),
            first.handle.clone(),
            65536,
            Body::from(vec![7; 19]),
            Policy::new(),
        )
        .await?;
    let (a, b) = tokio::join!(
        service.finish(auth.clone(), first.handle.clone()),
        service.finish(auth.clone(), first.handle.clone())
    );
    let finished = a?;
    assert_eq!(b?.attachment, finished.attachment);
    assert_eq!(
        service
            .finish(auth.clone(), first.handle.clone())
            .await?
            .attachment,
        finished.attachment
    );
    assert!(
        service
            .cancel(auth.clone(), first.handle.clone())
            .await
            .is_err()
    );
    service.cleanup(false).await;
    assert_eq!(
        service.status(auth, first.handle).await?.attachment,
        finished.attachment
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn server_storage_wait_is_excluded_but_the_hard_ceiling_and_client_idle_remain() -> TestResult
{
    let request = http_body_policy::wrap(Request::new(Body::from_stream(stream::iter([
        Ok::<_, std::io::Error>(Bytes::from_static(b"first")),
        Ok(Bytes::from_static(b"second")),
    ]))));
    let policy = request
        .extensions()
        .get::<Policy>()
        .ok_or("policy")?
        .clone();
    policy
        .authorize_upload()
        .map_err(|_| "authorization deadline")?;
    let mut body = request.into_body().into_data_stream();
    assert_eq!(body.next().await.ok_or("first")??, b"first".as_slice());
    let server = policy.server_wait();
    tokio::time::advance(Duration::from_secs(240)).await;
    drop(server);
    assert_eq!(body.next().await.ok_or("second")??, b"second".as_slice());
    tokio::time::advance(Duration::from_secs(31)).await;
    assert!(body.next().await.ok_or("idle")?.is_err());
    let request = http_body_policy::wrap(Request::new(Body::from("tail")));
    let policy = request
        .extensions()
        .get::<Policy>()
        .ok_or("policy")?
        .clone();
    policy
        .authorize_upload()
        .map_err(|_| "authorization deadline")?;
    let _server = policy.server_wait();
    tokio::time::advance(http_body_policy::UPLOAD_CEILING).await;
    assert!(
        request
            .into_body()
            .into_data_stream()
            .next()
            .await
            .ok_or("ceiling")?
            .is_err()
    );

    Ok(())
}
