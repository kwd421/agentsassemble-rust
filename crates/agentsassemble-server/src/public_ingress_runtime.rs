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
    public_ingress::{
        CanonicalPublicOrigin, ManagedIngressConfig, ManagedProjection, generated_origin_host,
    },
    public_ingress_process::{spawn_cloudflared, supervise_child},
};

const MAX_OUTPUT_LINE_BYTES: usize = 16 * 1024;
const OUTPUT_EVENTS: usize = 8;
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

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

pub(crate) async fn run_generation(
    generation: u64,
    config: ManagedIngressConfig,
    projection: Arc<RwLock<ManagedProjection>>,
    cancellation: CancellationToken,
) -> GenerationOutcome {
    let origin_host = generated_origin_host();
    let Some(cloudflared) = config.cloudflared.as_ref() else {
        return failed("cloudflared is not installed");
    };
    let Ok(spawned) = spawn_cloudflared(cloudflared, &config.local_url, &origin_host).await else {
        return failed("cloudflared could not be started");
    };
    let (output, mut output_events) = mpsc::channel(OUTPUT_EVENTS);
    let stdout_owner = spawn_output_reader(spawned.stdout, output.clone());
    let stderr_owner = spawn_output_reader(spawned.stderr, output);
    let child_cancellation = CancellationToken::new();
    let child_token = child_cancellation.clone();
    let mut child_owner = tokio::spawn(async move {
        let mut child = spawned.child;
        supervise_child(child.as_mut(), &child_token).await
    });
    let mut child_result = None;
    let mut output_closed = None;
    let mut error = None;
    let mut stopped = false;
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
                    let public_url = origin.value.clone();
                    if !projection.write().ready_managed(generation, origin, &origin_host) {
                        stopped = true;
                        break;
                    }
                    config
                        .stable_entry
                        .publish(&public_url, &cancellation)
                        .await;
                }
                Some(OutputEvent::Invalid) => {
                    error = Some("cloudflared output violated its safety limit".to_owned());
                    break;
                }
                None => {
                    output_closed = Some(child_owner.is_finished());
                    break;
                }
            },
            result = &mut child_owner => {
                error = Some(child_failure(&result));
                child_result = Some(result);
                break;
            }
        }
    }
    projection.write().revoke(generation);
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
    let reader_cleanup_failed =
        !join_reader(stdout_owner).await || !join_reader(stderr_owner).await;
    let stable_cleanup_failed = !config.stable_entry.clear().await;
    let cleanup_failed = child_cleanup_failed || reader_cleanup_failed || stable_cleanup_failed;
    if stopped && !cleanup_failed {
        error = None;
    } else if cleanup_failed && error.is_none() {
        error = Some("managed public ingress cleanup failed".to_owned());
    }
    GenerationOutcome {
        error,
        cleanup_failed,
    }
}

enum OutputEvent {
    Origin(CanonicalPublicOrigin),
    Invalid,
}

fn spawn_output_reader<R>(reader: R, events: mpsc::Sender<OutputEvent>) -> JoinHandle<()>
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
            if let Some(origin) = trycloudflare_origin(&line)
                && events.send(OutputEvent::Origin(origin)).await.is_err()
            {
                return;
            }
        }
    })
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

async fn join_reader(mut owner: JoinHandle<()>) -> bool {
    if let Ok(result) = tokio::time::timeout(READER_SHUTDOWN_TIMEOUT, &mut owner).await {
        result.is_ok()
    } else {
        owner.abort();
        let _ = owner.await;
        false
    }
}

fn failed(message: &str) -> GenerationOutcome {
    GenerationOutcome {
        error: Some(message.to_owned()),
        cleanup_failed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::trycloudflare_origin;

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
}
