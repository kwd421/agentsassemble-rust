use std::{ffi::OsStr, path::Path, time::Duration};

use agentsassemble_protocol::LocalControlResponse;
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::{ControlledServer, start_controlled_with_environment};

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

struct ManagedIngressFixture {
    _directory: tempfile::TempDir,
    database: std::path::PathBuf,
    frontend: std::path::PathBuf,
    path: std::ffi::OsString,
    cloudflared: std::path::PathBuf,
}

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

async fn wait_for_managed_ingress(
    server: &mut ControlledServer,
    client: &reqwest::Client,
) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut attempt = 0_u64;
        loop {
            let request_id = format!("managed-ingress-status-{attempt}");
            attempt += 1;
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
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("managed ingress readiness timed out"))
}

async fn start_controlled_with_managed(
    database: &Path,
    frontend: &Path,
    path: &OsStr,
) -> ControlledServer {
    start_controlled_with_environment(database, false, Some(frontend), Some(path)).await
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
