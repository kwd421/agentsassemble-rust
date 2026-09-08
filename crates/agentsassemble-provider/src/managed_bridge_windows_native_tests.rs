//! Real Windows native child and grandchild containment through the managed bridge.
use agentsassemble_domain::DurableAgentSession;
use std::{path::Path, time::Duration};

use crate::{
    credentials::ProviderCredentialStore,
    provider_factory::{DriverFactory, ProductionDriverFactory},
    runtime_lease::HeldRuntimeLease,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// Standalone native protocol fixture, compiled by the Windows runner's Rust toolchain.
// The descendant deliberately outlives its parent and has no inherited stdio handles.
const NATIVE: &str = r#"
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--descendant") {
        loop { std::thread::park(); }
    }
    let mut input = io::stdin().lock().lines();
    input.next().ok_or("initialize missing")??;
    println!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}");
    io::stdout().flush()?;
    input.next().ok_or("initialized missing")??;
    input.next().ok_or("thread missing")??;
    let child = Command::new(std::env::current_exe()?).arg("--descendant")
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::fs::write("native-pids", format!("{} {}", std::process::id(), child.id()))?;
    println!("{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}");
    io::stdout().flush()?;
    for line in input { line?; }
    Ok(())
}
"#;

#[tokio::test]
async fn managed_native_stop_and_pipe_loss_remove_the_entire_nested_job() -> TestResult {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("native.rs");
    let executable = directory.path().join("native.exe");
    std::fs::write(&source, NATIVE)?;
    let built = tokio::process::Command::new("rustc")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .kill_on_drop(true)
        .output()
        .await?;
    assert!(
        built.status.success(),
        "fixture compilation: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    for loss in [false, true] {
        tokio::time::timeout(
            Duration::from_secs(20),
            native_case(directory.path(), &executable, loss),
        )
        .await??;
    }
    Ok(())
}

async fn native_case(directory: &Path, executable: &Path, loss: bool) -> TestResult {
    let (session, mut lease) = native_session(directory, executable).await?;
    let factory = ProductionDriverFactory::local(ProviderCredentialStore::production());
    if loss {
        lost_pipe(&factory, &session, &lease, directory).await?;
    } else {
        let mut driver = factory
            .launch(&session, &lease)
            .await
            .map_err(|failure| failure.error)?;
        let attached = driver.attach_session(&session).await?;
        assert_eq!(attached.provider_session_id, "thread-1");
        assert_members(&lease, directory)?;
        driver.stop().await?;
    }
    assert!(lease.cleanup_receipt_is_present());
    lease.release_and_remove();
    Ok(())
}

async fn lost_pipe(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    lease: &HeldRuntimeLease,
    directory: &Path,
) -> TestResult {
    use super::super::wire::{self, Command, Event, Launch, read, write};
    let super::super::Spawn {
        mut child,
        connection,
        proof,
    } = super::spawn(factory, session, lease)
        .await
        .map_err(|failure| failure.error)?;
    let (input, output) = connection.await?;
    let mut input = wire::reader(input);
    let mut output = wire::writer(output);
    write(
        &mut output,
        &Launch {
            session: Box::new(session.clone()),
            credential: None,
            state_root: None,
        },
    )
    .await?;
    assert!(matches!(read(&mut input).await?, Some(Event::Acquired)));
    assert!(matches!(read(&mut input).await?, Some(Event::Facts { .. })));
    assert!(matches!(
        read(&mut input).await?,
        Some(Event::Ready { result: Ok(()) })
    ));
    write(
        &mut output,
        &Command::Attach {
            id: 1,
            session: Box::new(session.clone()),
        },
    )
    .await?;
    // Every command result is preceded by the worker's refreshed runtime facts.
    assert!(matches!(read(&mut input).await?, Some(Event::Facts { .. })));
    assert!(matches!(
        read(&mut input).await?,
        Some(Event::Attached { result: Ok(_), .. })
    ));
    assert_members(lease, directory)?;
    drop(input);
    drop(output);
    // Losing private transport must wake the worker and clean its native Job even
    // while the native fixture has a live descendant and no provider turn.
    assert!(!super::exited_successfully(child.wait().await?));
    assert!(proof.is_gone());
    Ok(())
}

fn assert_members(lease: &HeldRuntimeLease, directory: &Path) -> TestResult {
    let pids = std::fs::read_to_string(directory.join("native-pids"))?
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(pids.len(), 2);
    let members = lease.windows_custody()?.group().members()?;
    assert!(
        members.len() >= 3,
        "worker, native leader and descendant must remain owned"
    );
    eprintln!(
        "owned_windows_job_members={:?}",
        lease.windows_custody()?.group().members_info()?
    );
    assert!(pids.iter().all(|pid| members.contains(pid)));
    assert!(!lease.cleanup_receipt_is_present());
    Ok(())
}

async fn native_session(
    directory: &Path,
    executable: &Path,
) -> Result<(DurableAgentSession, HeldRuntimeLease), Box<dyn std::error::Error>> {
    let mut session = crate::test_support::durable_session(
        &uuid::Uuid::new_v4().to_string(),
        "windows-native",
        "Codex",
        crate::registration::CODEX_PROVIDER.provider_kind,
        "gpt-5.6-terra",
        crate::registration::CODEX_PROVIDER.transport,
    );
    session.executable = executable.canonicalize()?.to_string_lossy().into_owned();
    session.executable_identity =
        crate::filesystem::executable_identity(session.executable.clone())
            .await
            .map_err(|error| format!("fixture identity: {error:?}"))?;
    (session.workspace, session.workspace_identity) =
        crate::filesystem::canonical_workspace(directory.to_string_lossy().into_owned())
            .await
            .map_err(|error| format!("fixture workspace: {error:?}"))?;
    "default".clone_into(&mut session.public.service_tier);
    let lease = HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)?;
    session.runtime_handle_id = lease.new_runtime_handle_id();
    session.runtime_lease_token = lease.token().to_owned();
    "windows-native-parent".clone_into(&mut session.runtime_owner_id);
    lease.begin_launch_effect()?;
    Ok((session, lease))
}
