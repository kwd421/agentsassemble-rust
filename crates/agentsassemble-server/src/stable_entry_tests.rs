use std::path::PathBuf;

#[cfg(unix)]
use std::time::Duration;

use super::{StableEntry, StableEntryConfig, StableEntryFile};
#[cfg(unix)]
use crate::{AppState, HostSecret, TicketStore, serve};
use crate::{
    public_ingress::{ManagedIngressConfig, PublicIngress, PublicIngressControlError},
    public_ingress_runtime::run_generation,
};
#[cfg(unix)]
use agentsassemble_domain::ProviderCatalog;
#[cfg(unix)]
use agentsassemble_persistence::SqliteStore;
#[cfg(unix)]
use agentsassemble_provider::ProviderCatalogService;
use parking_lot::RwLock;
use std::sync::Arc;
#[cfg(unix)]
use tokio::net::TcpListener;
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
    let config = config(&fixture.publisher);
    let config_argument = wrangler_config_argument(&config);
    assert_eq!(
        std::fs::read_to_string(
            config
                .publisher_config
                .as_ref()
                .unwrap_or_else(|| panic!("publisher config is unavailable"))
                .path()
                .join("wrangler.json")
        )
        .unwrap_or_else(|error| panic!("read isolated wrangler config: {error}")),
        "{}\n"
    );
    let entry = StableEntry::new(Some(config), fixture.directory.path())
        .await
        .unwrap_or_else(|error| panic!("activate stable entry: {error}"));
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
                "kv key put target {DIRECT_ORIGIN} --namespace-id={} --remote {config_argument}",
                "a".repeat(32),
            ),
            format!(
                "kv key delete target --namespace-id={} --remote {config_argument}",
                "a".repeat(32),
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
    )
    .await
    .unwrap_or_else(|error| panic!("activate stable entry: {error}"));
    let status = ingress
        .stop(std::num::NonZeroU64::MIN)
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
        ingress
            .start(std::num::NonZeroU64::new(2).unwrap_or_else(|| unreachable!()))
            .await,
        Err(PublicIngressControlError::CleanupFailed)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn managed_generation_publishes_and_clears_stable_target_before_completion() {
    let publisher = publisher_fixture(
        "printf '%s\\n' \"$*\" >> \"$0.calls\"\nif [ \"$3\" = put ]; then exec /bin/sleep 600; fi\nexit 0",
    );
    let tunnel = tunnel_fixture(&publisher.publisher.with_extension("calls"));
    let stable_entry = StableEntry::new(
        Some(config(&publisher.publisher)),
        publisher.directory.path(),
    )
    .await
    .unwrap_or_else(|error| panic!("activate stable entry: {error}"));
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
    assert_eq!(attempt_count(&publisher.publisher), 2);
}

#[cfg(unix)]
#[tokio::test]
async fn tunnel_spawn_failure_still_clears_the_stable_target() {
    let publisher = publisher_fixture("printf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 0");
    let stable_entry = StableEntry::new(
        Some(config(&publisher.publisher)),
        publisher.directory.path(),
    )
    .await
    .unwrap_or_else(|error| panic!("activate stable entry: {error}"));
    let (projection, generation) =
        crate::public_ingress::managed_lifecycle_tests::started_projection(
            "http://127.0.0.1:41955",
        );
    let outcome = run_generation(
        generation,
        ManagedIngressConfig {
            local_url: "http://127.0.0.1:41955".to_owned(),
            cloudflared: Some(publisher.directory.path().join("missing-cloudflared")),
            stable_entry,
        },
        Arc::new(RwLock::new(projection)),
        CancellationToken::new(),
    )
    .await;

    assert_eq!(
        outcome.error.as_deref(),
        Some("cloudflared could not be started")
    );
    assert!(!outcome.cleanup_failed);
    assert_eq!(attempt_count(&publisher.publisher), 1);
    assert!(
        std::fs::read_to_string(publisher.publisher.with_extension("calls"))
            .unwrap_or_else(|error| panic!("read publisher calls: {error}"))
            .starts_with("kv key delete target ")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn serve_preflight_failure_clears_claimed_stable_entry() {
    let fixture = publisher_fixture("printf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 0");
    let database = fixture.directory.path().join("runtime.sqlite3");
    let store = SqliteStore::open_path(&database)
        .await
        .unwrap_or_else(|error| panic!("open cleanup test store: {error}"));
    store
        .bootstrap_local_authority("018f301c-e3bf-4b1c-82dd-5853bacb837f", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap cleanup test store: {error}"));
    let state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 8),
        HostSecret::new("stable-cleanup-test-host-token-000001")
            .unwrap_or_else(|error| panic!("build cleanup host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build cleanup app state: {error}"));
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .unwrap_or_else(|error| panic!("bind unsupported cleanup listener: {error}"));
    let listener_address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read cleanup listener: {error}"));
    let state = state
        .with_managed_public_ingress(
            listener_address,
            Some(config(&fixture.publisher)),
            fixture.directory.path(),
        )
        .await
        .unwrap_or_else(|error| panic!("activate cleanup stable entry: {error}"));

    assert!(
        serve(listener, state, CancellationToken::new())
            .await
            .is_err()
    );
    let calls = std::fs::read_to_string(fixture.publisher.with_extension("calls"))
        .unwrap_or_else(|error| panic!("read cleanup publisher calls: {error}"));
    assert_eq!(calls.lines().count(), 1);
    assert!(calls.starts_with("kv key delete target "));
}

#[cfg(unix)]
#[tokio::test]
async fn predecessor_without_an_operation_cannot_take_back_successor_ownership() {
    let fixture = publisher_fixture("printf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 0");
    let first = StableEntry::new(Some(config(&fixture.publisher)), fixture.directory.path())
        .await
        .unwrap_or_else(|error| panic!("activate first stable entry: {error}"));

    let successor_config = config(&fixture.publisher);
    let config_argument = wrangler_config_argument(&successor_config);
    let successor = StableEntry::new(Some(successor_config), fixture.directory.path())
        .await
        .unwrap_or_else(|error| panic!("activate successor stable entry: {error}"));
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
                "kv key put target {SUCCESSOR_ORIGIN} --namespace-id={} --remote {config_argument}",
                "a".repeat(32),
            ),
            format!(
                "kv key delete target --namespace-id={} --remote {config_argument}",
                "a".repeat(32),
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
fn wrangler_config_argument(config: &StableEntryConfig) -> String {
    format!(
        "--config={}",
        config
            .publisher_config
            .as_ref()
            .unwrap_or_else(|| panic!("publisher config is unavailable"))
            .path()
            .join("wrangler.json")
            .display()
    )
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
