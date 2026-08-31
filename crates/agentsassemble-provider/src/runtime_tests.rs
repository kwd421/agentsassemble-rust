use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use agentsassemble_domain::DurableAgentSession;

use super::{ProviderAdapter, ProviderRuntimeObservation};
use crate::filesystem::{canonical_workspace, codex_executable_identity, executable_identity};
use crate::profile::runtime_profile_key;
use crate::runtime::test_cleanup::{ExactProcessCleanup, ExactProcessGroupCleanup};
use crate::test_support::durable_session;

pub(super) static RUNTIME_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[path = "runtime_process_tree_tests.rs"]
mod process_tree_tests;

#[test]
fn provider_guardian_entry() {
    if let Some(code) = crate::guardian::run_test_helper_if_requested() {
        std::process::exit(code);
    }
}

#[test]
fn inert_provider_entry() {
    if std::env::var_os(crate::unix_process_tree::RUNTIME_TOKEN_ENV).is_some() {
        if let Some(report) = std::env::var_os("AGENTSASSEMBLE_TEST_CWD_REPORT") {
            std::fs::write(
                report,
                std::env::current_dir()
                    .unwrap_or_else(|error| panic!("resolve provider cwd: {error}"))
                    .as_os_str()
                    .as_encoded_bytes(),
            )
            .unwrap_or_else(|error| panic!("report provider cwd: {error}"));
        }
        loop {
            std::thread::park();
        }
    }
}

#[tokio::test]
async fn guardian_runs_outside_the_server_process_group() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let workspace =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create guardian workspace: {error}"));
    let workspace_path = workspace
        .path()
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonicalize guardian workspace: {error}"));
    let cwd_report = workspace_path.join("provider-cwd");
    let suffix = uuid::Uuid::new_v4().to_string();
    let lease = crate::runtime_lease::HeldRuntimeLease::prepare(
        &format!("guardian-group-room-{suffix}"),
        &format!("guardian-group-session-{suffix}"),
    )
    .unwrap_or_else(|error| panic!("prepare guardian group lease: {error}"));
    let launch = crate::guardian::GuardianLaunch::test_harness()
        .unwrap_or_else(|error| panic!("bind guardian test harness: {error}"));
    lease
        .begin_launch_effect()
        .unwrap_or_else(|error| panic!("begin guardian group launch: {error}"));
    let provider = crate::filesystem::bind_helper_executable_sync(
        &std::env::current_exe()
            .unwrap_or_else(|error| panic!("resolve guardian fixture provider: {error}")),
    )
    .unwrap_or_else(|error| panic!("bind guardian fixture provider: {error}"));
    let (provider_stdin, _stdin) =
        std::io::pipe().unwrap_or_else(|error| panic!("create guardian fixture stdin: {error}"));
    let (_stdout, provider_stdout) =
        std::io::pipe().unwrap_or_else(|error| panic!("create guardian fixture stdout: {error}"));
    let (_stderr, provider_stderr) =
        std::io::pipe().unwrap_or_else(|error| panic!("create guardian fixture stderr: {error}"));
    let mut command = launch
        .guardian_command(
            lease.path(),
            lease.token(),
            crate::guardian::ProviderLaunchConfig {
                provider: &provider,
                arguments: &inert_provider_arguments(),
                environment: &[(
                    "AGENTSASSEMBLE_TEST_CWD_REPORT".to_owned(),
                    cwd_report.to_string_lossy().into_owned(),
                )],
                working_directory: &workspace_path,
                pipes: [
                    provider_stdin.into(),
                    provider_stdout.into(),
                    provider_stderr.into(),
                ],
                fork_policy: crate::guardian::ProviderForkPolicy::Deny,
                codex_code_mode_host: None,
            },
        )
        .unwrap_or_else(|error| panic!("configure guardian test command: {error}"));
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut guardian_child = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn guardian test command: {error}"));
    drop(command);
    let guardian_pid = guardian_child
        .id()
        .unwrap_or_else(|| panic!("guardian pid is unavailable"));
    let mut cleanup = ExactProcessCleanup::new(guardian_pid);
    let mut input = guardian_child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("guardian input is unavailable"));
    let output = guardian_child
        .stdout
        .take()
        .unwrap_or_else(|| panic!("guardian output is unavailable"));
    let (anchor_pid, _) = read_guardian_ready(output, "guardian group", &lease, &mut input).await;
    assert!(anchor_pid > 0);
    wait_until(super::fixture::FIXTURE_READINESS_TIMEOUT, || {
        std::fs::read(&cwd_report)
            .is_ok_and(|contents| contents == workspace_path.as_os_str().as_encoded_bytes())
    })
    .await;
    assert_provider_working_directory(&cwd_report, &workspace_path);
    let guardian_process = i32::try_from(guardian_pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .unwrap_or_else(|| panic!("guardian pid is invalid"));
    assert_ne!(
        rustix::process::getpgid(Some(guardian_process))
            .unwrap_or_else(|error| panic!("inspect guardian group: {error}")),
        rustix::process::getpgrp()
    );
    drop(input);
    let status = tokio::time::timeout(Duration::from_secs(6), guardian_child.wait())
        .await
        .unwrap_or_else(|_| panic!("guardian cleanup timed out"))
        .unwrap_or_else(|error| panic!("wait for guardian cleanup: {error}"));
    assert!(status.success());
    cleanup.disarm();
    lease.cleanup_pre_effect();
}

fn assert_provider_working_directory(report: &Path, expected: &Path) {
    assert_eq!(
        std::fs::read(report).unwrap_or_else(|error| panic!("read provider cwd report: {error}")),
        expected.as_os_str().as_encoded_bytes()
    );
}

fn inert_provider_arguments() -> [String; 3] {
    [
        "--exact".to_owned(),
        "runtime::tests::inert_provider_entry".to_owned(),
        "--nocapture".to_owned(),
    ]
}

#[tokio::test]
async fn guardian_death_without_a_cleanup_receipt_never_proves_gone() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let suffix = uuid::Uuid::new_v4().to_string();
    let room_id = format!("guardian-death-room-{suffix}");
    let session_id = format!("guardian-death-session-{suffix}");
    let lease = crate::runtime_lease::HeldRuntimeLease::prepare(&room_id, &session_id)
        .unwrap_or_else(|error| panic!("prepare guardian death lease: {error}"));
    let launch = crate::guardian::GuardianLaunch::test_harness()
        .unwrap_or_else(|error| panic!("bind guardian death harness: {error}"));
    lease
        .begin_launch_effect()
        .unwrap_or_else(|error| panic!("begin guardian death launch: {error}"));
    let provider = crate::filesystem::bind_helper_executable_sync(
        &std::env::current_exe()
            .unwrap_or_else(|error| panic!("resolve guardian death provider: {error}")),
    )
    .unwrap_or_else(|error| panic!("bind guardian death provider: {error}"));
    let (provider_stdin, _provider_input) =
        std::io::pipe().unwrap_or_else(|error| panic!("create guardian death stdin: {error}"));
    let (_provider_output, provider_stdout) =
        std::io::pipe().unwrap_or_else(|error| panic!("create guardian death stdout: {error}"));
    let (_provider_error, provider_stderr) =
        std::io::pipe().unwrap_or_else(|error| panic!("create guardian death stderr: {error}"));
    let mut command = launch
        .guardian_command(
            lease.path(),
            lease.token(),
            crate::guardian::ProviderLaunchConfig {
                provider: &provider,
                arguments: &inert_provider_arguments(),
                environment: &[],
                working_directory: Path::new(env!("CARGO_MANIFEST_DIR")),
                pipes: [
                    provider_stdin.into(),
                    provider_stdout.into(),
                    provider_stderr.into(),
                ],
                fork_policy: crate::guardian::ProviderForkPolicy::Deny,
                codex_code_mode_host: None,
            },
        )
        .unwrap_or_else(|error| panic!("configure guardian death command: {error}"));
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut guardian = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn guardian death command: {error}"));
    drop(command);
    let guardian_pid = guardian
        .id()
        .unwrap_or_else(|| panic!("guardian death pid is unavailable"));
    let mut guardian_cleanup = ExactProcessCleanup::new(guardian_pid);
    let mut input = guardian
        .stdin
        .take()
        .unwrap_or_else(|| panic!("guardian death input is unavailable"));
    let output = guardian
        .stdout
        .take()
        .unwrap_or_else(|| panic!("guardian death output is unavailable"));
    let (anchor_pid, _) = read_guardian_ready(output, "guardian death", &lease, &mut input).await;
    let mut group_cleanup = ExactProcessGroupCleanup::new(anchor_pid);
    guardian_cleanup.kill_now();
    tokio::time::timeout(Duration::from_secs(2), guardian.wait())
        .await
        .unwrap_or_else(|_| panic!("guardian death wait timed out"))
        .unwrap_or_else(|error| panic!("wait for killed guardian: {error}"));
    guardian_cleanup.disarm();
    group_cleanup.kill_now();
    wait_until(Duration::from_secs(2), || !process_group_exists(anchor_pid)).await;
    group_cleanup.disarm();
    assert_eq!(
        crate::runtime_lease::observe_runtime_lease(&room_id, &session_id),
        crate::runtime_lease::LeaseObservation::Unknown
    );
    lease.cleanup_pre_effect();
}

#[tokio::test]
async fn codex_runtime_is_initialized_reused_and_stopped_by_exact_owner() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create runtime fixture: {error}"));
    let executable = directory.path().join("codex-fixture");
    std::fs::write(
        &executable,
        b"#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\nIFS= read -r forever\n",
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
    assert!(first.provider_session_active);
    assert_eq!(first.provider_session_id, "thread-1");
    assert!(!first.provider_session_reused);
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
    durable_session
        .runtime_lease_token
        .clone_from(&first.runtime_lease_token);
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
            &first.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("stop Codex fixture: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &first.runtime_handle_id,
            &first.runtime_owner_id,
            &first.runtime_lease_token,
        )
        .await;
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown runtime owner: {error}"));
}

#[tokio::test]
async fn codex_code_mode_host_stays_in_the_guardian_process_group() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create code-mode host fixture: {error}"));
    let (session, arguments_report, host_pid_report) =
        code_mode_host_fixture(directory.path()).await;
    let adapter = ProviderAdapter::new();
    let started = adapter.start(&session).await.unwrap_or_else(|error| {
        panic!(
            "start code-mode host fixture: {error}; arguments={:?}; host_pid={:?}",
            std::fs::read_to_string(&arguments_report),
            std::fs::read_to_string(&host_pid_report)
        )
    });
    wait_until(super::fixture::FIXTURE_READINESS_TIMEOUT, || {
        arguments_report.exists() && host_pid_report.exists()
    })
    .await;
    let arguments = std::fs::read_to_string(&arguments_report)
        .unwrap_or_else(|error| panic!("read provider arguments: {error}"))
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let host_argument = arguments
        .windows(2)
        .find_map(|pair| (pair[0] == "--code-mode-host").then_some(pair[1].as_str()))
        .unwrap_or_else(|| panic!("provider did not receive the code-mode host endpoint"));
    assert_eq!(host_argument, "ws://127.0.0.1:43123");
    let host_pid = std::fs::read_to_string(&host_pid_report)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .and_then(rustix::process::Pid::from_raw)
        .unwrap_or_else(|| panic!("code-mode host pid is invalid"));
    let mut host_cleanup = ExactProcessCleanup::new(host_pid.as_raw_pid().cast_unsigned());
    assert_ne!(
        rustix::process::getpgid(Some(host_pid))
            .unwrap_or_else(|error| panic!("inspect code-mode host group: {error}")),
        host_pid
    );
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("stop code-mode host fixture: {error}"));
    wait_until(Duration::from_secs(2), || {
        matches!(
            rustix::process::test_kill_process(host_pid),
            Err(rustix::io::Errno::SRCH)
        )
    })
    .await;
    host_cleanup.disarm();
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown code-mode host fixture owner: {error}"));
}

#[tokio::test]
async fn codex_staged_bundle_outlives_stopped_launcher_readiness() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create staged bundle fixture: {error}"));
    let (session, _arguments_report, _host_pid_report) =
        code_mode_host_fixture(directory.path()).await;
    let barrier_path = directory.path().join("launcher-barrier.sock");
    let listener = tokio::net::UnixListener::bind(&barrier_path)
        .unwrap_or_else(|error| panic!("bind launcher barrier: {error}"));
    let guardian =
        crate::guardian::GuardianLaunch::test_harness_with_launcher_barrier(barrier_path.clone())
            .unwrap_or_else(|error| panic!("bind guarded launcher fixture: {error}"));
    let adapter = ProviderAdapter::with_guardian_launch(guardian);
    let start_adapter = adapter.clone();
    let start_session = session.clone();
    let start = tokio::spawn(async move { start_adapter.start(&start_session).await });

    let (stream, _) = listener
        .accept()
        .await
        .unwrap_or_else(|error| panic!("accept launcher barrier: {error}"));
    let mut stream = BufReader::new(stream);
    let mut report = String::new();
    stream
        .read_line(&mut report)
        .await
        .unwrap_or_else(|error| panic!("read launcher report: {error}"));
    let (provider, companion): (String, Option<String>) = serde_json::from_str(&report)
        .unwrap_or_else(|error| panic!("decode launcher report: {error}"));
    assert!(Path::new(&provider).is_file());
    assert!(
        companion
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file())
    );
    stream
        .get_mut()
        .write_all(b"continue\n")
        .await
        .unwrap_or_else(|error| panic!("release launcher barrier: {error}"));

    let started = start
        .await
        .unwrap_or_else(|error| panic!("join staged bundle start: {error}"))
        .unwrap_or_else(|error| panic!("start staged bundle fixture: {error}"));
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("stop staged bundle fixture: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await;
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown staged bundle fixture owner: {error}"));
}

pub(super) async fn code_mode_host_fixture(root: &Path) -> (DurableAgentSession, PathBuf, PathBuf) {
    let executable = root.join("codex");
    let companion = root.join("codex-code-mode-host");
    let arguments_report = root.join("provider-arguments");
    let host_pid_report = root.join("code-mode-host-pid");
    let provider_script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nIFS= read -r initialize\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r forever\n",
        arguments_report.display()
    );
    let host_script = format!(
        "#!/bin/sh\nprintf '%s\\n' 'SIDE_CAR_PROTOCOL_LEAK' >&5\nprintf '%s' \"$$\" > '{}'\nprintf '%s\\n' 'ws://127.0.0.1:43123'\nexec /bin/sleep 30\n",
        host_pid_report.display()
    );
    for (path, contents) in [
        (&executable, provider_script.as_bytes()),
        (&companion, host_script.as_bytes()),
    ] {
        std::fs::write(path, contents)
            .unwrap_or_else(|error| panic!("write code-mode host fixture: {error}"));
        let mut permissions = std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("read code-mode host fixture mode: {error}"))
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions)
            .unwrap_or_else(|error| panic!("make code-mode host fixture executable: {error}"));
    }
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonicalize code-mode host fixture: {error}"))
        .to_string_lossy()
        .into_owned();
    let executable_identity = codex_executable_identity(executable.clone())
        .await
        .unwrap_or_else(|error| panic!("identify code-mode host fixture: {error:?}"));
    let (workspace, workspace_identity) = canonical_workspace(root.to_string_lossy().into_owned())
        .await
        .unwrap_or_else(|error| panic!("identify code-mode host workspace: {error:?}"));
    (
        session(
            executable,
            executable_identity,
            &workspace,
            &workspace_identity,
        ),
        arguments_report,
        host_pid_report,
    )
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
async fn fresh_supervisor_uses_the_guardian_lease_before_reporting_gone() {
    let _serial = RUNTIME_TEST_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create guardian fixture: {error}"));
    let script = "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\nIFS= read -r forever\n";
    let mut session = fixture_session(directory.path(), script).await;
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start guarded fixture: {error}"));
    session.runtime_handle_id = started.runtime_handle_id;
    session.runtime_owner_id = started.runtime_owner_id;
    session.runtime_lease_token = started.runtime_lease_token;
    let fresh = ProviderAdapter::new();
    assert!(matches!(
        fresh.observe(&session).await,
        ProviderRuntimeObservation::LeaseUncertain { .. }
    ));
    drop(adapter);
    let mut observation = fresh.observe(&session).await;
    for _ in 0..800 {
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

pub(super) async fn fixture_session(directory: &Path, script: &str) -> DurableAgentSession {
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
async fn wait_until(mut remaining: Duration, mut condition: impl FnMut() -> bool) {
    while !condition() && !remaining.is_zero() {
        let interval = Duration::from_millis(10).min(remaining);
        tokio::time::sleep(interval).await;
        remaining = remaining.saturating_sub(interval);
    }
    assert!(condition(), "condition did not become true before timeout");
}
async fn read_guardian_ready(
    output: tokio::process::ChildStdout,
    context: &str,
    lease: &crate::runtime_lease::HeldRuntimeLease,
    input: &mut tokio::process::ChildStdin,
) -> (u32, u32) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let ready = tokio::time::timeout(super::fixture::FIXTURE_READINESS_TIMEOUT, async {
        let mut output = BufReader::new(output);
        let mut lifetime_handed_off = false;
        for _ in 0..32 {
            let mut line = String::new();
            if output.read_line(&mut line).await? == 0 {
                break;
            }
            if line.trim() == crate::guardian_lifetime::READY {
                lease.release_launch_lifetime();
                input.write_all(crate::guardian_lifetime::CONTINUE).await?;
                input.flush().await?;
                lifetime_handed_off = true;
                continue;
            }
            if let Some(identities) = line
                .trim()
                .strip_prefix("AGENTSASSEMBLE_PROVIDER_READY=")
                .and_then(|value| value.split_once(':'))
                .and_then(|(anchor, provider)| {
                    Some((anchor.parse::<u32>().ok()?, provider.parse::<u32>().ok()?))
                })
            {
                if !lifetime_handed_off {
                    return Ok::<_, std::io::Error>(None);
                }
                return Ok::<_, std::io::Error>(Some(identities));
            }
        }
        Ok(None)
    })
    .await
    .unwrap_or_else(|_| panic!("{context} readiness timed out"))
    .unwrap_or_else(|error| panic!("read {context} readiness: {error}"));
    ready.unwrap_or_else(|| panic!("{context} readiness was not published"))
}

fn process_group_exists(raw_pid: u32) -> bool {
    i32::try_from(raw_pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|pid| match rustix::process::test_kill_process_group(pid) {
            Ok(()) | Err(rustix::io::Errno::PERM) => true,
            Err(rustix::io::Errno::SRCH) => false,
            Err(error) => panic!("inspect exact process group {raw_pid}: {error}"),
        })
}

fn session(
    executable: String,
    executable_identity: String,
    workspace: &str,
    workspace_identity: &str,
) -> DurableAgentSession {
    let mut session = durable_session(
        "general",
        "codex-agent",
        "Codex",
        "codex_live_session",
        "gpt-5.6-terra",
        "stdio_jsonl",
    );
    "default".clone_into(&mut session.public.service_tier);
    workspace.clone_into(&mut session.workspace);
    workspace_identity.clone_into(&mut session.workspace_identity);
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
        session.public.persona_card_id.as_ref(),
        session.public.transport.as_str(),
    ]);
    session
}
