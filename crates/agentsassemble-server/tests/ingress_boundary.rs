use std::{net::SocketAddr, time::Duration};

use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "ingress-boundary-host-token-000000001";

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
    let server = start().await;
    let authority = server.address.to_string();
    let valid = request(
        server.address,
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
        let response = request(server.address, &rejected_headers).await;
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "untrusted TCP request was not rejected: {response}"
        );
    }
    server.stop().await;
}

async fn start() -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open ingress store: {error}"));
    store
        .bootstrap_local_authority("518f301c-e3bf-4b1c-82dd-5853bacb837f", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap ingress identity: {error}"));
    let state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 8),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate ingress host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build ingress state: {error}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind ingress server: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read ingress address: {error}"));
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

async fn request(address: SocketAddr, headers: &str) -> String {
    let mut socket = TcpStream::connect(address)
        .await
        .unwrap_or_else(|error| panic!("connect ingress client: {error}"));
    socket
        .write_all(
            format!("GET /healthz HTTP/1.1\r\n{headers}Connection: close\r\n\r\n").as_bytes(),
        )
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
