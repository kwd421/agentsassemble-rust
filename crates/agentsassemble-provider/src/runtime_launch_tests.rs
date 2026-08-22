use std::{
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{DriverFactory, DriverFuture, ProviderAdapter, ProviderDriver};
#[cfg(target_os = "linux")]
use crate::{
    guardian::GuardianLaunch,
    runtime_lease::{LeaseObservation, observe_runtime_lease, unix_cleanup_receipt_is_present},
    unix_custody::{UnixProcessCustody, enable_test_escape_wait, test_escape_pid_path},
};
use crate::{launch_error::DriverLaunchError, runtime_lease::HeldRuntimeLease};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

struct NeverFactory {
    launches: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
}

impl DriverFactory for NeverFactory {
    fn launch<'a>(
        &'a self,
        _session: &'a agentsassemble_domain::DurableAgentSession,
        _runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        Box::pin(pending())
    }
}

#[test]
#[cfg(target_os = "linux")]
#[allow(clippy::zombie_processes)]
fn post_ready_escape_entry() {
    let Some(token) = std::env::var_os(crate::unix_process_tree::RUNTIME_TOKEN_ENV) else {
        return;
    };
    rustix::process::setsid()
        .unwrap_or_else(|error| panic!("create escaped provider session: {error}"));
    let child = Command::new("/bin/sh")
        .args(["-c", "exec 198>&-; exec /bin/sleep 30"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn untagged escaped provider: {error}"));
    std::fs::write(
        test_escape_pid_path(&token.to_string_lossy()),
        child.id().to_string(),
    )
    .unwrap_or_else(|error| panic!("publish escaped provider pid: {error}"));
}

#[tokio::test]
async fn cancelled_pre_ready_launch_retains_slot_custody() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create launch cancellation fixture: {error}"));
    let session = super::tests::fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let launches = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let adapter = ProviderAdapter::with_factory(Arc::new(NeverFactory {
        launches: Arc::clone(&launches),
        entered: Arc::clone(&entered),
    }));
    let pending_adapter = adapter.clone();
    let pending_session = session.clone();
    let task = tokio::spawn(async move { pending_adapter.start(&pending_session).await });
    entered.notified().await;
    task.abort();
    let _ = task.await;
    let Err(error) = adapter.start(&session).await else {
        panic!("cancelled launch must not permit a replacement");
    };
    assert!(error.effect_uncertain);
    assert!(!error.runtime_handle_id.is_empty());
    assert!(!error.runtime_owner_id.is_empty());
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    let outcome = adapter.shutdown_with_observations().await;
    assert!(outcome.failure.is_none());
    assert_eq!(outcome.gone.len(), 1);
    adapter.release_shutdown_observations(&outcome.gone).await;
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn post_ready_failure_is_safe_only_after_exact_guardian_receipt() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let suffix = uuid::Uuid::new_v4().to_string();
    let room_id = format!("launch-cleanup-room-{suffix}");
    let session_id = format!("launch-cleanup-session-{suffix}");
    let lease = HeldRuntimeLease::prepare(&room_id, &session_id)
        .unwrap_or_else(|error| panic!("prepare launch cleanup lease: {error}"));
    let pid_path = test_escape_pid_path(lease.token());
    let launch = GuardianLaunch::test_harness()
        .unwrap_or_else(|error| panic!("bind launch cleanup guardian: {error}"));
    let provider = crate::filesystem::bind_helper_executable_sync(
        &std::env::current_exe()
            .unwrap_or_else(|error| panic!("resolve launch cleanup provider: {error}")),
    )
    .unwrap_or_else(|error| panic!("bind launch cleanup provider: {error}"));
    enable_test_escape_wait();
    let result = UnixProcessCustody::start(
        &lease,
        &launch,
        &provider,
        &[
            "--exact".to_owned(),
            "runtime::launch_tests::post_ready_escape_entry".to_owned(),
            "--nocapture".to_owned(),
        ],
    )
    .await;
    let failure = match result {
        Err(failure) => failure,
        Ok((mut custody, _)) => {
            let _ = custody.stop().await;
            panic!("escaped provider must fail post-ready validation");
        }
    };
    assert!(!failure.effect_uncertain);
    assert!(
        unix_cleanup_receipt_is_present(lease.path(), lease.token())
            .unwrap_or_else(|error| panic!("inspect launch cleanup receipt: {error}"))
    );
    assert_eq!(
        observe_runtime_lease(&room_id, &session_id),
        LeaseObservation::Gone
    );
    let escaped_pid = std::fs::read_to_string(&pid_path)
        .unwrap_or_else(|error| panic!("read escaped provider pid: {error}"))
        .parse::<i32>()
        .unwrap_or_else(|error| panic!("parse escaped provider pid: {error}"));
    assert!(
        rustix::process::Pid::from_raw(escaped_pid).is_some_and(|pid| {
            matches!(
                rustix::process::test_kill_process(pid),
                Err(rustix::io::Errno::SRCH)
            )
        })
    );
    let _ = std::fs::remove_file(pid_path);
    lease.cleanup_pre_effect();
}
