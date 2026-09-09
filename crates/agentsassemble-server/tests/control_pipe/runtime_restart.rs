use super::*;
use agentsassemble_domain::RuntimeRestartPhase;
use agentsassemble_server::runtime_restart_ipc::{
    self, RuntimeControlRequest, RuntimeControlResponse,
};

#[tokio::test]
async fn actual_reexec_preserves_pid_tcp_control_and_replaces_frontend() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("runtime.sqlite3");
    let frontend = directory.path().join("frontend");
    std::fs::create_dir_all(frontend.join("assets"))?;
    std::fs::write(
        frontend.join("index.html"),
        "<!doctype html><html><head></head><body><script type=\"module\" src=\"/assets/app.js\"></script></body></html>",
    )?;
    std::fs::write(frontend.join("assets/app.js"), "console.log('first');")?;
    let mut server =
        start_controlled_with_environment(&database, false, Some(&frontend), None).await;
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    let pid = server.child.id();
    let before = agentsassemble_server::runtime_version::read(&server.address).await?;
    std::fs::write(frontend.join("assets/app.js"), "console.log('second');")?;
    let first = uuid::Uuid::new_v4();
    let LocalControlResponse::OperatorHttpOk { ticket, .. } = server.issue_operator_ticket().await
    else {
        anyhow::bail!("operator ticket missing");
    };
    let accepted = reqwest::Client::new()
        .post(format!("{}/api/runtime/rolling-restart", server.address))
        .bearer_auth(ticket)
        .json(&serde_json::json!({"operation_id": first}))
        .send()
        .await?;
    assert_eq!(accepted.status(), reqwest::StatusCode::ACCEPTED);
    let receipt: agentsassemble_domain::RuntimeRestartReceipt = accepted.json().await?;
    assert_eq!(receipt.phase, RuntimeRestartPhase::Quiescing);
    replacement_ready(&mut server).await?;
    assert_eq!(server.child.id(), pid);
    let after = agentsassemble_server::runtime_version::read(&server.address).await?;
    assert_ne!(before.frontend_build_id, after.frontend_build_id);
    assert_completed(&database, first).await?;
    assert!(matches!(
        server.issue_operator_ticket_for("after-reexec").await,
        LocalControlResponse::OperatorHttpOk { .. }
    ));
    let replay = runtime_restart_ipc::request(
        &database,
        RuntimeControlRequest::Restart {
            operation_id: first,
        },
    )
    .await?;
    assert!(
        matches!(replay, RuntimeControlResponse::Accepted { operation } if operation.phase == RuntimeRestartPhase::Completed)
    );
    // A second replacement must still read the original source, and keep the first receipt.
    std::fs::write(frontend.join("assets/app.js"), "console.log('third');")?;
    let second = uuid::Uuid::new_v4();
    let accepted = runtime_restart_ipc::request(
        &database,
        RuntimeControlRequest::Restart {
            operation_id: second,
        },
    )
    .await?;
    assert!(
        matches!(accepted, RuntimeControlResponse::Accepted { operation } if operation.phase == RuntimeRestartPhase::Quiescing)
    );
    replacement_ready(&mut server).await?;
    assert_eq!(server.child.id(), pid);
    assert_ne!(
        after.frontend_build_id,
        agentsassemble_server::runtime_version::read(&server.address)
            .await?
            .frontend_build_id
    );
    assert_completed(&database, first).await?;
    assert_completed(&database, second).await?;
    assert!(matches!(
        server
            .issue_operator_ticket_for("after-second-reexec")
            .await,
        LocalControlResponse::OperatorHttpOk { .. }
    ));
    server.close_parent_pipe().await;
    let reopened = SqliteStore::open_path(&database).await?;
    assert_eq!(
        reopened
            .runtime_restart_status()
            .await?
            .map(|record| record.phase),
        Some(RuntimeRestartPhase::Completed)
    );
    Ok(())
}

async fn replacement_ready(server: &mut ControlledServer) -> anyhow::Result<()> {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(20), server.output.read_line(&mut line)).await??;
    let ready: Value = serde_json::from_str(&line)?;
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["pid"].as_u64(), server.child.id().map(u64::from));
    assert_eq!(ready["address"], server.address);
    Ok(())
}

async fn assert_completed(database: &Path, operation_id: uuid::Uuid) -> anyhow::Result<()> {
    let response = runtime_restart_ipc::request(
        database,
        RuntimeControlRequest::Status {
            operation_id: Some(operation_id),
        },
    )
    .await?;
    assert!(
        matches!(response, RuntimeControlResponse::Snapshot { runtime } if runtime.supported && runtime.operation.as_ref().is_some_and(|receipt| receipt.phase == RuntimeRestartPhase::Completed && receipt.operation_id == operation_id.to_string()))
    );
    Ok(())
}

#[tokio::test]
async fn incomplete_handoff_refuses_startup_without_binding_or_creating_storage()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let database = directory.path().join("absent.sqlite3");
    for malformed_environment in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"));
        command.arg("--database").arg(&database);
        if malformed_environment {
            command.env("LISTEN_PID", "0").env("LISTEN_FDS", "2");
        } else {
            command
                .arg("--reexec-operation")
                .arg(uuid::Uuid::new_v4().to_string());
        }
        let output = command.output().await?;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!database.exists());
    }
    Ok(())
}
