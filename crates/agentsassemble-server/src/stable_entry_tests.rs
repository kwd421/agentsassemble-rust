use std::path::PathBuf;

use super::{StableEntry, StableEntryConfig, StableEntryFile};
use crate::{
    public_ingress::{ManagedIngressConfig, PublicIngress, PublicIngressControlError},
    public_ingress_runtime::run_generation,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const DIRECT_ORIGIN: &str = "https://quick-entry.trycloudflare.com";
const SUCCESSOR_ORIGIN: &str = "https://successor-entry.trycloudflare.com";

#[test]
fn config_rejects_ambiguous_or_unbounded_authority() {
    for (url, namespace_id, kv_key) in [
        ("http://stable.example", "a".repeat(32), "target".to_owned()),
        (
            "https://stable.example/path",
            "a".repeat(32),
            "target".to_owned(),
        ),
        (
            "https://stable.example",
            "short".to_owned(),
            "target".to_owned(),
        ),
        (
            "https://stable.example",
            "z".repeat(32),
            "target".to_owned(),
        ),
        ("https://stable.example", "a".repeat(32), " ".to_owned()),
        (
            "https://stable.example",
            "a".repeat(32),
            "--remote".to_owned(),
        ),
        ("https://stable.example", "a".repeat(32), "x".repeat(129)),
    ] {
        assert!(
            StableEntryConfig::from_file(
                &StableEntryFile {
                    url: url.to_owned(),
                    namespace_id,
                    kv_key,
                },
                None,
            )
            .is_err(),
            "accepted invalid stable-entry configuration for {url}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn publish_and_clear_use_exact_remote_kv_operations_once() {
    let fixture = publisher_fixture("printf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 0");
    let entry = StableEntry::new(Some(config(&fixture.publisher)), fixture.directory.path());
    assert_eq!(entry.status().phase, "pending");

    entry
        .publish(DIRECT_ORIGIN, &CancellationToken::new())
        .await;
    let published = entry.status();
    assert_eq!(published.phase, "ready");
    assert_eq!(published.url, "https://stable.example");

    assert!(entry.clear().await);
    assert!(entry.clear().await);
    let cleared = entry.status();
    assert_eq!(cleared.phase, "ready");
    assert!(cleared.url.is_empty());
    entry.begin_generation();
    assert_eq!(entry.status().phase, "pending");
    assert_eq!(
        std::fs::read_to_string(fixture.publisher.with_extension("calls"))
            .unwrap_or_else(|error| panic!("read publisher calls: {error}"))
            .lines()
            .collect::<Vec<_>>(),
        [
            format!(
                "kv key put target {DIRECT_ORIGIN} --namespace-id={} --remote",
                "a".repeat(32)
            ),
            format!(
                "kv key delete target --namespace-id={} --remote",
                "a".repeat(32)
            ),
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn failed_clear_is_explicit_and_blocks_tunnel_restart() {
    let fixture = publisher_fixture("exit 0");
    let ingress = PublicIngress::managed(
        "127.0.0.1:41955"
            .parse()
            .unwrap_or_else(|error| panic!("parse listener: {error}")),
        Some(config_without_publisher()),
        fixture.directory.path(),
    );
    let status = ingress
        .stop()
        .await
        .unwrap_or_else(|error| panic!("stop managed ingress: {error}"));
    assert_eq!(status.tunnel.phase, "error");
    assert_eq!(status.tunnel.stable_phase, "failed");
    assert!(status.stable_url.is_empty());
    assert_eq!(
        status.tunnel.last_error.as_deref(),
        Some("stable-entry cleanup failed")
    );
    assert!(matches!(
        ingress.start().await,
        Err(PublicIngressControlError::CleanupFailed)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn managed_generation_publishes_and_clears_stable_target_before_completion() {
    let publisher = publisher_fixture("printf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 0");
    let tunnel = tunnel_fixture(&publisher.publisher.with_extension("calls"));
    let stable_entry = StableEntry::new(
        Some(config(&publisher.publisher)),
        publisher.directory.path(),
    );
    let (projection, generation) =
        crate::public_ingress::managed_lifecycle_tests::started_projection(
            "http://127.0.0.1:41955",
        );
    let projection = Arc::new(RwLock::new(projection));
    let outcome = run_generation(
        generation,
        ManagedIngressConfig {
            local_url: "http://127.0.0.1:41955".to_owned(),
            cloudflared: Some(tunnel.publisher.clone()),
            stable_entry: stable_entry.clone(),
        },
        projection,
        CancellationToken::new(),
    )
    .await;
    assert!(!outcome.cleanup_failed);
    assert!(outcome.error.is_some());
    assert_eq!(stable_entry.status().phase, "ready");
    assert!(stable_entry.status().url.is_empty());
    assert_eq!(attempt_count(&publisher.publisher), 3);
}

#[cfg(unix)]
#[tokio::test]
async fn superseded_owner_cannot_republish_or_clear_successor_target() {
    let fixture = publisher_fixture("printf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 0");
    let first = StableEntry::new(Some(config(&fixture.publisher)), fixture.directory.path());
    first
        .publish(DIRECT_ORIGIN, &CancellationToken::new())
        .await;

    let successor = StableEntry::new(Some(config(&fixture.publisher)), fixture.directory.path());
    successor
        .publish(SUCCESSOR_ORIGIN, &CancellationToken::new())
        .await;

    first
        .publish(DIRECT_ORIGIN, &CancellationToken::new())
        .await;
    assert!(first.clear().await);
    assert_eq!(first.status().phase, "failed");
    assert_eq!(
        first.status().last_error.as_deref(),
        Some("stable-entry ownership was superseded")
    );
    assert!(successor.clear().await);

    assert_eq!(
        std::fs::read_to_string(fixture.publisher.with_extension("calls"))
            .unwrap_or_else(|error| panic!("read publisher calls: {error}"))
            .lines()
            .collect::<Vec<_>>(),
        [
            format!(
                "kv key put target {DIRECT_ORIGIN} --namespace-id={} --remote",
                "a".repeat(32)
            ),
            format!(
                "kv key put target {SUCCESSOR_ORIGIN} --namespace-id={} --remote",
                "a".repeat(32)
            ),
            format!(
                "kv key delete target --namespace-id={} --remote",
                "a".repeat(32)
            ),
        ]
    );
}

#[cfg(unix)]
fn attempt_count(publisher: &std::path::Path) -> usize {
    std::fs::read_to_string(publisher.with_extension("calls"))
        .map_or(0, |calls| calls.lines().count())
}

#[cfg(unix)]
fn config(publisher: &std::path::Path) -> StableEntryConfig {
    StableEntryConfig::from_file(
        &StableEntryFile {
            url: "https://stable.example".to_owned(),
            namespace_id: "a".repeat(32),
            kv_key: "target".to_owned(),
        },
        Some(publisher.to_owned()),
    )
    .unwrap_or_else(|error| panic!("build stable config: {error}"))
}

#[cfg(unix)]
fn config_without_publisher() -> StableEntryConfig {
    StableEntryConfig::from_file(
        &StableEntryFile {
            url: "https://stable.example".to_owned(),
            namespace_id: "a".repeat(32),
            kv_key: "target".to_owned(),
        },
        None,
    )
    .unwrap_or_else(|error| panic!("build stable config: {error}"))
}

#[cfg(unix)]
struct PublisherFixture {
    directory: tempfile::TempDir,
    publisher: PathBuf,
}

#[cfg(unix)]
fn publisher_fixture(body: &str) -> PublisherFixture {
    use std::os::unix::fs::PermissionsExt;

    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create publisher fixture: {error}"));
    let publisher = directory.path().join("wrangler");
    std::fs::write(&publisher, format!("#!/bin/sh\n{body}\n"))
        .unwrap_or_else(|error| panic!("write publisher fixture: {error}"));
    std::fs::set_permissions(&publisher, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("make publisher executable: {error}"));
    PublisherFixture {
        directory,
        publisher,
    }
}

#[cfg(unix)]
fn tunnel_fixture(publication_marker: &std::path::Path) -> PublisherFixture {
    publisher_fixture(&format!(
        "printf '%s\\n' 'INF Visit https://quick-entry.trycloudflare.com'\nprintf '%s\\n' 'INF Visit https://quick-entry.trycloudflare.com'\nwhile [ ! -s '{}' ]; do :; done\nexit 1",
        publication_marker.display()
    ))
}
