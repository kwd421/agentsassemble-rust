use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::{PROVIDER_ENVIRONMENT, ProbeFailure, probe};

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
            probe("/bin/sh", &args, &cancellation),
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
