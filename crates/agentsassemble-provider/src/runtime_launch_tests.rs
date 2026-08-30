use std::{
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use super::{
    DriverError, DriverFactory, DriverFuture, ProductionDriverFactory, ProviderAdapter,
    ProviderDriver, ProviderRuntimeObservation,
};
use crate::{
    guardian::GuardianLaunch, launch_error::DriverLaunchError, runtime_lease::HeldRuntimeLease,
};
#[cfg(target_os = "linux")]
use crate::{
    runtime_lease::{LeaseObservation, observe_runtime_lease, unix_cleanup_receipt_is_present},
    unix_custody::{UnixProcessCustody, enable_test_escape_wait, test_escape_pid_path},
};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

struct NeverFactory {
    launches: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
}

struct SafeFailureFactory;

impl DriverFactory for SafeFailureFactory {
    fn launch<'a>(
        &'a self,
        _session: &'a agentsassemble_domain::DurableAgentSession,
        _runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
        Box::pin(async {
            Err(DriverLaunchError::safe(DriverError::new(
                "provider_launch_failed",
                "The provider launch failed before a runtime remained alive.",
            )))
        })
    }
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
    assert!(outcome.failure.is_some());
    assert!(outcome.gone.is_empty());
    drop(adapter);
    crate::runtime_lease::cleanup_stale_runtime_lease(
        &session.public.room_id,
        &session.public.session_id,
    );
}

#[tokio::test]
async fn safe_launch_failure_retains_exact_gone_proof_until_terminal_commit() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create safe launch failure fixture: {error}"));
    let session = super::tests::fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let adapter = ProviderAdapter::with_factory(Arc::new(SafeFailureFactory));
    let reservation = adapter
        .reserve_start(&session)
        .await
        .unwrap_or_else(|error| panic!("reserve safe launch failure: {error}"));
    let mut authorized = session.clone();
    authorized
        .runtime_handle_id
        .clone_from(&reservation.runtime_handle_id);
    authorized
        .runtime_owner_id
        .clone_from(&reservation.runtime_owner_id);
    authorized
        .runtime_lease_token
        .clone_from(&reservation.runtime_lease_token);

    let Err(error) = adapter.start_reserved(&authorized).await else {
        panic!("safe launch failure must remain a terminal error");
    };
    assert!(error.runtime_stopped);
    assert_eq!(error.runtime_lease_token, reservation.runtime_lease_token);
    assert_eq!(
        adapter.observe(&authorized).await,
        ProviderRuntimeObservation::Gone
    );

    drop(adapter);
    assert_eq!(
        crate::runtime_lease::observe_runtime_lease(
            &authorized.public.room_id,
            &authorized.public.session_id,
        ),
        crate::runtime_lease::LeaseObservation::GenerationGone {
            launch_token: reservation.runtime_lease_token,
        }
    );
    crate::runtime_lease::cleanup_stale_runtime_lease(
        &authorized.public.room_id,
        &authorized.public.session_id,
    );
}

#[cfg(unix)]
#[tokio::test]
async fn begin_failure_observation_retains_exact_proof_until_db_checkpoint() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create begin failure fixture: {error}"));
    let session = super::tests::fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let adapter = ProviderAdapter::with_factory(Arc::new(SafeFailureFactory));
    let reservation = adapter
        .reserve_start(&session)
        .await
        .unwrap_or_else(|error| panic!("reserve begin failure: {error}"));
    let mut authorized = session.clone();
    authorized
        .runtime_handle_id
        .clone_from(&reservation.runtime_handle_id);
    authorized
        .runtime_owner_id
        .clone_from(&reservation.runtime_owner_id);
    authorized
        .runtime_lease_token
        .clone_from(&reservation.runtime_lease_token);
    let launch_blocker = crate::runtime_lease::lock_test_launch_lifetime(
        &authorized.public.room_id,
        &authorized.public.session_id,
    );

    let Err(failure) = adapter.start_reserved(&authorized).await else {
        panic!("missing launch lifetime must fail before the provider effect");
    };
    assert!(!failure.effect_uncertain);
    assert!(!failure.runtime_stopped);
    drop(launch_blocker);
    assert_eq!(
        adapter.observe(&authorized).await,
        ProviderRuntimeObservation::Gone
    );

    drop(adapter);
    assert_eq!(
        crate::runtime_lease::observe_runtime_lease(
            &authorized.public.room_id,
            &authorized.public.session_id,
        ),
        crate::runtime_lease::LeaseObservation::GenerationGone {
            launch_token: reservation.runtime_lease_token,
        }
    );
    crate::runtime_lease::cleanup_stale_runtime_lease(
        &authorized.public.room_id,
        &authorized.public.session_id,
    );
}

#[tokio::test]
async fn terminal_start_failure_release_permits_one_fresh_generation() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create terminal release fixture: {error}"));
    let session = super::tests::fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let adapter = ProviderAdapter::with_factory(Arc::new(SafeFailureFactory));
    let reservation = adapter
        .reserve_start(&session)
        .await
        .unwrap_or_else(|error| panic!("reserve failed generation: {error}"));
    let mut authorized = session.clone();
    authorized.runtime_handle_id = reservation.runtime_handle_id;
    authorized.runtime_owner_id = reservation.runtime_owner_id;
    authorized.runtime_lease_token = reservation.runtime_lease_token.clone();
    let Err(error) = adapter.start_reserved(&authorized).await else {
        panic!("fixture launch must fail safely");
    };
    assert!(error.runtime_stopped);

    adapter
        .release_checkpointed_start_absence(&authorized)
        .await;
    let next = adapter
        .reserve_start(&session)
        .await
        .unwrap_or_else(|error| panic!("reserve fresh generation: {error}"));
    assert_ne!(next.runtime_lease_token, reservation.runtime_lease_token);
    adapter
        .cancel_start_reservation(&session.public.room_id, &session.public.session_id, &next)
        .await;
}

#[tokio::test]
async fn reservation_cleanup_rejects_a_substituted_lease_generation() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create reservation cleanup fixture: {error}"));
    let session = super::tests::fixture_session(directory.path(), "#!/bin/sh\nexit 0\n").await;
    let adapter = ProviderAdapter::with_factory(Arc::new(SafeFailureFactory));
    let reservation = adapter
        .reserve_start(&session)
        .await
        .unwrap_or_else(|error| panic!("reserve exact generation: {error}"));
    let mut substituted = reservation.clone();
    substituted.runtime_lease_token = uuid::Uuid::new_v4().to_string();

    adapter
        .cancel_start_reservation(
            &session.public.room_id,
            &session.public.session_id,
            &substituted,
        )
        .await;
    assert_eq!(
        adapter
            .reserve_start(&session)
            .await
            .unwrap_or_else(|error| panic!("reload retained generation: {error}")),
        reservation
    );
    adapter
        .cancel_start_reservation(
            &session.public.room_id,
            &session.public.session_id,
            &reservation,
        )
        .await;
}

#[tokio::test]
async fn post_spawn_pre_anchor_cancellation_requires_the_guardian_receipt() {
    let _serial = super::tests::RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create pre-anchor cancellation fixture: {error}"));
    let guardian_spawned = directory.path().join("guardian-spawned");
    let provider_started = directory.path().join("provider-started");
    let script = format!(
        "#!/bin/sh\nprintf started > '{}'\nIFS= read -r forever\n",
        provider_started.to_string_lossy().replace('\'', "'\\''"),
    );
    let mut session = super::tests::fixture_session(directory.path(), &script).await;
    let guardian = GuardianLaunch::test_harness_with_pre_anchor_signal(guardian_spawned.clone())
        .unwrap_or_else(|error| panic!("bind delayed guardian harness: {error}"));
    let adapter = ProviderAdapter::with_factory(Arc::new(ProductionDriverFactory {
        credentials: crate::ProviderCredentialStore::production(),
        guardian: Some(guardian),
    }));
    let pending_adapter = adapter.clone();
    let pending_session = session.clone();
    let task = tokio::spawn(async move { pending_adapter.start(&pending_session).await });
    super::fixture::wait_for_path(&guardian_spawned).await;
    task.abort();
    let _ = task.await;
    let shutdown = adapter.shutdown_with_observations().await;
    assert!(shutdown.failure.is_some());
    assert!(shutdown.gone.is_empty());
    let Err(retry) = adapter.start(&session).await else {
        panic!("pre-anchor cancellation must not permit a replacement");
    };
    assert!(retry.effect_uncertain);
    session.runtime_handle_id = retry.runtime_handle_id;
    session.runtime_owner_id = retry.runtime_owner_id;
    session.runtime_lease_token = retry.runtime_lease_token;
    let mut observation = adapter.observe(&session).await;
    for _ in 0..1_000 {
        if observation == ProviderRuntimeObservation::Gone {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        observation = adapter.observe(&session).await;
    }
    assert_eq!(observation, ProviderRuntimeObservation::Gone);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    assert!(provider_started.exists());
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown after delayed guardian receipt: {error}"));
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
    lease
        .begin_launch_effect()
        .unwrap_or_else(|error| panic!("begin escaped provider launch: {error}"));
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
        &[],
        directory.path(),
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
        LeaseObservation::GenerationGone {
            launch_token: lease.token().to_owned(),
        }
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
