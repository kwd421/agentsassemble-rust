#![cfg(unix)]
use serde_json::json;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};
#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/local_socket.rs"]
mod local_socket;
#[path = "support/provider_fixture.rs"]
mod provider_fixture;
#[path = "support/room_portal_fixture.rs"]
mod room_portal_fixture;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn actual_attend_cli_discovers_joins_reports_and_confirms_remote_stop() -> TestResult {
    let directory = tempfile::tempdir()?;
    let bin = fixture_bundle(directory.path())?;
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    let _snapshot = manager.receive_json().await;
    let mut child = Command::new(env!("CARGO_BIN_EXE_assemble"))
        .args([
            "room",
            "attend",
            "--provider",
            "codex",
            "--display-name",
            "CLI Attendee",
        ])
        .env(
            "PATH",
            std::env::join_paths([
                bin,
                std::path::PathBuf::from("/usr/bin"),
                std::path::PathBuf::from("/bin"),
            ])?,
        )
        .env("CODEX_HOME", directory.path().join("codex-home"))
        .env("TMPDIR", directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let url = format!("{}/join?token={}\n", server.base_url, invite.invite_bearer);
    child
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(url.as_bytes())
        .await?;
    let session_id = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let snapshot = store.snapshot("general", 0, 200).await?;
            if let Some(session) = snapshot
                .agent_sessions
                .iter()
                .find(|session| session.provider_session_active)
            {
                break Ok::<_, agentsassemble_persistence::PersistenceError>(
                    session.session_id.clone(),
                );
            }
            let _event = manager.receive_json().await;
        }
    })
    .await??;
    manager.send_json(&json!({"op":"command","request_id":"cli-input","action":"message.send","payload":{"content":"CLI turn"}})).await;
    room_portal_fixture::wait_for_turn(&directory.path().join("seen"), "1").await;
    let endpoint =
        room_portal_fixture::wait_for_value(&directory.path().join("endpoint"), "endpoint").await;
    let token = room_portal_fixture::wait_for_value(&directory.path().join("token"), "token").await;
    room_portal_fixture::publish(&endpoint, &token, "CLI fixture answer").await;
    std::fs::write(directory.path().join("first"), b"go")?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = manager.receive_json().await;
            if event.to_string().contains("CLI fixture answer") {
                break;
            }
        }
    })
    .await?;
    manager.send_json(&json!({"op":"command","request_id":"cli-stop","action":"agent.stop","payload":{"agent_id":session_id}})).await;
    let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output()).await??;
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostics.contains("room cleanup confirmed"));
    assert!(!diagnostics.contains(&invite.invite_bearer));
    assert!(output.stdout.is_empty());
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(!snapshot.agent_sessions[0].provider_session_active);
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    manager.close().await;
    server.stop().await;
    Ok(())
}

fn fixture_bundle(root: &Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        _ => return Err("unsupported fixture platform".into()),
    };
    let bin = root.join("bin");
    let vendor = root.join("vendor").join(target).join("bin");
    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&vendor)?;
    let script = room_portal_fixture::script(
        &root.join("requests"),
        &root.join("endpoint"),
        &root.join("token"),
        &root.join("seen"),
        &root.join("first"),
        &root.join("second"),
        "completed",
    );
    let script = script.replacen("#!/bin/sh", r#"#!/bin/sh
if [ "$1 $2" = "login status" ]; then
    printf '%s\n' 'Logged in using an API key' >&2
    exit 0
fi
if [ "$1" = debug ]; then
    printf '%s\n' '{"models":[{"slug":"gpt-5.6-luna","display_name":"Fixture","supported_reasoning_levels":[{"effort":"low"}],"service_tiers":[]}]}'
    exit 0
fi"#, 1);
    let catalog = provider_fixture::agent_catalog(&vendor, Some(script.as_bytes()));
    let executable = &catalog.providers[0].executable;
    std::fs::rename(executable, vendor.join("codex"))?;
    std::fs::copy(vendor.join("codex"), bin.join("codex"))?;
    Ok(bin)
}
