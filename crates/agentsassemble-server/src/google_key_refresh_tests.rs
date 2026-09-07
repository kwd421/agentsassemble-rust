use super::*;
use crate::google_accounts::tests::checked;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinSet,
};

// A local rejecting CONNECT proxy exercises the real HTTPS failure path without
// contacting Google or adding a configurable production key source.
#[tokio::test]
async fn failed_refresh_is_shared_by_concurrent_requests_and_retried_after_cooldown() {
    let listener = checked(TcpListener::bind("127.0.0.1:0").await);
    let address = checked(listener.local_addr());
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let proxy = tokio::spawn(async move {
        loop {
            let (mut stream, _) = checked(listener.accept().await);
            let mut request = [0; 4096];
            let length = checked(stream.read(&mut request).await);
            assert!(request[..length].starts_with(b"CONNECT www.googleapis.com:443 "));
            observed.fetch_add(1, Ordering::SeqCst);
            checked(stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await);
        }
    });
    let mut verifier = checked(GoogleTokenVerifier::new("fixture-client".into()));
    verifier.client = checked(
        reqwest::Client::builder()
            .proxy(checked(reqwest::Proxy::https(format!("http://{address}"))))
            .build(),
    );
    let verifier = Arc::new(verifier);
    let mut requests = JoinSet::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    for _ in 0..8 {
        let verifier = verifier.clone();
        let barrier = barrier.clone();
        requests.spawn(async move {
            barrier.wait().await;
            let error = checked(
                verifier
                    .trusted_keys("missing-key")
                    .await
                    .err()
                    .ok_or("expected rejection"),
            );
            assert_eq!(error.code, "google_verification_unavailable");
        });
    }
    while let Some(result) = requests.join_next().await {
        checked(result);
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    tokio::time::pause();
    tokio::time::advance(Duration::from_mins(1)).await;
    assert!(verifier.trusted_keys("missing-key").await.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    // A failed retry must also start a cooldown, without supplying stale keys.
    assert!(verifier.trusted_keys("missing-key").await.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    verifier
        .install_fixture_keys(checked(serde_json::from_value(serde_json::json!({
            "keys": [{"kty": "RSA", "kid": "expired-key", "n": "AQAB", "e": "AQAB"}]
        }))))
        .await;
    tokio::time::advance(Duration::from_secs(3601)).await;
    assert!(verifier.trusted_keys("expired-key").await.is_err());
    assert!(verifier.trusted_keys("expired-key").await.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    proxy.abort();
    assert!(checked(proxy.await.err().ok_or("proxy cancellation")).is_cancelled());
}
