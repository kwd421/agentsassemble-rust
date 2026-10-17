use super::*;
use chrono::TimeZone;

#[test]
fn quota_park_ends_at_next_utc_midnight_with_bounded_jitter() {
    let noon = chrono::Utc
        .with_ymd_and_hms(2026, 10, 17, 12, 0, 0)
        .unwrap()
        .timestamp();
    let midnight = chrono::Utc
        .with_ymd_and_hms(2026, 10, 18, 0, 0, 0)
        .unwrap()
        .timestamp();
    let next_midnight = chrono::Utc
        .with_ymd_and_hms(2026, 10, 19, 0, 0, 0)
        .unwrap()
        .timestamp();
    assert_eq!(quota_park_until_secs(noon, 0), midnight);
    assert_eq!(quota_park_until_secs(noon, 60), midnight + 60);
    assert_eq!(quota_park_until_secs(noon, 61), midnight + 60);
    assert_eq!(quota_park_until_secs(noon, -5), midnight);
    // Exactly at midnight the park targets the following midnight.
    assert_eq!(quota_park_until_secs(midnight, 0), next_midnight);
}

#[test]
fn retry_after_parses_seconds_dates_and_rejects_everything_else() {
    let now = chrono::Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).unwrap();
    assert_eq!(parse_retry_after_ms("2", now), Some(2_000));
    assert_eq!(parse_retry_after_ms("  45 ", now), Some(45_000));
    assert_eq!(parse_retry_after_ms("99999", now), Some(3_600_000));
    assert_eq!(
        parse_retry_after_ms("Sat, 17 Oct 2026 12:01:00 GMT", now),
        Some(60_000)
    );
    assert_eq!(parse_retry_after_ms("Sat, 17 Oct 2026 11:59:00 GMT", now), None);
    for invalid in ["", "soon", "-3", "1.5", "Mon, 99 Xxx 2026 00:00:00 GMT"] {
        assert_eq!(parse_retry_after_ms(invalid, now), None, "{invalid}");
    }
}

#[tokio::test]
async fn quota_refusal_parks_every_host_request_until_utc_midnight()
-> Result<(), Box<dyn std::error::Error>> {
    use axum::{Router, http::StatusCode, routing::any};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "Owner")
        .await?;
    store.set_registration_epoch(Some("quota-epoch")).await?;
    let identity = crate::CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let hits = Arc::new(AtomicUsize::new(0));
    let counting = hits.clone();
    let router = Router::new().route(
        "/{*path}",
        any(move || {
            let counting = counting.clone();
            async move {
                counting.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(serde_json::json!({"error": {"code": "actor_quota_exhausted"}})),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let directory =
        CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let shutdown = cancellation.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let inner = directory.0.as_ref().ok_or("configured")?;
    let path = format!("/v1/servers/{}/endpoint", identity.server_id());
    let result = send_signed(
        inner,
        &identity,
        &store,
        reqwest::Method::PUT,
        &path,
        serde_json::json!({"origin": "https://host.test", "generation": 1}),
    )
    .await;
    assert!(matches!(result, Err(CentralDirectoryError::QuotaExhausted)));
    let parked_until = inner.status.read().quota_parked_until;
    let midnight = (chrono::Utc::now().timestamp().div_euclid(86_400) + 1) * 86_400;
    assert!(
        (midnight..=midnight + 60).contains(&parked_until),
        "parked_until={parked_until} midnight={midnight}"
    );
    // Every host->central caller shares the gate: the parked retry spends
    // no request.
    for method in [reqwest::Method::PUT, reqwest::Method::POST] {
        let parked = send_signed(
            inner,
            &identity,
            &store,
            method,
            &path,
            serde_json::json!({"generation": 1}),
        )
        .await;
        assert!(matches!(parked, Err(CentralDirectoryError::QuotaExhausted)));
    }
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(directory.status().quota_parked_until, parked_until);
    cancellation.cancel();
    server.await??;
    Ok(())
}

#[tokio::test]
async fn temporary_capacity_refusal_parks_like_quota_until_utc_midnight()
-> Result<(), Box<dyn std::error::Error>> {
    use axum::{Router, http::StatusCode, routing::any};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "Owner")
        .await?;
    store.set_registration_epoch(Some("capacity-epoch")).await?;
    let identity = crate::CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let hits = Arc::new(AtomicUsize::new(0));
    let counting = hits.clone();
    let router = Router::new().route(
        "/{*path}",
        any(move || {
            let counting = counting.clone();
            async move {
                counting.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(
                        serde_json::json!({"error": {"code": "temporary_capacity_exhausted"}}),
                    ),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let directory =
        CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let shutdown = cancellation.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let inner = directory.0.as_ref().ok_or("configured")?;
    let path = format!("/v1/servers/{}/endpoint", identity.server_id());
    let result = send_signed(
        inner,
        &identity,
        &store,
        reqwest::Method::PUT,
        &path,
        serde_json::json!({"origin": "https://host.test", "generation": 1}),
    )
    .await;
    assert!(matches!(result, Err(CentralDirectoryError::QuotaExhausted)));
    let parked_until = inner.status.read().quota_parked_until;
    let midnight = (chrono::Utc::now().timestamp().div_euclid(86_400) + 1) * 86_400;
    assert!(
        (midnight..=midnight + 60).contains(&parked_until),
        "parked_until={parked_until} midnight={midnight}"
    );
    // The parked retry spends no request, exactly like actor_quota_exhausted.
    let parked = send_signed(
        inner,
        &identity,
        &store,
        reqwest::Method::PUT,
        &path,
        serde_json::json!({"generation": 1}),
    )
    .await;
    assert!(matches!(parked, Err(CentralDirectoryError::QuotaExhausted)));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(directory.status().quota_parked_until, parked_until);
    cancellation.cancel();
    server.await??;
    Ok(())
}

#[tokio::test]
async fn other_rate_limits_honour_retry_after_without_parking()
-> Result<(), Box<dyn std::error::Error>> {
    use axum::{
        Router,
        http::{StatusCode, header},
        routing::any,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "Owner")
        .await?;
    store.set_registration_epoch(Some("rate-epoch")).await?;
    let identity = crate::CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
    let hits = Arc::new(AtomicUsize::new(0));
    let counting = hits.clone();
    let router = Router::new().route(
        "/{*path}",
        any(move || {
            let counting = counting.clone();
            async move {
                counting.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, "120")],
                    axum::Json(serde_json::json!({"error": {"code": "per_minute_limited"}})),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let directory =
        CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let shutdown = cancellation.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let inner = directory.0.as_ref().ok_or("configured")?;
    let path = format!("/v1/servers/{}/endpoint", identity.server_id());
    let result = send_signed(
        inner,
        &identity,
        &store,
        reqwest::Method::PUT,
        &path,
        serde_json::json!({"generation": 1}),
    )
    .await;
    assert!(matches!(
        result,
        Err(CentralDirectoryError::CapacityExhausted)
    ));
    assert_eq!(inner.status.read().quota_parked_until, 0);
    assert!(*inner.rate_limited_until_ms.read() > chrono::Utc::now().timestamp_millis());
    // The gated retry spends no request either.
    let gated = send_signed(
        inner,
        &identity,
        &store,
        reqwest::Method::PUT,
        &path,
        serde_json::json!({"generation": 1}),
    )
    .await;
    assert!(matches!(
        gated,
        Err(CentralDirectoryError::CapacityExhausted)
    ));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    cancellation.cancel();
    server.await??;
    Ok(())
}
