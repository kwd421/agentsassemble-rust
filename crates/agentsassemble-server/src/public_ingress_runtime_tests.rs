//! Stale-credential fallback tests: a named connector that never registers,
//! one that fails closed with an auth line, one that registers late but
//! inside the bound, and one that registers then fails with an auth line.
//! The credentials themselves never enter assertions or logs.

use super::{NAMED_READY_TIMEOUT, run_generation};

const FALLBACK_TUNNEL_ID: &str = "fallback-tunnel-id";

fn fallback_credentials() -> String {
    serde_json::json!({
        "AccountTag": "account-tag",
        "TunnelID": FALLBACK_TUNNEL_ID,
        "TunnelSecret": "fallback-secret",
    })
    .to_string()
}

#[cfg(unix)]
async fn named_fallback_fixture(
    named_script_body: &str,
) -> (
    agentsassemble_persistence::SqliteStore,
    crate::public_ingress::ManagedIngressConfig,
    std::sync::Arc<parking_lot::RwLock<crate::public_ingress::ManagedProjection>>,
    u64,
    tempfile::TempDir,
) {
    use std::os::unix::fs::PermissionsExt;

    let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("store: {error}"));
    store
        .set_registration_epoch(Some("fallback-epoch"))
        .await
        .unwrap_or_else(|error| panic!("epoch: {error}"));
    store
        .save_named_tunnel(
            "h-0123456789ab.agentsassemble.uk",
            FALLBACK_TUNNEL_ID,
            &fallback_credentials(),
            "fallback-epoch",
        )
        .await
        .unwrap_or_else(|error| panic!("save named credential: {error}"));
    let identity = crate::central::host_identity::CentralHostIdentity::from_persistent(
        &store
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("identity: {error}")),
    )
    .unwrap_or_else(|error| panic!("host identity: {error}"));
    let context = crate::public_ingress::NamedTunnelContext {
        store: store.clone(),
        directory: crate::central::directory::CentralDirectory::disabled(),
        identity,
    };
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("test directory: {error}"));
    let executable = directory.path().join("cloudflared");
    std::fs::write(&executable, named_script_body)
        .unwrap_or_else(|error| panic!("write fake cloudflared: {error}"));
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("make fake cloudflared executable: {error}"));
    let stable_entry = crate::stable_entry::StableEntry::new(None, directory.path())
        .await
        .unwrap_or_else(|error| panic!("stable entry: {error}"));
    stable_entry.begin_generation();
    let config = crate::public_ingress::ManagedIngressConfig {
        local_url: "http://127.0.0.1:41955".to_owned(),
        cloudflared: Some(executable),
        stable_entry,
        named: Some(context),
    };
    let (projection, generation) =
        crate::public_ingress::managed_lifecycle_tests::started_projection(
            "http://127.0.0.1:41955",
        );
    let projection = std::sync::Arc::new(parking_lot::RwLock::new(projection));
    (store, config, projection, generation, directory)
}

#[cfg(unix)]
async fn wait_for_origin(
    projection: &std::sync::Arc<parking_lot::RwLock<crate::public_ingress::ManagedProjection>>,
    timeout: std::time::Duration,
) -> (u64, String) {
    let mut changes = projection.read().endpoint_changes.subscribe();
    tokio::time::timeout(timeout, async {
        loop {
            changes
                .changed()
                .await
                .unwrap_or_else(|_| panic!("endpoint channel closed"));
            let (generation, origin) = changes.borrow_and_update().clone();
            if !origin.is_empty() {
                return (generation, origin);
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("origin was not published"))
}

#[cfg(unix)]
#[tokio::test]
async fn stale_named_credential_falls_back_to_quick_after_the_bound() {
    use std::time::Duration;

    let (store, config, projection, generation, _directory) = named_fallback_fixture(
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
for a in "$@"; do
  if [ "$a" = "run" ]; then
    exec /bin/sleep 60
  fi
done
printf '%s\n' 'INF Visit https://quick-fallback-9.trycloudflare.com to connect'
exec /bin/sleep 60
"#,
    )
    .await;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task_projection = projection.clone();
    let task = tokio::spawn(async move {
        run_generation(generation, config, task_projection, task_cancellation).await
    });
    let (ready_generation, origin) = wait_for_origin(&projection, Duration::from_secs(10)).await;
    assert_eq!(ready_generation, generation);
    assert_eq!(origin, "https://quick-fallback-9.trycloudflare.com");
    assert!(
        store
            .named_tunnel()
            .await
            .unwrap_or_else(|error| panic!("read stored: {error}"))
            .is_none(),
        "stale credential must be cleared before the quick fallback"
    );
    cancellation.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .unwrap_or_else(|_| panic!("fallback generation did not stop"))
        .unwrap_or_else(|error| panic!("fallback generation owner failed: {error}"));
    assert!(
        !outcome.cleanup_failed,
        "fallback generation cleanup failed: {:?}",
        outcome.error
    );
}

#[cfg(unix)]
#[tokio::test]
async fn named_auth_error_falls_back_to_quick_immediately() {
    use std::time::Duration;

    let (store, config, projection, generation, _directory) = named_fallback_fixture(
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
for a in "$@"; do
  if [ "$a" = "run" ]; then
    printf '%s\n' 'ERR Unauthorized: invalid tunnel credentials'
    exit 1
  fi
done
printf '%s\n' 'INF Visit https://quick-fallback-9.trycloudflare.com to connect'
exec /bin/sleep 60
"#,
    )
    .await;
    let started = std::time::Instant::now();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task_projection = projection.clone();
    let task = tokio::spawn(async move {
        run_generation(generation, config, task_projection, task_cancellation).await
    });
    let (ready_generation, origin) = wait_for_origin(&projection, Duration::from_secs(10)).await;
    assert_eq!(ready_generation, generation);
    assert_eq!(origin, "https://quick-fallback-9.trycloudflare.com");
    assert!(
        started.elapsed() < NAMED_READY_TIMEOUT + Duration::from_secs(10),
        "auth failure must not wait out the full run"
    );
    assert!(
        store
            .named_tunnel()
            .await
            .unwrap_or_else(|error| panic!("read stored: {error}"))
            .is_none(),
        "revoked credential must be cleared before the quick fallback"
    );
    cancellation.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .unwrap_or_else(|_| panic!("fallback generation did not stop"))
        .unwrap_or_else(|error| panic!("fallback generation owner failed: {error}"));
    assert!(
        !outcome.cleanup_failed,
        "fallback generation cleanup failed: {:?}",
        outcome.error
    );
}

#[cfg(unix)]
#[tokio::test]
async fn late_named_registration_within_the_bound_keeps_the_credential() {
    use std::time::Duration;

    let (store, config, projection, generation, _directory) = named_fallback_fixture(
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
for a in "$@"; do
  if [ "$a" = "run" ]; then
    /bin/sleep 0.2
    printf '%s\n' 'INF Registered tunnel connection "test" connIndex=0'
    exec /bin/sleep 60
  fi
done
printf '%s\n' 'INF Visit https://quick-fallback-9.trycloudflare.com to connect'
exec /bin/sleep 60
"#,
    )
    .await;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task_projection = projection.clone();
    let task = tokio::spawn(async move {
        run_generation(generation, config, task_projection, task_cancellation).await
    });
    let (ready_generation, origin) = wait_for_origin(&projection, Duration::from_secs(10)).await;
    assert_eq!(ready_generation, generation);
    assert_eq!(origin, "https://h-0123456789ab.agentsassemble.uk");
    assert!(
        store
            .named_tunnel()
            .await
            .unwrap_or_else(|error| panic!("read stored: {error}"))
            .is_some(),
        "recovered named registration must keep the credential"
    );
    cancellation.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .unwrap_or_else(|_| panic!("named generation did not stop"))
        .unwrap_or_else(|error| panic!("named generation owner failed: {error}"));
    assert!(
        !outcome.cleanup_failed,
        "named generation cleanup failed: {:?}",
        outcome.error
    );
}

#[cfg(unix)]
#[tokio::test]
async fn post_ready_auth_failure_falls_back_to_quick_and_clears() {
    use std::time::Duration;

    let (store, config, projection, generation, _directory) = named_fallback_fixture(
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
for a in "$@"; do
  if [ "$a" = "run" ]; then
    printf '%s\n' 'INF Registered tunnel connection "test" connIndex=0'
    /bin/sleep 0.2
    printf '%s\n' 'ERR Unauthorized: invalid tunnel credentials' >&2
    exit 1
  fi
done
printf '%s\n' 'INF Visit https://quick-fallback-7.trycloudflare.com to connect'
exec /bin/sleep 60
"#,
    )
    .await;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task_projection = projection.clone();
    let mut changes = projection.read().endpoint_changes.subscribe();
    let task = tokio::spawn(async move {
        run_generation(generation, config, task_projection, task_cancellation).await
    });
    // The fixed origin is published first, then the post-ready auth failure
    // must replace it with the quick origin through the same endpoint path.
    let mut saw_fixed = false;
    let mut saw_quick = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            changes
                .changed()
                .await
                .unwrap_or_else(|_| panic!("endpoint channel closed"));
            let (_, origin) = changes.borrow_and_update().clone();
            if origin == "https://h-0123456789ab.agentsassemble.uk" {
                saw_fixed = true;
            } else if saw_fixed && origin == "https://quick-fallback-7.trycloudflare.com" {
                saw_quick = true;
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("post-ready fallback did not re-publish quick origin"));
    assert!(saw_fixed && saw_quick);
    assert!(
        store
            .named_tunnel()
            .await
            .unwrap_or_else(|error| panic!("read stored: {error}"))
            .is_none(),
        "post-ready revoked credential must be cleared before the quick fallback"
    );
    cancellation.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .unwrap_or_else(|_| panic!("fallback generation did not stop"))
        .unwrap_or_else(|error| panic!("fallback generation owner failed: {error}"));
    assert!(
        !outcome.cleanup_failed,
        "fallback generation cleanup failed: {:?}",
        outcome.error
    );
}
