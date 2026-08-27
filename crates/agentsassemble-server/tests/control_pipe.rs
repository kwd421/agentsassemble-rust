use std::{ffi::OsStr, path::Path, process::Stdio, time::Duration};

use agentsassemble_persistence::SqliteStore;
use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::{Child, ChildStdin, ChildStdout, Command},
};
mod control_pipe_invite_tickets;
const HOST_TOKEN: &str = "control-pipe-test-host-token-000000001";
const PUBLIC_ORIGIN: &str = "https://public.example.test";
const PROXY_SECRET: &str = "manual-ingress-control-secret-000000001";
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
    output: BufReader<ChildStdout>,
    address: String,
}

impl ControlledServer {
    async fn initialize_bootstrap(&mut self) -> LocalControlResponse {
        let request = LocalControlRequest::InitializeBootstrap {
            request_id: "572341d5-a6a7-47cc-8a74-a5b328645f05".to_owned(),
            display_name: "Control Operator".to_owned(),
        };
        self.send_control(&request).await
    }

    async fn close_parent_pipe(mut self) {
        drop(self.control);
        let status = tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .unwrap_or_else(|_| panic!("server did not stop after parent control EOF"))
            .unwrap_or_else(|error| panic!("wait for controlled server: {error}"));
        assert!(status.success(), "controlled server exited with {status}");
    }

    async fn issue_ticket(&mut self) -> LocalControlResponse {
        let request = LocalControlRequest::IssueTicket {
            request_id: "control-ticket-1".to_owned(),
            meeting_id: "general".to_owned(),
        };
        self.send_control(&request).await
    }

    async fn issue_operator_ticket(&mut self) -> LocalControlResponse {
        self.issue_operator_ticket_for("control-operator-ticket-1")
            .await
    }

    async fn issue_operator_ticket_for(&mut self, request_id: &str) -> LocalControlResponse {
        let request = LocalControlRequest::IssueOperatorHttpTicket {
            request_id: request_id.to_owned(),
        };
        self.send_control(&request).await
    }

    async fn issue_central_registration_ticket(&mut self) -> LocalControlResponse {
        let request = LocalControlRequest::IssueCentralRegistrationTicket {
            request_id: "control-central-registration-ticket-1".to_owned(),
        };
        self.send_control(&request).await
    }

    async fn send_control(&mut self, request: &LocalControlRequest) -> LocalControlResponse {
        let mut encoded = serde_json::to_vec(request)
            .unwrap_or_else(|error| panic!("encode control request: {error}"));
        encoded.push(b'\n');
        self.control
            .write_all(&encoded)
            .await
            .unwrap_or_else(|error| panic!("write control request: {error}"));
        self.control
            .flush()
            .await
            .unwrap_or_else(|error| panic!("flush control request: {error}"));
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), self.output.read_line(&mut line))
            .await
            .unwrap_or_else(|_| panic!("control response timed out"))
            .unwrap_or_else(|error| panic!("read control response: {error}"));
        serde_json::from_str(line.trim())
            .unwrap_or_else(|error| panic!("decode control response: {error}"))
    }
}

#[tokio::test]
async fn control_pipe_eof_releases_database_for_restart() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");

    let mut first_server = start_controlled(&database).await;
    assert!(matches!(
        first_server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    first_server.close_parent_pipe().await;
    let first = SqliteStore::open_path(&database)
        .await
        .unwrap_or_else(|error| panic!("open first initialized authority: {error}"));
    let first_server_id = first
        .server_id()
        .await
        .unwrap_or_else(|error| panic!("read first server identity: {error}"));
    let first_rooms = first
        .list_room_directory(true)
        .await
        .unwrap_or_else(|error| panic!("read first room directory: {error}"));
    assert!(first_rooms.is_empty());
    drop(first);

    start_controlled(&database).await.close_parent_pipe().await;
    let reopened = SqliteStore::open_path(&database)
        .await
        .unwrap_or_else(|error| panic!("reopen initialized authority: {error}"));
    assert_eq!(
        reopened
            .server_id()
            .await
            .unwrap_or_else(|error| panic!("read reopened server identity: {error}")),
        first_server_id
    );
    let reopened_rooms = reopened
        .list_room_directory(true)
        .await
        .unwrap_or_else(|error| panic!("read reopened room directory: {error}"));
    assert!(reopened_rooms.is_empty());
}

#[tokio::test]
async fn owned_control_pipe_issues_proof_bound_ticket_without_http_secret() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let mut server = start_controlled(&database).await;
    assert!(matches!(
        server.issue_ticket().await,
        LocalControlResponse::Error { code, .. } if code == "bootstrap_required"
    ));
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    let operator = server.issue_operator_ticket().await;
    let LocalControlResponse::OperatorHttpOk { ticket, .. } = operator else {
        panic!("operator ticket request was rejected");
    };
    let created = reqwest::Client::new()
        .post(format!("{}/api/rooms", server.address))
        .bearer_auth(ticket)
        .json(&serde_json::json!({
            "request_id": "22000000-0000-4000-8000-000000000020",
            "room_id": "general",
            "label": "General"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("create first room: {error}"));
    assert!(created.status().is_success());
    let response = server.issue_ticket().await;
    let LocalControlResponse::Ok {
        request_id,
        ticket,
        ttl_seconds,
        server_proof_key,
    } = response
    else {
        panic!("control ticket request was rejected");
    };
    assert_eq!(request_id, "control-ticket-1");
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    assert_eq!(server_proof_key.len(), 64);
    server.close_parent_pipe().await;
}

#[tokio::test]
async fn owned_control_pipe_issues_a_distinct_operator_http_ticket() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let mut server = start_controlled(&database).await;
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    let response = server.issue_operator_ticket().await;
    let LocalControlResponse::OperatorHttpOk {
        request_id,
        ticket,
        ttl_seconds,
    } = response
    else {
        panic!("operator HTTP ticket request was rejected");
    };
    assert_eq!(request_id, "control-operator-ticket-1");
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    server.close_parent_pipe().await;
}

#[tokio::test]
async fn public_ingress_status_requires_one_exact_operator_ticket() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let mut server = start_controlled(&database).await;
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    let url = format!("{}/api/public-invite/status", server.address);
    let client = reqwest::Client::new();
    let preflight = client
        .request(reqwest::Method::OPTIONS, &url)
        .header(reqwest::header::ORIGIN, "tauri://localhost")
        .header(
            reqwest::header::ACCESS_CONTROL_REQUEST_METHOD,
            reqwest::Method::GET.as_str(),
        )
        .header(
            reqwest::header::ACCESS_CONTROL_REQUEST_HEADERS,
            reqwest::header::AUTHORIZATION.as_str(),
        )
        .send()
        .await
        .unwrap_or_else(|error| panic!("request ingress status preflight: {error}"));
    assert!(preflight.status().is_success());
    assert_eq!(
        preflight.headers()[reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "tauri://localhost"
    );
    assert_eq!(
        preflight.headers()[reqwest::header::ACCESS_CONTROL_ALLOW_METHODS],
        "GET,POST"
    );
    let allowed_headers = preflight.headers()[reqwest::header::ACCESS_CONTROL_ALLOW_HEADERS]
        .to_str()
        .unwrap_or_else(|error| panic!("decode ingress status CORS headers: {error}"));
    assert!(allowed_headers.contains("authorization"));
    assert_eq!(
        client
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|error| panic!("request unauthorized ingress status: {error}"))
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    let LocalControlResponse::OperatorHttpOk { ticket, .. } = server.issue_operator_ticket().await
    else {
        panic!("operator ticket request was rejected");
    };
    let response = client
        .get(&url)
        .bearer_auth(&ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("request ingress status: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers()[reqwest::header::CACHE_CONTROL],
        "private, no-store"
    );
    let status: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode ingress status: {error}"));
    assert_eq!(status["mode"], "managed");
    assert_eq!(status["tunnel"]["phase"], "stopped");
    assert_eq!(status["tunnel"]["stable_phase"], "unconfigured");
    assert_eq!(
        client
            .get(url)
            .bearer_auth(ticket)
            .send()
            .await
            .unwrap_or_else(|error| panic!("reuse ingress status ticket: {error}"))
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    server.close_parent_pipe().await;
}

#[cfg(unix)]
#[tokio::test]
async fn managed_public_ingress_enforces_real_process_tcp_and_revocation_boundaries() {
    let fixture = managed_ingress_fixture().await;
    let mut server =
        start_controlled_with_managed(&fixture.database, &fixture.frontend, &fixture.path).await;
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|error| panic!("build managed-ingress client: {error}"));
    let start_ticket = operator_ticket(&mut server, "managed-ingress-start").await;
    let start = client
        .post(format!("{}/api/public-invite/tunnel/start", server.address))
        .bearer_auth(start_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("start managed ingress: {error}"));
    assert_eq!(start.status(), reqwest::StatusCode::OK);
    assert_eq!(
        start.headers()[reqwest::header::CACHE_CONTROL],
        "private, no-store"
    );

    let running = wait_for_managed_ingress(&mut server, &client).await;
    assert_eq!(
        running["public_url"],
        "https://soft-river-demo.trycloudflare.com"
    );
    let arguments = tokio::fs::read_to_string(fixture.cloudflared.with_extension("args"))
        .await
        .unwrap_or_else(|error| panic!("read fake cloudflared arguments: {error}"));
    let arguments = arguments.lines().collect::<Vec<_>>();
    assert!(arguments.contains(&"--no-autoupdate"));
    assert!(
        arguments
            .windows(2)
            .any(|pair| { pair[0] == "--url" && pair[1] == server.address })
    );
    let origin_host = arguments
        .windows(2)
        .find_map(|pair| (pair[0] == "--http-host-header").then_some(pair[1]))
        .unwrap_or_else(|| panic!("cloudflared origin host argument is missing"));
    assert!(origin_host.ends_with(".origin.invalid"));
    let trusted_headers = format!(
        "Host: {origin_host}\r\nX-Forwarded-Host: soft-river-demo.trycloudflare.com\r\nX-Forwarded-Proto: https\r\nOrigin: https://soft-river-demo.trycloudflare.com\r\n"
    );
    let valid = tcp_request(&server.address, "GET /join HTTP/1.1", &trusted_headers).await;
    assert!(valid.starts_with("HTTP/1.1 200"));
    assert!(valid.contains("MANAGED PUBLIC INDEX"));
    for (path, rejected_headers) in [
        (
            "/join",
            trusted_headers.replace(
                "X-Forwarded-Host: soft-river-demo.trycloudflare.com",
                "X-Forwarded-Host: hostile.example",
            ),
        ),
        (
            "/join",
            trusted_headers.replace("X-Forwarded-Proto: https", "X-Forwarded-Proto: http"),
        ),
        (
            "/join",
            trusted_headers.replace(
                "Origin: https://soft-river-demo.trycloudflare.com",
                "Origin: https://hostile.example",
            ),
        ),
        ("/api/public-invite/status", trusted_headers.clone()),
    ] {
        let response = tcp_request(
            &server.address,
            &format!("GET {path} HTTP/1.1"),
            &rejected_headers,
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 403"));
    }

    let stop_ticket = operator_ticket(&mut server, "managed-ingress-stop").await;
    let stopped: Value = client
        .post(format!("{}/api/public-invite/tunnel/stop", server.address))
        .bearer_auth(stop_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("stop managed ingress: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode stopped managed ingress: {error}"));
    assert_eq!(stopped["tunnel"]["phase"], "stopped");
    assert!(fixture.cloudflared.with_extension("stopped").is_file());
    let descendant = fixture.cloudflared.with_extension("descendant-stopped");
    assert!(descendant.is_file());
    let revoked = tcp_request(&server.address, "GET /join HTTP/1.1", &trusted_headers).await;
    assert!(revoked.starts_with("HTTP/1.1 403"));
    server.close_parent_pipe().await;
}

#[cfg(unix)]
struct ManagedIngressFixture {
    _directory: tempfile::TempDir,
    database: std::path::PathBuf,
    frontend: std::path::PathBuf,
    path: std::ffi::OsString,
    cloudflared: std::path::PathBuf,
}

#[cfg(unix)]
async fn managed_ingress_fixture() -> ManagedIngressFixture {
    use std::os::unix::fs::PermissionsExt;

    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let bin = directory.path().join("bin");
    let frontend = directory.path().join("frontend");
    tokio::fs::create_dir(&bin)
        .await
        .unwrap_or_else(|error| panic!("create fake cloudflared directory: {error}"));
    tokio::fs::create_dir(&frontend)
        .await
        .unwrap_or_else(|error| panic!("create frontend fixture: {error}"));
    tokio::fs::write(frontend.join("index.html"), "MANAGED PUBLIC INDEX")
        .await
        .unwrap_or_else(|error| panic!("write frontend fixture: {error}"));
    let cloudflared = bin.join("cloudflared");
    tokio::fs::write(
        &cloudflared,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
(
  trap 'printf stopped > "$0.descendant-stopped"; exit 0' TERM INT
  while :; do
    /bin/sleep 60 &
    wait $!
  done
) &
descendant=$!
trap 'wait "$descendant"; printf stopped > "$0.stopped"; exit 0' TERM INT
printf '%s\n' 'INF Visit https://soft-river-demo.trycloudflare.com'
wait "$descendant"
"#,
    )
    .await
    .unwrap_or_else(|error| panic!("write fake cloudflared: {error}"));
    std::fs::set_permissions(&cloudflared, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("make fake cloudflared executable: {error}"));
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap_or_else(|error| panic!("build managed-ingress PATH: {error}"));
    ManagedIngressFixture {
        _directory: directory,
        database,
        frontend,
        path,
        cloudflared,
    }
}

#[cfg(unix)]
async fn wait_for_managed_ingress(
    server: &mut ControlledServer,
    client: &reqwest::Client,
) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        for attempt in 0..256 {
            let request_id = format!("managed-ingress-status-{attempt}");
            let ticket = operator_ticket(server, &request_id).await;
            let response = client
                .get(format!("{}/api/public-invite/status", server.address))
                .bearer_auth(ticket)
                .send()
                .await
                .unwrap_or_else(|error| panic!("read managed-ingress status: {error}"));
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let status: Value = response
                .json()
                .await
                .unwrap_or_else(|error| panic!("decode managed-ingress status: {error}"));
            if status["tunnel"]["phase"] == "running" {
                return status;
            }
            assert_ne!(
                status["tunnel"]["phase"], "error",
                "managed ingress failed: {}",
                status["tunnel"]["last_error"]
            );
            tokio::task::yield_now().await;
        }
        panic!("managed ingress readiness attempt limit reached");
    })
    .await
    .unwrap_or_else(|_| panic!("managed ingress readiness timed out"))
}

#[tokio::test]
async fn owned_control_pipe_issues_exact_settings_tickets_after_authority_exists() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let mut server = start_controlled(&database).await;

    let before_bootstrap = server
        .send_control(&LocalControlRequest::IssueSettingsDirectoryReadTicket {
            request_id: "settings-directory-before-bootstrap".to_owned(),
        })
        .await;
    assert!(matches!(
        before_bootstrap,
        LocalControlResponse::Error { code, .. } if code == "bootstrap_required"
    ));
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));

    let LocalControlResponse::OperatorHttpOk { ticket, .. } = server.issue_operator_ticket().await
    else {
        panic!("operator ticket request was rejected");
    };
    let created = reqwest::Client::new()
        .post(format!("{}/api/rooms", server.address))
        .bearer_auth(ticket)
        .json(&serde_json::json!({
            "request_id": "23000000-0000-4000-8000-000000000023",
            "room_id": "general",
            "label": "General"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("create settings test room: {error}"));
    assert!(created.status().is_success());

    let read = server
        .send_control(&LocalControlRequest::IssuePreferencesReadTicket {
            request_id: "preferences-read-ticket-1".to_owned(),
            meeting_id: "general".to_owned(),
        })
        .await;
    assert!(matches!(
        read,
        LocalControlResponse::PreferencesReadOk {
            request_id,
            ticket,
            ttl_seconds,
        } if request_id == "preferences-read-ticket-1"
            && ticket.len() == 64
            && ttl_seconds > 0
    ));

    let write = server
        .send_control(&LocalControlRequest::IssuePreferencesWriteTicket {
            request_id: "preferences-write-ticket-1".to_owned(),
            meeting_id: "general".to_owned(),
        })
        .await;
    assert!(matches!(
        write,
        LocalControlResponse::PreferencesWriteOk {
            request_id,
            ticket,
            ttl_seconds,
        } if request_id == "preferences-write-ticket-1"
            && ticket.len() == 64
            && ttl_seconds > 0
    ));

    control_pipe_invite_tickets::assert_invite_tickets(&mut server).await;

    let directory = server
        .send_control(&LocalControlRequest::IssueSettingsDirectoryReadTicket {
            request_id: "settings-directory-ticket-1".to_owned(),
        })
        .await;
    assert!(matches!(
        directory,
        LocalControlResponse::SettingsDirectoryReadOk {
            request_id,
            ticket,
            ttl_seconds,
        } if request_id == "settings-directory-ticket-1"
            && ticket.len() == 64
            && ttl_seconds > 0
    ));
    server.close_parent_pipe().await;
}

#[tokio::test]
async fn owned_control_pipe_issues_a_purpose_bound_central_registration_ticket() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let mut server = start_controlled(&database).await;
    assert!(matches!(
        server.issue_central_registration_ticket().await,
        LocalControlResponse::Error { code, .. } if code == "bootstrap_required"
    ));
    assert!(matches!(
        server.initialize_bootstrap().await,
        LocalControlResponse::BootstrapOk { .. }
    ));
    let response = server.issue_central_registration_ticket().await;
    let LocalControlResponse::CentralRegistrationOk {
        request_id,
        ticket,
        ttl_seconds,
        server_id,
        host_public_key_x,
        host_key_fingerprint,
    } = response
    else {
        panic!("central registration ticket request was rejected");
    };
    assert_eq!(request_id, "control-central-registration-ticket-1");
    assert_eq!(ticket.len(), 64);
    assert!(ttl_seconds > 0);
    assert!(uuid::Uuid::parse_str(&server_id).is_ok());
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(host_public_key_x)
            .unwrap_or_else(|error| panic!("decode host public key: {error}"))
            .len(),
        32
    );
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(host_key_fingerprint)
            .unwrap_or_else(|error| panic!("decode host fingerprint: {error}"))
            .len(),
        32
    );
    server.close_parent_pipe().await;
}

#[tokio::test]
async fn startup_manual_public_ingress_requires_a_pair_and_reaches_identity() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database = directory.path().join("runtime.sqlite3");
    let output = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"))
        .args(["--database", database.to_string_lossy().as_ref()])
        .env("AGENTSASSEMBLE_PUBLIC_URL", PUBLIC_ORIGIN)
        .env_remove("AGENTSASSEMBLE_TRUSTED_PROXY_TOKEN")
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap_or_else(|error| panic!("run incomplete manual ingress: {error}"));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("must be configured together"),
        "unexpected startup failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let server = start_controlled_with_manual(&database).await;
    let identity = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|error| panic!("build manual-ingress client: {error}"))
        .get(format!("{}/api/server-info", server.address))
        .header("host", "public.example.test")
        .header("origin", "https://directory.example")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY_SECRET)
        .send()
        .await
        .unwrap_or_else(|error| panic!("request startup-configured identity: {error}"));
    assert_eq!(identity.status(), reqwest::StatusCode::OK);
    server.close_parent_pipe().await;
}

async fn start_controlled(database: &Path) -> ControlledServer {
    start_controlled_with_environment(database, false, None, None).await
}

async fn start_controlled_with_manual(database: &Path) -> ControlledServer {
    start_controlled_with_environment(database, true, None, None).await
}

async fn start_controlled_with_managed(
    database: &Path,
    frontend: &Path,
    path: &OsStr,
) -> ControlledServer {
    start_controlled_with_environment(database, false, Some(frontend), Some(path)).await
}

async fn start_controlled_with_environment(
    database: &Path,
    manual_public: bool,
    frontend: Option<&Path>,
    path: Option<&OsStr>,
) -> ControlledServer {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"));
    command
        .args([
            "--bind",
            "127.0.0.1:0",
            "--database",
            database
                .to_str()
                .unwrap_or_else(|| panic!("database path is not UTF-8")),
        ])
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .env_remove("AGENTSASSEMBLE_PUBLIC_URL")
        .env_remove("AGENTSASSEMBLE_TRUSTED_PROXY_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if manual_public {
        command
            .env("AGENTSASSEMBLE_PUBLIC_URL", PUBLIC_ORIGIN)
            .env("AGENTSASSEMBLE_TRUSTED_PROXY_TOKEN", PROXY_SECRET);
    }
    if let Some(frontend) = frontend {
        command.arg("--frontend").arg(frontend);
    }
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command
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
    let mut output = BufReader::new(stdout);
    let mut readiness = String::new();
    tokio::time::timeout(Duration::from_secs(3), output.read_line(&mut readiness))
        .await
        .unwrap_or_else(|_| panic!("controlled server readiness timed out"))
        .unwrap_or_else(|error| panic!("read controlled server readiness: {error}"));
    let record: Value = serde_json::from_str(readiness.trim())
        .unwrap_or_else(|error| panic!("parse controlled server readiness: {error}"));
    assert_eq!(record["status"], "ready");
    assert_eq!(record["runtime"], "rust");
    assert_eq!(record["pid"].as_u64(), child.id().map(u64::from));
    let address = record["address"]
        .as_str()
        .unwrap_or_else(|| panic!("controlled server has no address"))
        .trim_end_matches('/')
        .to_owned();

    ControlledServer {
        child,
        control,
        output,
        address,
    }
}

async fn operator_ticket(server: &mut ControlledServer, request_id: &str) -> String {
    let LocalControlResponse::OperatorHttpOk { ticket, .. } =
        server.issue_operator_ticket_for(request_id).await
    else {
        panic!("operator ticket request was rejected");
    };
    ticket
}

async fn tcp_request(address: &str, request_line: &str, headers: &str) -> String {
    let authority = address
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("controlled server address is not HTTP"));
    let mut socket = TcpStream::connect(authority)
        .await
        .unwrap_or_else(|error| panic!("connect managed-ingress client: {error}"));
    socket
        .write_all(format!("{request_line}\r\n{headers}Connection: close\r\n\r\n").as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write managed-ingress request: {error}"));
    let mut response = Vec::new();
    socket
        .read_to_end(&mut response)
        .await
        .unwrap_or_else(|error| panic!("read managed-ingress response: {error}"));
    String::from_utf8(response)
        .unwrap_or_else(|error| panic!("managed-ingress response was not UTF-8: {error}"))
}
