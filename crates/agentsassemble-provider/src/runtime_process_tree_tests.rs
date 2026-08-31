use std::{ffi::OsStr, os::unix::net::UnixDatagram as StdUnixDatagram, path::Path, time::Duration};

use agentsassemble_domain::DurableAgentSession;

use super::{
    ProviderAdapter, ProviderRuntimeObservation, RUNTIME_TEST_LOCK, fixture_session, wait_until,
};
use crate::runtime::test_cleanup::ExactProcessCleanup;
use crate::runtime::{RuntimeKey, RuntimeState};

const TEST_PID_SOCKET_ENV: &str = "AGENTSASSEMBLE_TEST_PID_SOCKET";

struct ProcessPidBarrier(tokio::net::UnixDatagram);

impl ProcessPidBarrier {
    fn bind(path: &Path) -> Self {
        let socket = StdUnixDatagram::bind(path)
            .unwrap_or_else(|error| panic!("bind process PID barrier: {error}"));
        socket
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("make process PID barrier nonblocking: {error}"));
        Self(
            tokio::net::UnixDatagram::from_std(socket)
                .unwrap_or_else(|error| panic!("adopt process PID barrier: {error}")),
        )
    }

    async fn receive(self) -> u32 {
        let mut bytes = [0_u8; 20];
        let size = self
            .0
            .recv(&mut bytes)
            .await
            .unwrap_or_else(|error| panic!("receive process PID barrier: {error}"));
        std::str::from_utf8(&bytes[..size])
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or_else(|| panic!("process PID barrier returned a noncanonical PID"))
    }
}

fn publish_process_pid(socket_path: &OsStr) {
    let socket = StdUnixDatagram::unbound()
        .unwrap_or_else(|error| panic!("open process PID publisher: {error}"));
    socket
        .send_to(
            rustix::process::getpid()
                .as_raw_pid()
                .to_string()
                .as_bytes(),
            socket_path,
        )
        .unwrap_or_else(|error| panic!("publish process PID: {error}"));
}

fn parked_descendant_shell(socket_path: &Path) -> String {
    let test_binary = std::env::current_exe()
        .unwrap_or_else(|error| panic!("resolve parked descendant binary: {error}"));
    format!(
        "{TEST_PID_SOCKET_ENV}={} {} --exact runtime::tests::process_tree_tests::parked_descendant_entry --nocapture </dev/null >/dev/null 2>&1 &",
        shell_quote(socket_path),
        shell_quote(&test_binary),
    )
}

#[test]
fn parked_descendant_entry() {
    let Some(socket_path) = std::env::var_os(TEST_PID_SOCKET_ENV) else {
        return;
    };
    publish_process_pid(&socket_path);
    loop {
        std::thread::park();
    }
}

#[test]
#[allow(clippy::zombie_processes)] // The first fixture child must exit without reaping its child.
fn escaped_descendant_entry() {
    let Some(pid_socket) = std::env::var_os(TEST_PID_SOCKET_ENV) else {
        return;
    };
    if std::env::var_os("AGENTSASSEMBLE_ESCAPE_FIXTURE_CHILD").is_some() {
        publish_process_pid(&pid_socket);
        loop {
            std::thread::park();
        }
    }
    rustix::process::setsid()
        .unwrap_or_else(|error| panic!("create escaped descendant session: {error}"));
    std::process::Command::new("/bin/sh")
        .args([
            "-c",
            "exec 198>&-; exec \"$1\" --exact runtime::tests::process_tree_tests::escaped_descendant_entry --nocapture",
            "escaped-descendant",
            &std::env::current_exe()
                .unwrap_or_else(|error| panic!("resolve escaped descendant binary: {error}"))
                .to_string_lossy(),
        ])
        .env_clear()
        .env(TEST_PID_SOCKET_ENV, pid_socket)
        .env("AGENTSASSEMBLE_ESCAPE_FIXTURE_CHILD", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn reparented descendant: {error}"));
}

#[tokio::test]
async fn stop_kills_descendants_after_the_codex_leader_exits() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create descendant fixture: {error}"));
    let descendant_socket = directory.path().join("descendant.sock");
    let pid_barrier = ProcessPidBarrier::bind(&descendant_socket);
    let descendant = parked_descendant_shell(&descendant_socket);
    let script = format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nIFS= read -r thread\n{descendant}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nsleep 1\nexit 0\n"
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start descendant fixture: {error}"));
    let pid = pid_barrier.receive().await;
    let mut cleanup = ExactProcessCleanup::new(pid);
    tokio::time::timeout(Duration::from_secs(2), async {
        while leader_is_alive(&adapter, &session).await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("provider leader did not exit before timeout"));
    assert!(process_exists(pid));
    let stopped = adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    #[cfg(not(target_os = "macos"))]
    stopped.unwrap_or_else(|error| panic!("stop exited leader and descendants: {error}"));
    #[cfg(target_os = "macos")]
    {
        let Err(error) = stopped else {
            panic!("macOS must fail closed when the provider forked before its leader exited");
        };
        assert_eq!(error.code, "provider_stop_unconfirmed");
        assert_eq!(
            error.message,
            "The provider leader exited before descendant custody could be proven."
        );
    }
    #[cfg(not(target_os = "macos"))]
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    #[cfg(target_os = "macos")]
    {
        cleanup.kill_now();
    }
    wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
    cleanup.disarm();
    #[cfg(target_os = "macos")]
    crate::runtime_lease::cleanup_stale_runtime_lease(
        &session.public.room_id,
        &session.public.session_id,
    );
}

#[tokio::test]
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
async fn stop_captures_a_reparented_descendant_from_a_new_session() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create escape fixture: {error}"));
    let descendant_socket = directory.path().join("escaped-descendant.sock");
    let pid_barrier = ProcessPidBarrier::bind(&descendant_socket);
    let test_binary = std::env::current_exe()
        .unwrap_or_else(|error| panic!("resolve provider test binary: {error}"));
    let script = format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nIFS= read -r thread\n{TEST_PID_SOCKET_ENV}={} {} --exact runtime::tests::process_tree_tests::escaped_descendant_entry --nocapture </dev/null >/dev/null 2>&1 &\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r forever\n",
        shell_quote(&descendant_socket),
        shell_quote(&test_binary),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start escaped descendant fixture: {error}"));
    let pid = pid_barrier.receive().await;
    let mut cleanup = ExactProcessCleanup::new(pid);
    assert!(process_exists(pid));
    let stopped = adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    #[cfg(target_os = "linux")]
    stopped.unwrap_or_else(|error| panic!("stop escaped descendant fixture: {error}"));
    #[cfg(any(target_os = "android", target_os = "macos"))]
    {
        let Err(error) = stopped else {
            panic!("a platform without stable process handles must fail closed");
        };
        assert_eq!(error.code, "provider_stop_unconfirmed");
        assert_eq!(
            error.message,
            "The provider lineage history could not be confirmed."
        );
        assert!(process_exists(pid));
        let mut durable_session = session.clone();
        durable_session.runtime_handle_id = started.runtime_handle_id.clone();
        durable_session.runtime_owner_id = started.runtime_owner_id.clone();
        durable_session.runtime_lease_token = started.runtime_lease_token.clone();
        let fresh = ProviderAdapter::new();
        assert!(matches!(
            fresh.observe(&durable_session).await,
            ProviderRuntimeObservation::Ambiguous { .. }
        ));
        cleanup.kill_now();
        wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
        assert!(matches!(
            fresh.observe(&durable_session).await,
            ProviderRuntimeObservation::Ambiguous { .. }
        ));
        crate::runtime_lease::cleanup_stale_runtime_lease(
            &session.public.room_id,
            &session.public.session_id,
        );
    }
    #[cfg(target_os = "linux")]
    {
        adapter
            .release_confirmed_stop(
                &session.public.room_id,
                &session.public.session_id,
                &started.runtime_handle_id,
                &started.runtime_owner_id,
                &started.runtime_lease_token,
            )
            .await;
        wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
        cleanup.disarm();
    }
}

#[tokio::test]
async fn cancelled_initialization_remains_owned_for_shutdown() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create cancellation fixture: {error}"));
    let descendant_socket = directory.path().join("cancelled-descendant.sock");
    let pid_barrier = ProcessPidBarrier::bind(&descendant_socket);
    let descendant = parked_descendant_shell(&descendant_socket);
    let script =
        format!("#!/bin/sh\nIFS= read -r initialize\n{descendant}\nwhile :; do sleep 1; done\n");
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let pending_adapter = adapter.clone();
    let pending_session = session.clone();
    let mut pending = tokio::spawn(async move { pending_adapter.start(&pending_session).await });
    let pid = tokio::select! {
        pid = pid_barrier.receive() => pid,
        _ = &mut pending => panic!("provider initialization ended before descendant readiness"),
    };
    let mut cleanup = ExactProcessCleanup::new(pid);
    pending.abort();
    let _ = pending.await;
    let shutdown = adapter.shutdown().await;
    #[cfg(not(target_os = "macos"))]
    shutdown.unwrap_or_else(|error| panic!("shutdown cancelled initialization: {error}"));
    #[cfg(target_os = "macos")]
    {
        let Err(error) = shutdown else {
            panic!("macOS must fail closed after a cancelled provider fork");
        };
        assert_eq!(error.code, "provider_stop_unconfirmed");
        cleanup.kill_now();
    }
    wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
    cleanup.disarm();
    #[cfg(target_os = "macos")]
    crate::runtime_lease::cleanup_stale_runtime_lease(
        &session.public.room_id,
        &session.public.session_id,
    );
}

async fn leader_is_alive(adapter: &ProviderAdapter, session: &DurableAgentSession) -> bool {
    let Some(slot) = adapter.owner.runtimes.try_lock().ok().and_then(|slots| {
        slots
            .get(&RuntimeKey {
                room_id: session.public.room_id.clone(),
                session_id: session.public.session_id.clone(),
            })
            .cloned()
    }) else {
        return true;
    };
    let Ok(mut slot) = slot.try_lock() else {
        return true;
    };
    let RuntimeState::Running(runtime) = &mut slot.state else {
        return false;
    };
    let Ok(mut driver) = runtime.driver.try_take() else {
        return true;
    };
    let alive = driver.is_alive().await.unwrap_or(true);
    runtime.driver.put(driver).await;
    alive
}

fn process_exists(raw_pid: u32) -> bool {
    i32::try_from(raw_pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|pid| match rustix::process::test_kill_process(pid) {
            Ok(()) | Err(rustix::io::Errno::PERM) => true,
            Err(rustix::io::Errno::SRCH) => false,
            Err(error) => panic!("inspect exact process {raw_pid}: {error}"),
        })
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
