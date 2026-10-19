use std::{io, process::ExitStatus, sync::Arc, time::Duration};

use futures_util::StreamExt;
use parking_lot::RwLock;
use tokio::{
    io::AsyncRead,
    sync::mpsc,
    task::{JoinError, JoinHandle},
};
use tokio_util::{
    codec::{FramedRead, LinesCodec},
    sync::CancellationToken,
};

use crate::{
    named_tunnel::{TunnelPlan, ensure_named_tunnel},
    public_ingress::{
        CanonicalPublicOrigin, ManagedIngressConfig, ManagedProjection, ManagedReadiness,
        generated_origin_host,
    },
    public_ingress_process::{spawn_cloudflared, spawn_cloudflared_named, supervise_child},
};

const MAX_OUTPUT_LINE_BYTES: usize = 16 * 1024;
const OUTPUT_EVENTS: usize = 8;
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
// A stored named credential can go stale while the host is offline (central
// reclaims the hostname or the owner revokes the tunnel). The connector
// then never logs a registered connection, so a named run must not wait
// forever: without readiness in this bound it falls back to quick.
#[cfg(not(test))]
const NAMED_READY_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const NAMED_READY_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) struct GenerationOutcome {
    pub(crate) error: Option<String>,
    pub(crate) cleanup_failed: bool,
}

impl GenerationOutcome {
    pub(crate) fn owner_failed() -> Self {
        Self {
            error: Some("managed public ingress owner task failed".to_owned()),
            cleanup_failed: true,
        }
    }
}

struct GenerationObservation {
    child_owner: JoinHandle<io::Result<ExitStatus>>,
    child_result: Option<Result<io::Result<ExitStatus>, JoinError>>,
    output_closed: Option<bool>,
    stable_publication: Option<StablePublication>,
    error: Option<String>,
    stopped: bool,
    publication_cleanup_failed: bool,
    named_fallback: Option<&'static str>,
}

pub(crate) async fn run_generation(
    generation: u64,
    config: ManagedIngressConfig,
    projection: Arc<RwLock<ManagedProjection>>,
    cancellation: CancellationToken,
) -> GenerationOutcome {
    // One provision attempt at most per open: a stored credential for the live
    // epoch runs named with zero central calls, otherwise a single attempt
    // decides between a fresh named credential and today's quick tunnel.
    // Every failure falls back to quick for this run and never blocks.
    let named = match config.named.as_ref() {
        Some(context) => {
            match ensure_named_tunnel(&context.store, &context.directory, &context.identity).await {
                TunnelPlan::Named(stored) => match CanonicalPublicOrigin::parse(&stored.origin()) {
                    Ok(origin) => Some((stored, origin)),
                    Err(_) => {
                        tracing::warn!(
                            %generation,
                            "named tunnel origin is invalid; using quick tunnel"
                        );
                        None
                    }
                },
                TunnelPlan::Quick { reason } => {
                    tracing::info!(
                        %generation,
                        reason,
                        "named tunnel unavailable; using quick tunnel"
                    );
                    None
                }
            }
        }
        None => None,
    };
    if let Some((stored, origin)) = named {
        return run_named_generation(
            generation,
            &config,
            &projection,
            &cancellation,
            origin,
            stored.tunnel_id(),
            stored.credentials(),
        )
        .await;
    }
    let origin_host = generated_origin_host();
    let Some(cloudflared) = config.cloudflared.as_ref() else {
        return failed_with_stable_cleanup(&config, "cloudflared is not installed").await;
    };
    let Ok(spawned) = spawn_cloudflared(cloudflared, &config.local_url, &origin_host).await else {
        return failed_with_stable_cleanup(&config, "cloudflared could not be started").await;
    };
    run_spawned_generation(
        generation,
        &config,
        &projection,
        &cancellation,
        spawned,
        origin_host,
        None,
    )
    .await
    .0
}

/// Runs one named generation with the fixed origin: publishes
/// `https://<hostname>` once cloudflared reports a registered connection.
/// A spawn failure, a bounded not-ready wait, a pre-ready or post-ready
/// connector auth failure, or a pre-ready or post-ready exit falls back to
/// the quick tunnel in the same run so external access never blocks on a
/// stale credential. The stale credential is cleared before the quick
/// fallback so the next open re-provisions.
async fn run_named_generation(
    generation: u64,
    config: &ManagedIngressConfig,
    projection: &Arc<RwLock<ManagedProjection>>,
    cancellation: &CancellationToken,
    origin: CanonicalPublicOrigin,
    tunnel_id: &str,
    credentials: &str,
) -> GenerationOutcome {
    let Some(cloudflared) = config.cloudflared.as_ref() else {
        return failed_with_stable_cleanup(config, "cloudflared is not installed").await;
    };
    // The named path uses the same per-run generated origin Host as the quick
    // path: cloudflared rewrites the request Host to that secret and forwards
    // the public hostname, so the ingress trust decision is identical for both
    // and no local process can reach the loopback port with the public Host.
    let origin_host = generated_origin_host();
    let spawned = match spawn_cloudflared_named(
        cloudflared,
        &config.local_url,
        &origin_host,
        tunnel_id,
        credentials,
    )
    .await
    {
        Ok(spawned) => spawned,
        Err(_) => {
            tracing::warn!(
                %generation,
                "named tunnel spawn failed; using quick tunnel"
            );
            return run_quick_generation(generation, config, projection, cancellation).await;
        }
    };
    let (outcome, fallback) = run_spawned_generation(
        generation,
        config,
        projection,
        cancellation,
        spawned,
        origin_host,
        Some(origin),
    )
    .await;
    let Some(reason) = fallback else {
        return outcome;
    };
    if let Some(context) = config.named.as_ref() {
        let _ = context.store.clear_named_tunnel().await;
    }
    tracing::warn!(
        %generation,
        reason,
        "named tunnel not usable; using quick tunnel"
    );
    run_quick_generation(generation, config, projection, cancellation).await
}

async fn run_quick_generation(
    generation: u64,
    config: &ManagedIngressConfig,
    projection: &Arc<RwLock<ManagedProjection>>,
    cancellation: &CancellationToken,
) -> GenerationOutcome {
    let Some(cloudflared) = config.cloudflared.as_ref() else {
        return failed_with_stable_cleanup(config, "cloudflared is not installed").await;
    };
    let origin_host = generated_origin_host();
    let Ok(spawned) = spawn_cloudflared(cloudflared, &config.local_url, &origin_host).await else {
        return failed_with_stable_cleanup(config, "cloudflared could not be started").await;
    };
    run_spawned_generation(
        generation,
        config,
        projection,
        cancellation,
        spawned,
        origin_host,
        None,
    )
    .await
    .0
}

#[allow(clippy::too_many_arguments)]
async fn run_spawned_generation(
    generation: u64,
    config: &ManagedIngressConfig,
    projection: &Arc<RwLock<ManagedProjection>>,
    cancellation: &CancellationToken,
    spawned: crate::public_ingress_process::SpawnedTunnel,
    origin_host: String,
    fixed_origin: Option<CanonicalPublicOrigin>,
) -> (GenerationOutcome, Option<&'static str>) {
    let named = fixed_origin.is_some();
    let (output, output_events) = mpsc::channel(OUTPUT_EVENTS);
    let stdout_owner = spawn_output_reader(spawned.stdout, output.clone(), named);
    let stderr_owner = spawn_output_reader(spawned.stderr, output, named);
    let child_cancellation = CancellationToken::new();
    let child_token = child_cancellation.clone();
    let child_owner = tokio::spawn(async move {
        let mut child = spawned.child;
        supervise_child(child.as_mut(), &child_token).await
    });
    let observation = observe_generation(
        generation,
        &origin_host,
        &config.stable_entry,
        &projection,
        &cancellation,
        child_owner,
        output_events,
        fixed_origin,
    )
    .await;
    let GenerationObservation {
        child_owner,
        child_result,
        output_closed,
        mut stable_publication,
        mut error,
        stopped,
        mut publication_cleanup_failed,
        named_fallback,
    } = observation;
    projection.write().revoke(generation);
    if let Some(publication) = stable_publication.as_ref() {
        publication.cancellation.cancel();
    }
    child_cancellation.cancel();
    let child_result = match child_result {
        Some(result) => result,
        None => child_owner.await,
    };
    if let Some(child_was_finished) = output_closed {
        error = Some(if child_was_finished {
            child_failure(&child_result)
        } else {
            "cloudflared closed its output before exiting".to_owned()
        });
    }
    let child_cleanup_failed = !matches!(child_result, Ok(Ok(_)));
    let reader_cleanup_failed = !join_output_readers(stdout_owner, stderr_owner).await;
    if !join_publication(&mut stable_publication).await {
        publication_cleanup_failed = true;
    }
    let stable_cleanup_failed = publication_cleanup_failed || !config.stable_entry.clear().await;
    let cleanup_failed = child_cleanup_failed || reader_cleanup_failed || stable_cleanup_failed;
    if stopped && !cleanup_failed {
        error = None;
    } else if cleanup_failed && error.is_none() {
        error = Some("managed public ingress cleanup failed".to_owned());
    }
    (
        GenerationOutcome {
            error,
            cleanup_failed,
        },
        named_fallback,
    )
}

async fn observe_generation(
    generation: u64,
    origin_host: &str,
    stable_entry: &crate::stable_entry::StableEntry,
    projection: &Arc<RwLock<ManagedProjection>>,
    cancellation: &CancellationToken,
    mut child_owner: JoinHandle<io::Result<ExitStatus>>,
    mut output_events: mpsc::Receiver<OutputEvent>,
    fixed_origin: Option<CanonicalPublicOrigin>,
) -> GenerationObservation {
    let mut child_result = None;
    let mut output_closed = None;
    let mut error = None;
    let mut stopped = false;
    let mut stable_publication = None;
    let mut pending_stable_target = None;
    let mut publication_cleanup_failed = false;
    let mut named_fallback: Option<&'static str> = None;
    let mut named_ready = false;
    let named_deadline = tokio::time::Instant::now() + NAMED_READY_TIMEOUT;
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                stopped = true;
                projection.write().begin_stop(generation);
                break;
            }
            event = output_events.recv() => match event {
                Some(OutputEvent::Origin(origin)) => {
                    if fixed_origin.is_some() {
                        // Named mode publishes only the fixed origin once the
                        // connector registers; quick-tunnel URLs never appear
                        // there and are ignored as diagnostics.
                        continue;
                    }
                    let target = origin.value.clone();
                    match projection
                        .write()
                        .ready_managed(generation, origin, origin_host)
                    {
                        ManagedReadiness::Unchanged => {}
                        ManagedReadiness::Changed => {
                            queue_publication(
                                stable_entry,
                                target,
                                &mut stable_publication,
                                &mut pending_stable_target,
                            );
                        }
                        ManagedReadiness::Rejected => {
                            stopped = true;
                            projection.write().begin_stop(generation);
                            break;
                        }
                    }
                }
                Some(OutputEvent::Invalid) => {
                    if fixed_origin.is_some() && !named_ready {
                        named_fallback = Some("named_output_invalid");
                        break;
                    }
                    error = Some("cloudflared output violated its safety limit".to_owned());
                    break;
                }
                Some(OutputEvent::NamedReady) => {
                    let Some(origin) = fixed_origin.clone() else {
                        continue;
                    };
                    named_ready = true;
                    let target = origin.value.clone();
                    match projection
                        .write()
                        .ready_managed(generation, origin, origin_host)
                    {
                        ManagedReadiness::Unchanged => {}
                        ManagedReadiness::Changed => {
                            queue_publication(
                                stable_entry,
                                target,
                                &mut stable_publication,
                                &mut pending_stable_target,
                            );
                        }
                        ManagedReadiness::Rejected => {
                            stopped = true;
                            projection.write().begin_stop(generation);
                            break;
                        }
                    }
                }
                Some(OutputEvent::NamedAuthFailure) => {
                    if fixed_origin.is_some() {
                        // Both pre-ready and post-ready credential failures
                        // fall back: a revoked tunnel or deleted hostname can
                        // surface after the first registered connection.
                        named_fallback = Some(if named_ready {
                            "named_auth_failed_after_ready"
                        } else {
                            "named_auth_failed"
                        });
                        break;
                    }
                }
                None => {
                    if fixed_origin.is_some() {
                        // Both pre-ready and post-ready exits fall back so a
                        // dead connector never leaves the fixed origin
                        // published without a live edge.
                        named_fallback = Some(if named_ready {
                            "named_exited_after_ready"
                        } else {
                            "named_exited_before_ready"
                        });
                        output_closed = Some(child_owner.is_finished());
                        break;
                    }
                    output_closed = Some(child_owner.is_finished());
                    break;
                }
            },
            result = &mut child_owner => {
                if fixed_origin.is_some() {
                    child_result = Some(result);
                    named_fallback = Some(if named_ready {
                        "named_exited_after_ready"
                    } else {
                        "named_exited_before_ready"
                    });
                    break;
                }
                error = Some(child_failure(&result));
                child_result = Some(result);
                break;
            }
            () = tokio::time::sleep_until(named_deadline), if fixed_origin.is_some() && !named_ready => {
                named_fallback = Some("named_ready_timeout");
                break;
            }
            result = wait_for_publication(&mut stable_publication), if stable_publication.is_some() => {
                stable_publication.take();
                if result.is_err() {
                    publication_cleanup_failed = true;
                    error = Some("stable-entry publication owner task failed".to_owned());
                    break;
                }
                start_pending_publication(
                    stable_entry,
                    &mut stable_publication,
                    &mut pending_stable_target,
                );
            }
        }
    }
    GenerationObservation {
        child_owner,
        child_result,
        output_closed,
        stable_publication,
        error,
        stopped,
        publication_cleanup_failed,
        named_fallback,
    }
}

struct StablePublication {
    cancellation: CancellationToken,
    owner: JoinHandle<()>,
}

fn queue_publication(
    stable_entry: &crate::stable_entry::StableEntry,
    target: String,
    active: &mut Option<StablePublication>,
    pending_target: &mut Option<String>,
) {
    *pending_target = Some(target);
    if let Some(publication) = active.as_ref() {
        publication.cancellation.cancel();
    } else {
        start_pending_publication(stable_entry, active, pending_target);
    }
}

fn start_pending_publication(
    stable_entry: &crate::stable_entry::StableEntry,
    active: &mut Option<StablePublication>,
    pending_target: &mut Option<String>,
) {
    let Some(target) = pending_target.take() else {
        return;
    };
    let cancellation = CancellationToken::new();
    let task_entry = stable_entry.clone();
    let task_cancellation = cancellation.clone();
    let owner = tokio::spawn(async move {
        task_entry.publish(&target, &task_cancellation).await;
    });
    *active = Some(StablePublication {
        cancellation,
        owner,
    });
}

async fn wait_for_publication(
    publication: &mut Option<StablePublication>,
) -> Result<(), JoinError> {
    let Some(publication) = publication.as_mut() else {
        std::future::pending().await
    };
    (&mut publication.owner).await
}

async fn join_publication(publication: &mut Option<StablePublication>) -> bool {
    let Some(mut publication) = publication.take() else {
        return true;
    };
    (&mut publication.owner).await.is_ok()
}

enum OutputEvent {
    Origin(CanonicalPublicOrigin),
    NamedReady,
    NamedAuthFailure,
    Invalid,
}

fn spawn_output_reader<R>(
    reader: R,
    events: mpsc::Sender<OutputEvent>,
    named: bool,
) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = FramedRead::new(
            reader,
            LinesCodec::new_with_max_length(MAX_OUTPUT_LINE_BYTES),
        );
        while let Some(line) = lines.next().await {
            let Ok(line) = line else {
                let _ = events.send(OutputEvent::Invalid).await;
                return;
            };
            if named {
                if named_tunnel_auth_failure(&line) {
                    if events.send(OutputEvent::NamedAuthFailure).await.is_err() {
                        return;
                    }
                } else if named_tunnel_ready(&line)
                    && events.send(OutputEvent::NamedReady).await.is_err()
                {
                    return;
                }
            } else if let Some(origin) = trycloudflare_origin(&line)
                && events.send(OutputEvent::Origin(origin)).await.is_err()
            {
                return;
            }
        }
    })
}

/// Reports whether one cloudflared log line confirms the named connector has
/// a registered edge connection. Readiness waits for this edge registration
/// rather than mere process startup, so the fixed origin is only published
/// once Cloudflare can actually route it.
pub(crate) fn named_tunnel_ready(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("registered") && lower.contains("connection") && !lower.contains("unregistered")
}

/// Reports whether one cloudflared log line is a clear credential failure
/// (revoked token, deleted tunnel) rather than a transient network blip.
/// Only these lines trigger an immediate named-to-quick fallback; ordinary
/// dial/timeout/reset lines keep waiting for readiness until the bound.
pub(crate) fn named_tunnel_auth_failure(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("unauthorized")
        || lower.contains("unauthorised")
        || (lower.contains("invalid") && lower.contains("tunnel"))
        || (lower.contains("invalid") && lower.contains("token"))
        || (lower.contains("authentication") && lower.contains("fail"))
        || (lower.contains("failed") && lower.contains("authenticat"))
        || (lower.contains("tunnel") && lower.contains("not found"))
        || lower.contains("unknown tunnel")
        || lower.contains("tunnel-not-found")
        || (lower.contains("no such") && lower.contains("tunnel"))
}

fn trycloudflare_origin(line: &str) -> Option<CanonicalPublicOrigin> {
    let start = line.find("https://")? + "https://".len();
    let tail = &line[start..];
    let end = tail
        .bytes()
        .position(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.'))
        .unwrap_or(tail.len());
    let host = &tail[..end];
    let label = host.strip_suffix(".trycloudflare.com")?;
    if label.is_empty()
        || label.len() > 63
        || label.starts_with('-')
        || label.ends_with('-')
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let expected = format!("https://{}", host.to_ascii_lowercase());
    CanonicalPublicOrigin::parse(&expected)
        .ok()
        .filter(|origin| origin.value == expected)
}

fn child_failure(result: &Result<io::Result<ExitStatus>, JoinError>) -> String {
    match result {
        Ok(Ok(status)) => format!("cloudflared exited with status {status}"),
        Ok(Err(_)) => "cloudflared process supervision failed".to_owned(),
        Err(_) => "cloudflared process owner stopped unexpectedly".to_owned(),
    }
}

async fn join_output_readers(stdout: JoinHandle<()>, stderr: JoinHandle<()>) -> bool {
    let (stdout_clean, stderr_clean) = tokio::join!(join_reader(stdout), join_reader(stderr));
    stdout_clean && stderr_clean
}

async fn join_reader(mut owner: JoinHandle<()>) -> bool {
    if let Ok(result) = tokio::time::timeout(READER_SHUTDOWN_TIMEOUT, &mut owner).await {
        result.is_ok()
    } else {
        owner.abort();
        let _ = owner.await;
        false
    }
}

async fn failed_with_stable_cleanup(
    config: &ManagedIngressConfig,
    message: &str,
) -> GenerationOutcome {
    let cleanup_failed = !config.stable_entry.clear().await;
    GenerationOutcome {
        error: Some(if cleanup_failed {
            "managed public ingress cleanup failed".to_owned()
        } else {
            message.to_owned()
        }),
        cleanup_failed,
    }
}

#[cfg(test)]
mod tests {
    use super::{named_tunnel_auth_failure, named_tunnel_ready, trycloudflare_origin};

    #[tokio::test(start_paused = true)]
    async fn failed_stdout_cleanup_still_joins_and_aborts_blocked_stderr() {
        let stdout = tokio::spawn(std::future::pending::<()>());
        stdout.abort();
        let stderr = tokio::spawn(std::future::pending::<()>());
        let stderr_observation = stderr.abort_handle();
        assert!(!super::join_output_readers(stdout, stderr).await);
        assert!(stderr_observation.is_finished());
    }

    #[test]
    fn output_parser_extracts_one_exact_trycloudflare_tenant() {
        let origin =
            trycloudflare_origin("INF Visit https://Soft-River-7.trycloudflare.com to connect")
                .unwrap_or_else(|| panic!("valid quick-tunnel origin was not extracted"));
        assert_eq!(origin.value, "https://soft-river-7.trycloudflare.com");
    }

    #[test]
    fn output_parser_rejects_deceptive_or_non_tenant_hosts() {
        for line in [
            "https://trycloudflare.com",
            "https://-tenant.trycloudflare.com",
            "https://tenant-.trycloudflare.com",
            "https://nested.tenant.trycloudflare.com",
            "https://tenant.trycloudflare.com.example",
        ] {
            assert!(
                trycloudflare_origin(line).is_none(),
                "accepted deceptive output: {line}"
            );
        }
    }

    #[test]
    fn named_readiness_waits_for_the_registered_connection_line() {
        for line in [
            "INF Registered tunnel connection \"a\" connIndex=0",
            "2026-10-19T00:00:00Z INF Connection 123 registered with Cloudflare",
            "inf registered tunnel CONNECTION established",
        ] {
            assert!(named_tunnel_ready(line), "missed readiness: {line}");
        }
        for line in [
            "INF Visit https://soft-river-7.trycloudflare.com to connect",
            "INF Unregistered tunnel connection closed",
            "ERR tunnel connection failed",
            "hello world",
            "",
        ] {
            assert!(!named_tunnel_ready(line), "false readiness: {line}");
        }
    }

    /// Reads the rewritten request Host from one recorded cloudflared argv.
    #[cfg(unix)]
    fn named_host_header(arguments: &str) -> String {
        let mut lines = arguments.lines();
        while let Some(line) = lines.next() {
            if line == "--http-host-header" {
                return lines
                    .next()
                    .unwrap_or_else(|| panic!("missing --http-host-header value"))
                    .to_owned();
            }
        }
        panic!("the named spawn did not set --http-host-header");
    }

    /// Builds one managed request: the local Host, the forwarded public
    /// authority when the edge supplied one, and the forwarded scheme.
    #[cfg(unix)]
    fn named_request_headers(host: &str, forwarded_host: Option<&str>) -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::try_from(host)
                .unwrap_or_else(|error| panic!("request Host: {error}")),
        );
        if let Some(forwarded) = forwarded_host {
            headers.insert(
                "x-forwarded-host",
                axum::http::HeaderValue::try_from(forwarded)
                    .unwrap_or_else(|error| panic!("forwarded host: {error}")),
            );
        }
        headers.insert(
            "x-forwarded-proto",
            axum::http::HeaderValue::from_static("https"),
        );
        headers
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn named_generation_publishes_the_fixed_origin_on_registered_connection() {
        use std::{os::unix::fs::PermissionsExt, time::Duration};

        use agentsassemble_persistence::SqliteStore;

        use crate::{
            product_surface::RouteExposure,
            public_ingress::managed_lifecycle_tests::managed_trust_authorizes,
        };

        const TUNNEL_ID: &str = "named-test-tunnel-id";
        const CREDENTIALS: &str = r#"{"AccountTag":"account-tag","TunnelID":"named-test-tunnel-id","TunnelSecret":"named-test-secret"}"#;
        const PUBLIC_HOSTNAME: &str = "h-0123456789ab.agentsassemble.uk";

        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("store: {error}"));
        store
            .set_registration_epoch(Some("fixed-epoch"))
            .await
            .unwrap_or_else(|error| panic!("epoch: {error}"));
        store
            .save_named_tunnel(PUBLIC_HOSTNAME, TUNNEL_ID, CREDENTIALS, "fixed-epoch")
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

        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("test directory: {error}"));
        let executable = directory.path().join("cloudflared");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
printf '%s' "$TUNNEL_CRED_CONTENTS" > "$0.credentials"
printf '%s' "${TUNNEL_TOKEN-unset}" > "$0.token"
printf '%s\n' 'INF Registered tunnel connection "test" connIndex=0'
exec /bin/sleep 60
"#,
        )
        .unwrap_or_else(|error| panic!("write fake cloudflared: {error}"));
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("make fake cloudflared executable: {error}"));

        let stable_entry = crate::stable_entry::StableEntry::new(None, directory.path())
            .await
            .unwrap_or_else(|error| panic!("stable entry: {error}"));
        stable_entry.begin_generation();
        let config = crate::public_ingress::ManagedIngressConfig {
            local_url: "http://127.0.0.1:41955".to_owned(),
            cloudflared: Some(executable.clone()),
            stable_entry,
            named: Some(context),
        };
        let (projection, generation) =
            crate::public_ingress::managed_lifecycle_tests::started_projection(
                "http://127.0.0.1:41955",
            );
        let projection = std::sync::Arc::new(parking_lot::RwLock::new(projection));
        let mut changes = projection.read().endpoint_changes.subscribe();
        let cancellation = tokio_util::sync::CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task_projection = projection.clone();
        let task = tokio::spawn(async move {
            super::run_generation(generation, config, task_projection, task_cancellation).await
        });
        let (ready_generation, origin) = tokio::time::timeout(Duration::from_secs(10), async {
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
        .unwrap_or_else(|_| panic!("fixed origin was not published"));
        assert_eq!(ready_generation, generation);
        assert_eq!(origin, format!("https://{PUBLIC_HOSTNAME}"));
        let arguments = tokio::fs::read_to_string(executable.with_extension("args"))
            .await
            .unwrap_or_else(|error| panic!("read fake cloudflared arguments: {error}"));
        assert!(arguments.contains("--no-autoupdate"));
        assert!(arguments.contains("run"));
        assert!(arguments.contains("http://127.0.0.1:41955"));
        assert!(arguments.contains(TUNNEL_ID));
        assert!(
            !arguments.contains("named-test-secret"),
            "tunnel credentials must never appear in argv"
        );
        assert!(
            !arguments.contains("--token"),
            "tunnel credentials must travel via TUNNEL_CRED_CONTENTS, not --token"
        );
        // The named path must rewrite the request Host exactly like the quick
        // path, and the running generation must trust that generated secret.
        let origin_host = named_host_header(&arguments);
        assert!(
            origin_host.ends_with(".origin.invalid"),
            "the named connector must rewrite the request Host to the generated secret"
        );
        assert!(
            !origin_host.contains("agentsassemble.uk"),
            "the public hostname must never become the local request Host"
        );
        let trusted = named_request_headers(&origin_host, Some(PUBLIC_HOSTNAME));
        assert!(
            managed_trust_authorizes(
                &projection.read(),
                &trusted,
                RouteExposure::SameOriginPublic
            ),
            "the running named generation must trust its own generated Host"
        );
        let forged = named_request_headers(PUBLIC_HOSTNAME, None);
        assert!(
            !managed_trust_authorizes(&projection.read(), &forged, RouteExposure::SameOriginPublic),
            "the public hostname alone must never be trusted"
        );
        assert_eq!(
            tokio::fs::read_to_string(executable.with_extension("credentials"))
                .await
                .unwrap_or_else(|error| panic!("read fake cloudflared credentials: {error}")),
            CREDENTIALS
        );
        assert_eq!(
            tokio::fs::read_to_string(executable.with_extension("token"))
                .await
                .unwrap_or_else(|error| panic!("read fake cloudflared token: {error}")),
            "unset"
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

    #[test]
    fn named_auth_failure_detects_only_credential_errors() {
        for line in [
            "ERR Unauthorized: invalid tunnel credentials",
            "ERR failed to authenticate tunnel: unauthorized",
            "ERR Tunnel \"abc\" not found",
            "ERR unknown tunnel id",
            "ERR Invalid tunnel credentials",
            "ERR authentication failed for tunnel",
            "ERR tunnel-not-found",
        ] {
            assert!(
                named_tunnel_auth_failure(line),
                "missed auth failure: {line}"
            );
        }
        for line in [
            "INF Registered tunnel connection \"a\" connIndex=0",
            "ERR tunnel connection failed: dial tcp: i/o timeout",
            "ERR failed to connect: connection reset by peer",
            "INF Retrying connection in 5s",
            "hello world",
            "",
        ] {
            assert!(
                !named_tunnel_auth_failure(line),
                "false auth failure: {line}"
            );
        }
    }
}

#[cfg(test)]
#[path = "public_ingress_runtime_tests.rs"]
mod fallback_tests;
