use std::{path::Path, process::Stdio, time::Duration};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
};

const HOST_TOKEN: &str = "control-pipe-test-host-token-000000001";

#[test]
fn command_line_does_not_accept_a_host_secret() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"))
        .arg("--help")
        .output()
        .unwrap_or_else(|error| panic!("run server help: {error}"));
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("decode server help: {error}"));
    assert!(!help.contains("host-token"));
    assert!(!help.contains("AGENTSASSEMBLE_HOST_TOKEN"));
}

struct ControlledServer {
    child: Child,
    control: ChildStdin,
}

impl ControlledServer {
    async fn close_parent_pipe(mut self) {
        drop(self.control);
        let status = tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .unwrap_or_else(|_| panic!("server did not stop after parent control EOF"))
            .unwrap_or_else(|error| panic!("wait for controlled server: {error}"));
        assert!(status.success(), "controlled server exited with {status}");
    }
}

#[tokio::test]
async fn control_pipe_eof_releases_database_for_restart() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");

    start_controlled(&database).await.close_parent_pipe().await;
    start_controlled(&database).await.close_parent_pipe().await;
}

async fn start_controlled(database: &Path) -> ControlledServer {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"))
        .args([
            "--bind",
            "127.0.0.1:0",
            "--database",
            database
                .to_str()
                .unwrap_or_else(|| panic!("database path is not UTF-8")),
            "--bootstrap-room",
            "general",
        ])
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn controlled server: {error}"));
    let mut control = child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("controlled server has no stdin"));
    control
        .write_all(format!("{HOST_TOKEN}\n").as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write control secret: {error}"));
    control
        .flush()
        .await
        .unwrap_or_else(|error| panic!("flush control secret: {error}"));

    let stdout = child
        .stdout
        .take()
        .unwrap_or_else(|| panic!("controlled server has no stdout"));
    let mut readiness = String::new();
    tokio::time::timeout(
        Duration::from_secs(3),
        BufReader::new(stdout).read_line(&mut readiness),
    )
    .await
    .unwrap_or_else(|_| panic!("controlled server readiness timed out"))
    .unwrap_or_else(|error| panic!("read controlled server readiness: {error}"));
    let record: Value = serde_json::from_str(readiness.trim())
        .unwrap_or_else(|error| panic!("parse controlled server readiness: {error}"));
    assert_eq!(record["status"], "ready");
    assert_eq!(record["runtime"], "rust");
    assert_eq!(record["pid"].as_u64(), child.id().map(u64::from));

    ControlledServer { child, control }
}
