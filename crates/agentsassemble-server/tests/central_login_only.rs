use agentsassemble_protocol::{
    CentralLoginAction, CentralLoginResult, LocalControlRequest, LocalControlResponse,
};
use reqwest::{Client, StatusCode};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

struct LoginProcess {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    address: String,
}

impl LoginProcess {
    async fn start(root: &std::path::Path) -> anyhow::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"))
            .arg("--central-login-only")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("stdin missing"))?;
        let mut output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("stdout missing"))?,
        );
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut line)).await??;
        let record: serde_json::Value = serde_json::from_str(&line)?;
        assert_eq!(record["status"], "ready");
        assert!(record.get("database").is_none());
        let address = record["address"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("address missing"))?
            .to_owned();
        Ok(Self {
            child,
            input,
            output,
            address,
        })
    }

    async fn request(
        &mut self,
        request: LocalControlRequest,
    ) -> anyhow::Result<LocalControlResponse> {
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        self.input.write_all(&bytes).await?;
        self.input.flush().await?;
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), self.output.read_line(&mut line)).await??;
        Ok(serde_json::from_str(&line)?)
    }

    async fn login(
        &mut self,
        action: CentralLoginAction,
        state: &str,
    ) -> anyhow::Result<LocalControlResponse> {
        self.request(LocalControlRequest::CentralLogin {
            request_id: "login-test".into(),
            action,
            state: state.into(),
        })
        .await
    }

    async fn stop(mut self) -> anyhow::Result<()> {
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(6), self.child.wait())
                .await??
                .success()
        );
        let mut diagnostics = String::new();
        self.child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("stderr missing"))?
            .read_to_string(&mut diagnostics)
            .await?;
        assert!(
            diagnostics.is_empty(),
            "authentication must not log payloads"
        );
        Ok(())
    }
}

#[tokio::test]
async fn callback_only_process_preserves_authority_and_never_creates_room_storage()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut process = LoginProcess::start(root.path()).await?;
    let state = "s".repeat(43);
    assert!(matches!(
        process.login(CentralLoginAction::Start, &state).await?,
        LocalControlResponse::CentralLoginOk {
            result: CentralLoginResult::Pending { .. },
            ..
        }
    ));
    for request in [
        LocalControlRequest::InspectBootstrap {
            request_id: "inspect".into(),
        },
        LocalControlRequest::IssueOperatorHttpTicket {
            request_id: "ticket".into(),
        },
        LocalControlRequest::IssueCentralRegistrationTicket {
            request_id: "register".into(),
        },
    ] {
        assert!(
            matches!(process.request(request).await?, LocalControlResponse::Error { code, .. } if code == "login_only")
        );
    }
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    for path in ["/api/rooms", "/mcp", "/app", "/api/central-login/start"] {
        assert_eq!(
            client
                .get(format!("{}{path}", process.address))
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let callback = format!(
        "{}/api/central-login/callback?state={state}&code=fixture-code-123456",
        process.address
    );
    for (header, value) in [
        ("host", "evil.test"),
        ("origin", "https://evil.test"),
        ("x-forwarded-host", "evil.test"),
    ] {
        assert_eq!(
            client
                .get(&callback)
                .header(header, value)
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert!(matches!(
        process.login(CentralLoginAction::Poll, &state).await?,
        LocalControlResponse::CentralLoginOk {
            result: CentralLoginResult::Pending { .. },
            ..
        }
    ));
    let accepted = client.get(&callback).send().await?;
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);
    assert_eq!(accepted.headers()["cache-control"], "private, no-store");
    assert!(
        accepted.headers()["content-security-policy"]
            .to_str()?
            .contains("frame-ancestors 'none'")
    );
    assert!(
        matches!(process.login(CentralLoginAction::Poll, &state).await?,
        LocalControlResponse::CentralLoginOk { result: CentralLoginResult::Complete { authorization_code }, .. }
        if authorization_code == "fixture-code-123456")
    );
    assert!(matches!(
        process.login(CentralLoginAction::Cancel, &state).await?,
        LocalControlResponse::CentralLoginOk {
            result: CentralLoginResult::Cancelled,
            ..
        }
    ));
    // Cancellation retires the return immediately, but the browser can finish its clean redirect.
    assert_eq!(
        client.get(&callback).send().await?.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client
            .get(format!("{}/central-login-complete", process.address))
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    process.stop().await?;
    assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn lost_parent_stops_pending_authentication_without_room_startup() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut process = LoginProcess::start(root.path()).await?;
    process
        .login(CentralLoginAction::Start, &"s".repeat(43))
        .await?;
    let address = process.address.clone();
    process.stop().await?;
    assert!(
        Client::new()
            .get(format!("{address}/central-login-complete"))
            .send()
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
    Ok(())
}
