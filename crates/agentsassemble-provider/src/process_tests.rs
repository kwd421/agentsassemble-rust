use std::{future::Future, io, pin::Pin, process::ExitStatus, time::Duration};

use process_wrap::tokio::ChildWrapper;
use tokio_util::sync::CancellationToken;

use super::{PROVIDER_ENVIRONMENT, ProbeFailure, probe, terminate_probe_tree};

#[derive(Debug)]
struct UnconfirmedCleanup;

impl ChildWrapper for UnconfirmedCleanup {
    fn inner(&self) -> &dyn ChildWrapper {
        self
    }

    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        self
    }

    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        self
    }

    fn start_kill(&mut self) -> io::Result<()> {
        Err(io::Error::other("synthetic signal failure"))
    }

    fn wait(&mut self) -> Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send + '_>> {
        Box::pin(async { Err(io::Error::other("synthetic wait failure")) })
    }
}

#[test]
fn probe_environment_has_no_credential_names() {
    assert!(PROVIDER_ENVIRONMENT.iter().all(|name| {
        !["KEY", "TOKEN", "SECRET", "PASSWORD"]
            .iter()
            .any(|marker| name.contains(marker))
            && !name.starts_with("AGENTSASSEMBLE_")
    }));
}

#[tokio::test]
async fn unconfirmed_probe_cleanup_is_a_distinct_failure() {
    assert_eq!(
        terminate_probe_tree(&mut UnconfirmedCleanup).await,
        Err(ProbeFailure::CleanupUnconfirmed)
    );
}

#[tokio::test]
async fn cancelled_probe_tree_is_killed_reaped_and_joinable() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create probe fixture: {error}"));
    let pid_path = directory.path().join("descendant.pid");
    let pid_text = pid_path.to_string_lossy().into_owned();
    let args = [
        "-c",
        "sleep 30 & echo $! > \"$1\"; wait",
        "sh",
        pid_text.as_str(),
    ];
    let cancellation = CancellationToken::new();
    let cancel = cancellation.clone();
    let cancel_after_descendant = async {
        for _ in 0..100 {
            if std::fs::read_to_string(&pid_path)
                .ok()
                .is_some_and(|value| value.trim().parse::<i32>().is_ok())
            {
                cancel.cancel();
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("probe descendant did not publish its pid");
    };
    let outcome = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            probe("/bin/sh", &args, &cancellation, &[]),
            cancel_after_descendant,
        )
        .0
    })
    .await
    .unwrap_or_else(|_| panic!("cancelled provider probe did not finish"));
    assert_eq!(outcome, Err(ProbeFailure::Cancelled));
    let descendant = tokio::fs::read_to_string(&pid_path)
        .await
        .unwrap_or_else(|error| panic!("read descendant pid: {error}"))
        .trim()
        .parse::<i32>()
        .unwrap_or_else(|error| panic!("parse descendant pid: {error}"));
    for _ in 0..100 {
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", &descendant.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if status.is_err() || status.is_ok_and(|status| !status.success()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("probe descendant {descendant} survived cancellation");
}
