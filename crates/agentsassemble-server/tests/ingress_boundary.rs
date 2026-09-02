use std::{
    net::{Ipv6Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::join_all;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const PUBLIC_ORIGIN: &str = "https://public.example.test";
const PUBLIC_AUTHORITY: &str = "public.example.test";
const PROXY_SECRET: &str = "manual-ingress-proxy-secret-000000001";
const HELD_PUBLIC_BODY_BYTES: usize = 256;

struct RunningServer {
    address: SocketAddr,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        self.task
            .await
            .unwrap_or_else(|error| panic!("server task join: {error}"));
    }
}

#[tokio::test]
async fn tcp_ingress_enforces_peer_host_origin_and_proxy_boundaries() {
    let server = start(None).await;
    let authority = server.address.to_string();
    let valid = request(
        server.address,
        "GET /healthz HTTP/1.1",
        &format!("Host: {authority}\r\nOrigin: http://{authority}\r\n"),
    )
    .await;
    assert!(valid.starts_with("HTTP/1.1 200"));

    for rejected_headers in [
        format!("Host: {authority}\r\nVia: hostile-proxy\r\n"),
        "Host: 127.0.0.1:9\r\n".to_owned(),
        format!(
            "Host: {authority}\r\nOrigin: http://localhost:{}\r\n",
            server.address.port()
        ),
    ] {
        let response = request(server.address, "GET /healthz HTTP/1.1", &rejected_headers).await;
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "untrusted TCP request was not rejected: {response}"
        );
    }

    let method_mismatch = request(
        server.address,
        "POST /healthz HTTP/1.1",
        &format!("Host: {authority}\r\n"),
    )
    .await;
    assert!(method_mismatch.starts_with("HTTP/1.1 405"));

    let cors_preflight = request(
        server.address,
        "OPTIONS /api/room-invite/admission HTTP/1.1",
        &format!(
            "Host: {authority}\r\nOrigin: tauri://localhost\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: authorization,content-type,x-device-token\r\n"
        ),
    )
    .await;
    assert!(cors_preflight.starts_with("HTTP/1.1 200"));
    server.stop().await;
}

#[tokio::test]
async fn identity_probe_uses_the_persistent_key_and_exact_local_origin() {
    let server = start(None).await;
    let base_url = format!("http://{}", server.address);
    let client = reqwest::Client::new();

    let info_response = client
        .get(format!("{base_url}/api/server-info"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request server identity: {error}"));
    assert_eq!(info_response.status(), reqwest::StatusCode::OK);
    let info: Value = info_response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode server identity: {error}"));
    assert_eq!(info["protocol_version"], 1);
    assert_eq!(info["status"], "ready");
    assert_eq!(info["central_directory"], json!({"enabled": false}));

    let challenge = "Q2hhbGxlbmdlX2Zvcl9waW5uaW5nXzAx";
    let proof_response = client
        .post(format!("{base_url}/api/server-info/challenge"))
        .json(&json!({"challenge": challenge}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request server challenge: {error}"));
    assert_eq!(proof_response.status(), reqwest::StatusCode::OK);
    let proof: Value = proof_response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode server challenge: {error}"));
    assert_eq!(proof["server_id"], info["server_id"]);
    assert_eq!(proof["host_public_key_jwk"], info["host_public_key_jwk"]);
    assert_eq!(proof["host_key_fingerprint"], info["host_key_fingerprint"]);
    assert_eq!(proof["origin"], base_url);
    assert_eq!(proof["challenge"], challenge);
    verify_identity_signature(&proof, &base_url, challenge);

    let invalid = client
        .post(format!("{base_url}/api/server-info/challenge"))
        .json(&json!({"challenge": "short"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invalid challenge: {error}"));
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);

    let foreign_origin = client
        .get(format!("{base_url}/api/server-info"))
        .header("origin", "https://directory.example")
        .send()
        .await
        .unwrap_or_else(|error| panic!("request foreign local probe: {error}"));
    assert_eq!(foreign_origin.status(), reqwest::StatusCode::FORBIDDEN);

    verify_identity_preflight(&client, &base_url).await;
    server.stop().await;
}

#[tokio::test]
async fn identity_challenge_accepts_an_exact_numeric_loopback_listener() {
    let listener = match TcpListener::bind(SocketAddr::from(([127, 0, 0, 2], 0))).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrNotAvailable => {
            TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, 0)))
                .await
                .unwrap_or_else(|error| panic!("bind IPv6 loopback ingress server: {error}"))
        }
        Err(error) => panic!("bind alternate-loopback ingress server: {error}"),
    };
    let server = start_on(listener, None).await;
    let base_url = format!("http://{}", server.address);
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|error| panic!("build direct ingress client: {error}"));
    let challenge = "Q2hhbGxlbmdlX2Zvcl9sb29wYmFja18wMg";
    let response = client
        .post(format!("{base_url}/api/server-info/challenge"))
        .json(&json!({"challenge": challenge}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request alternate-loopback challenge: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let proof: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode alternate-loopback challenge: {error}"));
    assert_eq!(proof["origin"], base_url);
    verify_identity_signature(&proof, &base_url, challenge);
    server.stop().await;
}

#[tokio::test]
async fn manual_public_ingress_enforces_the_real_tcp_route_and_origin_boundary() {
    let (_directory, frontend) = public_frontend_fixture().await;
    let server = start_manual(Some(frontend)).await;
    let base_url = format!("http://{}", server.address);
    let trusted_headers = format!(
        "Host: {PUBLIC_AUTHORITY}\r\nX-Forwarded-Proto: https\r\nX-AgentsAssemble-Proxy-Token: {PROXY_SECRET}\r\n"
    );

    for origin in ["", "Origin: https://public.example.test:443\r\n"] {
        let response = request(
            server.address,
            "GET /join HTTP/1.1",
            &format!("{trusted_headers}{origin}"),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("PUBLIC INDEX"), "{response}");
    }
    for (path, rejected_headers) in [
        (
            "/join",
            format!("{trusted_headers}Origin: https://hostile.example\r\n"),
        ),
        (
            "/join",
            format!(
                "Host: {PUBLIC_AUTHORITY}\r\nX-Forwarded-Proto: https\r\nX-AgentsAssemble-Proxy-Token: wrong\r\n"
            ),
        ),
        (
            "/join",
            format!(
                "Host: hostile.example\r\nX-Forwarded-Proto: https\r\nX-AgentsAssemble-Proxy-Token: {PROXY_SECRET}\r\n"
            ),
        ),
        (
            "/join",
            format!(
                "Host: {PUBLIC_AUTHORITY}\r\nX-Forwarded-Proto: http\r\nX-AgentsAssemble-Proxy-Token: {PROXY_SECRET}\r\n"
            ),
        ),
        ("/healthz", trusted_headers.clone()),
    ] {
        let response = request(
            server.address,
            &format!("GET {path} HTTP/1.1"),
            &rejected_headers,
        )
        .await;
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "untrusted public request was not rejected: {response}"
        );
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|error| panic!("build direct public-ingress client: {error}"));
    let info = client
        .get(format!("{base_url}/api/server-info"))
        .header("host", PUBLIC_AUTHORITY)
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY_SECRET)
        .header("origin", "https://directory.example")
        .send()
        .await
        .unwrap_or_else(|error| panic!("request public server identity: {error}"));
    assert_eq!(info.status(), reqwest::StatusCode::OK);
    assert_eq!(info.headers()["access-control-allow-origin"], "*");

    let challenge = "Q2hhbGxlbmdlX2Zvcl9wdWJsaWNfMDE";
    let proof = client
        .post(format!("{base_url}/api/server-info/challenge"))
        .header("host", PUBLIC_AUTHORITY)
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY_SECRET)
        .header("origin", "https://directory.example")
        .json(&json!({"challenge": challenge}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request public identity challenge: {error}"));
    assert_eq!(proof.status(), reqwest::StatusCode::OK);
    let proof: Value = proof
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode public identity challenge: {error}"));
    assert_eq!(proof["origin"], PUBLIC_ORIGIN);
    verify_identity_signature(&proof, PUBLIC_ORIGIN, challenge);
    server.stop().await;

    let unconfigured = start(None).await;
    let response = request(
        unconfigured.address,
        "GET /api/server-info HTTP/1.1",
        &trusted_headers,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    unconfigured.stop().await;
}

#[tokio::test]
async fn classified_public_saturation_preserves_one_local_http_connection() {
    let server = start_manual(None).await;
    let public_connections =
        join_all((0..127).map(|_| open_public_body_connection(server.address))).await;

    let authority = server.address.to_string();
    let local = request(
        server.address,
        "GET /healthz HTTP/1.1",
        &format!("Host: {authority}\r\n"),
    )
    .await;
    assert!(local.starts_with("HTTP/1.1 200"), "{local}");

    let public_overload = request(
        server.address,
        "GET /api/server-info HTTP/1.1",
        &public_headers(),
    )
    .await;
    assert!(
        public_overload.starts_with("HTTP/1.1 503"),
        "classified public traffic consumed the local reserve: {public_overload}"
    );

    join_all(
        public_connections
            .into_iter()
            .map(complete_public_body_connection),
    )
    .await;
    let public_after_release = request(
        server.address,
        "GET /api/server-info HTTP/1.1",
        &public_headers(),
    )
    .await;
    assert!(public_after_release.starts_with("HTTP/1.1 200"));
    server.stop().await;
}

async fn open_public_body_connection(address: SocketAddr) -> TcpStream {
    let mut socket = TcpStream::connect(address)
        .await
        .unwrap_or_else(|error| panic!("connect public body client: {error}"));
    socket
        .write_all(
            format!(
                "POST /api/server-info/challenge HTTP/1.1\r\n{}Origin: https://directory.example\r\nContent-Type: application/json\r\nContent-Length: {HELD_PUBLIC_BODY_BYTES}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n",
                public_headers()
            )
            .as_bytes(),
        )
        .await
        .unwrap_or_else(|error| panic!("write public body headers: {error}"));
    let response = tokio::time::timeout(Duration::from_secs(2), read_response_head(&mut socket))
        .await
        .unwrap_or_else(|_| panic!("public 100-continue barrier timed out"));
    assert!(response.starts_with("HTTP/1.1 100 Continue"), "{response}");
    socket
}

async fn complete_public_body_connection(mut socket: TcpStream) {
    let mut body = br#"{"challenge":"AAAAAAAAAAAAAAAAAAAAAA"}"#.to_vec();
    body.resize(HELD_PUBLIC_BODY_BYTES, b' ');
    socket
        .write_all(&body)
        .await
        .unwrap_or_else(|error| panic!("complete public body: {error}"));
    let mut response = Vec::new();
    socket
        .read_to_end(&mut response)
        .await
        .unwrap_or_else(|error| panic!("read public terminal response: {error}"));
    let response = String::from_utf8(response)
        .unwrap_or_else(|error| panic!("public terminal response is not UTF-8: {error}"));
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

async fn read_response_head(socket: &mut TcpStream) -> String {
    let mut response = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    loop {
        let read = socket
            .read(&mut buffer)
            .await
            .unwrap_or_else(|error| panic!("read public interim response: {error}"));
        assert!(
            read > 0,
            "public request closed before its interim response"
        );
        response.extend_from_slice(&buffer[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            return String::from_utf8(response)
                .unwrap_or_else(|error| panic!("public interim response is not UTF-8: {error}"));
        }
    }
}

fn public_headers() -> String {
    format!(
        "Host: {PUBLIC_AUTHORITY}\r\nX-Forwarded-Proto: https\r\nX-AgentsAssemble-Proxy-Token: {PROXY_SECRET}\r\n"
    )
}

async fn public_frontend_fixture() -> (tempfile::TempDir, PathBuf) {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create frontend fixture: {error}"));
    let frontend = directory.path().join("frontend");
    tokio::fs::create_dir_all(&frontend)
        .await
        .unwrap_or_else(|error| panic!("create public frontend fixture: {error}"));
    tokio::fs::write(frontend.join("index.html"), "PUBLIC INDEX")
        .await
        .unwrap_or_else(|error| panic!("write public frontend fixture: {error}"));
    (directory, frontend)
}

fn verify_identity_signature(proof: &Value, base_url: &str, challenge: &str) {
    let issued_at = proof["issued_at"]
        .as_i64()
        .unwrap_or_else(|| panic!("challenge issue time is not an integer"));
    let transcript = format!(
        "AA-SERVER-CHALLENGE-1\n{}\n{base_url}\n{challenge}\n{issued_at}",
        proof["server_id"]
            .as_str()
            .unwrap_or_else(|| panic!("server id is not a string"))
    );
    let public_key = URL_SAFE_NO_PAD
        .decode(
            proof["host_public_key_jwk"]["x"]
                .as_str()
                .unwrap_or_else(|| panic!("public key is not a string")),
        )
        .unwrap_or_else(|error| panic!("decode server public key: {error}"));
    let signature = URL_SAFE_NO_PAD
        .decode(
            proof["signature"]
                .as_str()
                .unwrap_or_else(|| panic!("signature is not a string")),
        )
        .unwrap_or_else(|error| panic!("decode server signature: {error}"));
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(transcript.as_bytes(), &signature)
        .unwrap_or_else(|_| panic!("server identity signature did not verify"));
}

async fn verify_identity_preflight(client: &reqwest::Client, base_url: &str) {
    let tauri_preflight = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{base_url}/api/server-info/challenge"),
        )
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .unwrap_or_else(|error| panic!("request identity preflight: {error}"));
    assert!(tauri_preflight.status().is_success());
    assert_eq!(
        tauri_preflight.headers()["access-control-allow-origin"],
        "*"
    );
    let wrong_method_preflight = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{base_url}/api/server-info"),
        )
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .send()
        .await
        .unwrap_or_else(|error| panic!("request mismatched identity preflight: {error}"));
    assert_eq!(
        wrong_method_preflight.status(),
        reqwest::StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn static_routes_match_the_declared_mounts() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create frontend fixture: {error}"));
    let frontend = directory.path().join("frontend");
    tokio::fs::create_dir_all(frontend.join("assets"))
        .await
        .unwrap_or_else(|error| panic!("create asset fixture: {error}"));
    tokio::fs::write(frontend.join("index.html"), "INDEX")
        .await
        .unwrap_or_else(|error| panic!("write index fixture: {error}"));
    tokio::fs::write(frontend.join("assets/app.js"), "ASSET")
        .await
        .unwrap_or_else(|error| panic!("write asset fixture: {error}"));
    let server = start(Some(frontend)).await;
    let authority = server.address.to_string();
    for path in ["/app", "/app/"] {
        let response = request(
            server.address,
            &format!("GET {path} HTTP/1.1"),
            &format!("Host: {authority}\r\n"),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
        assert!(response.contains("INDEX"), "{path}: {response}");
    }
    let app_fallback = request(
        server.address,
        "GET /app/missing HTTP/1.1",
        &format!("Host: {authority}\r\n"),
    )
    .await;
    assert!(app_fallback.starts_with("HTTP/1.1 404"));
    assert!(app_fallback.contains("INDEX"));
    for path in [
        "/app/assets/app.js",
        "/assets/app.js",
        "/join/assets/app.js?v=1",
        "/pair/assets/app.js",
    ] {
        let response = request(
            server.address,
            &format!("GET {path} HTTP/1.1"),
            &format!("Host: {authority}\r\n"),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
        assert!(response.contains("ASSET"), "{path}: {response}");
    }
    let asset_method_mismatch = request(
        server.address,
        "POST /assets/app.js HTTP/1.1",
        &format!("Host: {authority}\r\n"),
    )
    .await;
    assert!(asset_method_mismatch.starts_with("HTTP/1.1 405"));
    for path in [
        "/assets",
        "/assets/",
        "/join/assets",
        "/join/assets/",
        "/pair/assets",
        "/pair/assets/",
    ] {
        let response = request(
            server.address,
            &format!("GET {path} HTTP/1.1"),
            &format!("Host: {authority}\r\n"),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 404"), "{path}: {response}");
    }
    server.stop().await;
}

async fn start(frontend: Option<PathBuf>) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind ingress server: {error}"));
    start_on(listener, frontend).await
}

async fn start_manual(frontend: Option<PathBuf>) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind manual ingress server: {error}"));
    start_on_with_manual(listener, frontend, true).await
}

async fn start_on(listener: TcpListener, frontend: Option<PathBuf>) -> RunningServer {
    start_on_with_manual(listener, frontend, false).await
}

async fn start_on_with_manual(
    listener: TcpListener,
    frontend: Option<PathBuf>,
    manual_public: bool,
) -> RunningServer {
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read ingress address: {error}"));
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open ingress store: {error}"));
    store
        .bootstrap_local_authority("518f301c-e3bf-4b1c-82dd-5853bacb837f", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap ingress identity: {error}"));
    let mut state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 8),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build ingress state: {error}"));
    if manual_public {
        state = state
            .with_manual_public_ingress(address, PUBLIC_ORIGIN, PROXY_SECRET)
            .unwrap_or_else(|error| panic!("configure manual public ingress: {error}"));
    }
    if let Some(frontend) = frontend {
        state = state.with_frontend(frontend);
    }
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve ingress boundary: {error}"));
    });
    RunningServer {
        address,
        cancellation,
        task,
    }
}

async fn request(address: SocketAddr, request_line: &str, headers: &str) -> String {
    let mut socket = TcpStream::connect(address)
        .await
        .unwrap_or_else(|error| panic!("connect ingress client: {error}"));
    socket
        .write_all(format!("{request_line}\r\n{headers}Connection: close\r\n\r\n").as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write ingress request: {error}"));
    let mut response = Vec::new();
    socket
        .read_to_end(&mut response)
        .await
        .unwrap_or_else(|error| panic!("read ingress response: {error}"));
    String::from_utf8(response)
        .unwrap_or_else(|error| panic!("ingress response was not UTF-8: {error}"))
}
