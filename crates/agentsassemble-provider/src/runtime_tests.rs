use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};

use super::{ProviderAdapter, ProviderRuntimeObservation};
use crate::filesystem::{canonical_workspace, executable_identity};
use crate::profile::runtime_profile_key;

static RUNTIME_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[test]
fn provider_guardian_entry() {
    if let Some(code) = crate::guardian::run_test_helper_if_requested() {
        std::process::exit(code);
    }
}

#[test]
#[allow(clippy::zombie_processes)] // The first fixture child must exit without reaping its child.
fn escaped_descendant_entry() {
    let Some(pid_path) = std::env::var_os("AGENTSASSEMBLE_ESCAPE_FIXTURE_PID") else {
        return;
    };
    if std::env::var_os("AGENTSASSEMBLE_ESCAPE_FIXTURE_CHILD").is_some() {
        std::fs::write(pid_path, rustix::process::getpid().as_raw_pid().to_string())
            .unwrap_or_else(|error| panic!("publish escaped descendant pid: {error}"));
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    rustix::process::setsid()
        .unwrap_or_else(|error| panic!("create escaped descendant session: {error}"));
    std::process::Command::new(
        std::env::current_exe()
            .unwrap_or_else(|error| panic!("resolve escaped descendant binary: {error}")),
    )
    .args([
        "--exact",
        "runtime::tests::escaped_descendant_entry",
        "--nocapture",
    ])
    .env("AGENTSASSEMBLE_ESCAPE_FIXTURE_CHILD", "1")
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn()
    .unwrap_or_else(|error| panic!("spawn reparented descendant: {error}"));
}

#[tokio::test]
async fn codex_runtime_is_initialized_reused_and_stopped_by_exact_owner() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create runtime fixture: {error}"));
    let executable = directory.path().join("codex-fixture");
    std::fs::write(
        &executable,
        b"#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nwhile :; do sleep 1; done\n",
    )
    .unwrap_or_else(|error| panic!("write runtime fixture: {error}"));
    let mut permissions = std::fs::metadata(&executable)
        .unwrap_or_else(|error| panic!("read runtime fixture mode: {error}"))
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)
        .unwrap_or_else(|error| panic!("make runtime fixture executable: {error}"));
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonicalize runtime fixture: {error}"))
        .to_string_lossy()
        .into_owned();
    let executable_identity = executable_identity(executable.clone())
        .await
        .unwrap_or_else(|error| panic!("identify runtime fixture: {error:?}"));
    let workspace = directory.path().to_string_lossy().into_owned();
    let (workspace, workspace_identity) = canonical_workspace(workspace)
        .await
        .unwrap_or_else(|error| panic!("identify runtime workspace: {error:?}"));
    let session = session(
        executable,
        executable_identity,
        &workspace,
        &workspace_identity,
    );
    let adapter = ProviderAdapter::new();
    let first = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start Codex fixture: {error}"));
    assert!(!first.runtime_reused);
    assert!(!first.runtime_handle_id.is_empty());
    assert!(!first.runtime_owner_id.is_empty());
    assert!(!first.provider_session_active);
    let second = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("reuse Codex fixture: {error}"));
    assert!(second.runtime_reused);
    assert_eq!(second.runtime_handle_id, first.runtime_handle_id);
    assert_eq!(second.runtime_owner_id, first.runtime_owner_id);
    let mut durable_session = session.clone();
    durable_session
        .runtime_handle_id
        .clone_from(&first.runtime_handle_id);
    durable_session
        .runtime_owner_id
        .clone_from(&first.runtime_owner_id);
    assert_adopted(&adapter, &durable_session, &first).await;
    std::fs::write(
        &session.executable,
        b"provider bytes changed while runtime is alive",
    )
    .unwrap_or_else(|error| panic!("change selected executable bytes: {error}"));
    let Err(changed) = adapter.start(&session).await else {
        panic!("changed authority must not be reported as safe while its runtime is alive");
    };
    assert!(changed.effect_uncertain);
    assert_eq!(changed.runtime_handle_id, first.runtime_handle_id);
    assert_eq!(changed.runtime_owner_id, first.runtime_owner_id);
    assert_executable_lease_uncertain(&adapter, &durable_session, &first).await;
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &first.runtime_handle_id,
            &first.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("stop Codex fixture: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &first.runtime_handle_id,
            &first.runtime_owner_id,
        )
        .await;
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown runtime owner: {error}"));
}

async fn assert_adopted(
    adapter: &ProviderAdapter,
    session: &DurableAgentSession,
    started: &super::ProviderRuntimeStarted,
) {
    assert_eq!(
        adapter.observe(session).await,
        ProviderRuntimeObservation::Adopted {
            handle_id: started.runtime_handle_id.clone(),
            previous_owner_id: started.runtime_owner_id.clone(),
            new_owner_id: started.runtime_owner_id.clone(),
            runtime_profile_key: session.runtime_profile_key.clone(),
        }
    );
}

async fn assert_executable_lease_uncertain(
    adapter: &ProviderAdapter,
    session: &DurableAgentSession,
    started: &super::ProviderRuntimeStarted,
) {
    assert_eq!(
        adapter.observe(session).await,
        ProviderRuntimeObservation::LeaseUncertain {
            handle_id: started.runtime_handle_id.clone(),
            owner_id: started.runtime_owner_id.clone(),
            reason_code: "executable_authority_changed".to_owned(),
        }
    );
}

#[tokio::test]
async fn stop_kills_descendants_after_the_codex_leader_exits() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create descendant fixture: {error}"));
    let descendant_pid = directory.path().join("descendant.pid");
    let script = format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\n(while :; do sleep 1; done) </dev/null >/dev/null 2>&1 &\nprintf '%s' \"$!\" > {}\nexit 0\n",
        shell_quote(&descendant_pid)
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start descendant fixture: {error}"));
    let pid = wait_for_pid(&descendant_pid).await;
    let _cleanup = ExactProcessCleanup(pid);
    wait_until(Duration::from_secs(2), || {
        !leader_is_alive(&adapter, &session)
    })
    .await;
    assert!(process_exists(pid));
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("stop exited leader and descendants: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
        )
        .await;
    wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
}

#[tokio::test]
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
async fn stop_captures_a_reparented_descendant_from_a_new_session() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create escape fixture: {error}"));
    let descendant_pid = directory.path().join("escaped-descendant.pid");
    let test_binary = std::env::current_exe()
        .unwrap_or_else(|error| panic!("resolve provider test binary: {error}"));
    let script = format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nAGENTSASSEMBLE_ESCAPE_FIXTURE_PID={} {} --exact runtime::tests::escaped_descendant_entry --nocapture </dev/null >/dev/null 2>&1 &\nwhile :; do sleep 1; done\n",
        shell_quote(&descendant_pid),
        shell_quote(&test_binary),
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start escaped descendant fixture: {error}"));
    let pid = wait_for_pid(&descendant_pid).await;
    let cleanup = ExactProcessCleanup(pid);
    assert!(process_exists(pid));
    let stopped = adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
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
        assert!(process_exists(pid));
        let mut durable_session = session.clone();
        durable_session.runtime_handle_id = started.runtime_handle_id.clone();
        durable_session.runtime_owner_id = started.runtime_owner_id.clone();
        let fresh = ProviderAdapter::new();
        assert!(matches!(
            fresh.observe(&durable_session).await,
            ProviderRuntimeObservation::LeaseUncertain { .. }
        ));
        drop(cleanup);
        wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
        assert_eq!(
            fresh.observe(&durable_session).await,
            ProviderRuntimeObservation::Gone
        );
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
            )
            .await;
        wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
        drop(cleanup);
    }
}

#[tokio::test]
async fn fresh_supervisor_uses_the_guardian_lease_before_reporting_gone() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create guardian fixture: {error}"));
    let script = "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nwhile :; do sleep 1; done\n";
    let mut session = fixture_session(directory.path(), script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start guarded fixture: {error}"));
    session.runtime_handle_id = started.runtime_handle_id;
    session.runtime_owner_id = started.runtime_owner_id;
    let fresh = ProviderAdapter::new();
    assert!(matches!(
        fresh.observe(&session).await,
        ProviderRuntimeObservation::LeaseUncertain { .. }
    ));
    drop(adapter);
    let mut observation = fresh.observe(&session).await;
    for _ in 0..200 {
        if observation == ProviderRuntimeObservation::Gone {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        observation = fresh.observe(&session).await;
    }
    assert_eq!(observation, ProviderRuntimeObservation::Gone);
    crate::runtime_lease::cleanup_stale_runtime_lease(
        &session.public.room_id,
        &session.public.session_id,
    );
}

#[tokio::test]
async fn cancelled_initialization_remains_owned_for_shutdown() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create cancellation fixture: {error}"));
    let descendant_pid = directory.path().join("cancelled-descendant.pid");
    let script = format!(
        "#!/bin/sh\n(while :; do sleep 1; done) </dev/null >/dev/null 2>&1 &\nprintf '%s' \"$!\" > {}\nIFS= read -r initialize\nwhile :; do sleep 1; done\n",
        shell_quote(&descendant_pid)
    );
    let session = fixture_session(directory.path(), &script).await;
    let adapter = ProviderAdapter::new();
    let pending_adapter = adapter.clone();
    let pending_session = session.clone();
    let pending = tokio::spawn(async move { pending_adapter.start(&pending_session).await });
    let pid = wait_for_pid(&descendant_pid).await;
    let _cleanup = ExactProcessCleanup(pid);
    pending.abort();
    let _ = pending.await;
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown cancelled initialization: {error}"));
    wait_until(Duration::from_secs(2), || !process_exists(pid)).await;
}

async fn fixture_session(directory: &Path, script: &str) -> DurableAgentSession {
    let executable = directory.join("codex-fixture");
    std::fs::write(&executable, script)
        .unwrap_or_else(|error| panic!("write process fixture: {error}"));
    let mut permissions = std::fs::metadata(&executable)
        .unwrap_or_else(|error| panic!("read process fixture mode: {error}"))
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)
        .unwrap_or_else(|error| panic!("make process fixture executable: {error}"));
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonicalize process fixture: {error}"))
        .to_string_lossy()
        .into_owned();
    let executable_identity = executable_identity(executable.clone())
        .await
        .unwrap_or_else(|error| panic!("identify process fixture: {error:?}"));
    let (workspace, workspace_identity) =
        canonical_workspace(directory.to_string_lossy().into_owned())
            .await
            .unwrap_or_else(|error| panic!("identify process workspace: {error:?}"));
    session(
        executable,
        executable_identity,
        &workspace,
        &workspace_identity,
    )
}

async fn wait_for_pid(path: &Path) -> u32 {
    let mut pid = None;
    wait_until(Duration::from_secs(2), || {
        pid = std::fs::read_to_string(path)
            .ok()
            .and_then(|value| value.parse::<u32>().ok());
        pid.is_some()
    })
    .await;
    pid.unwrap_or_else(|| panic!("fixture did not publish descendant pid"))
}

async fn wait_until(mut remaining: Duration, mut condition: impl FnMut() -> bool) {
    while !condition() && !remaining.is_zero() {
        let interval = Duration::from_millis(10).min(remaining);
        tokio::time::sleep(interval).await;
        remaining = remaining.saturating_sub(interval);
    }
    assert!(condition(), "condition did not become true before timeout");
}

fn leader_is_alive(adapter: &ProviderAdapter, session: &DurableAgentSession) -> bool {
    let Some(slot) = adapter.owner.runtimes.try_lock().ok().and_then(|slots| {
        slots
            .get(&super::RuntimeKey {
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
    let super::RuntimeState::Running(runtime) = &mut slot.state else {
        return false;
    };
    runtime.driver.is_alive().unwrap_or(true)
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

struct ExactProcessCleanup(u32);

impl Drop for ExactProcessCleanup {
    fn drop(&mut self) {
        if let Some(pid) = i32::try_from(self.0)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
        }
    }
}

fn session(
    executable: String,
    executable_identity: String,
    workspace: &str,
    workspace_identity: &str,
) -> DurableAgentSession {
    let mut session = serde_json::from_value::<DurableAgentSession>(serde_json::json!({
        "room_id": "general",
        "session_id": "codex-agent",
        "participant_id": "codex-agent",
        "display_name": "Codex",
        "status": "available",
        "runtime_status": "starting",
        "enabled": true,
        "provider_kind": "codex_live_session",
        "runtime_kind": "live_cli",
        "connection_kind": "native_cli_bridge",
        "external_owned": false,
        "process_ownership": "server",
        "model": "gpt-5.6-terra",
        "reasoning_effort": "high",
        "service_tier": "default",
        "variant": "",
        "execution_harness": "builtin",
        "permission_mode": "meeting_read_only",
        "max_output_tokens": 0,
        "catalog_revision": "revision",
        "transport": "stdio_jsonl",
        "last_seen_event_id": "",
        "last_seen_seq": 0,
        "last_provider_sync_event_id": "",
        "last_provider_sync_seq": 0,
        "bootstrap_cutoff_seq": 0,
        "turn_count": 0,
        "created_at": "2026-08-23T00:00:00Z",
        "updated_at": "2026-08-23T00:00:00Z",
        "workspace": workspace,
        "workspace_identity": workspace_identity,
        "runtime_profile_key": "codex-profile",
        "runtime_profile_version": CURRENT_RUNTIME_PROFILE_VERSION
    }))
    .unwrap_or_else(|error| panic!("decode runtime session: {error}"));
    session.executable = executable;
    session.executable_identity = executable_identity;
    session.runtime_profile_key = runtime_profile_key([
        session.public.provider_kind.as_str(),
        session.public.runtime_kind.as_str(),
        session.executable.as_str(),
        session.executable_identity.as_str(),
        session.workspace.as_str(),
        session.workspace_identity.as_str(),
        session.public.model.as_str(),
        session.public.reasoning_effort.as_str(),
        session.public.service_tier.as_str(),
        session.public.variant.as_str(),
        session.public.execution_harness.as_str(),
        session.public.permission_mode.as_str(),
        session.public.transport.as_str(),
    ]);
    session
}
