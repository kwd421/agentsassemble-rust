use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    response::Response,
};
use reqwest::Client;
use std::{collections::HashSet, sync::Arc};
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Relay {
    upstream: String,
    http: Client,
    pending_loss: Arc<Mutex<HashSet<String>>>,
}

pub struct LossyHttpRelay {
    pub base_url: String,
    cancellation: CancellationToken,
    task: JoinHandle<std::io::Result<()>>,
}

impl LossyHttpRelay {
    pub async fn start(upstream: &str, paths: &[&str]) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let cancellation = CancellationToken::new();
        let stopping = cancellation.clone();
        let relay = Router::new().fallback(forward).with_state(Relay {
            upstream: upstream.to_owned(),
            http: Client::new(),
            pending_loss: Arc::new(Mutex::new(
                paths.iter().map(|path| (*path).to_owned()).collect(),
            )),
        });
        let task = tokio::spawn(async move {
            axum::serve(listener, relay)
                .with_graceful_shutdown(stopping.cancelled_owned())
                .await
        });
        Ok(Self {
            base_url,
            cancellation,
            task,
        })
    }

    pub async fn stop(self) -> Result<(), Box<dyn std::error::Error>> {
        self.cancellation.cancel();
        self.task.await??;
        Ok(())
    }
}

async fn forward(State(relay): State<Relay>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    let mutation = parts.method == axum::http::Method::POST;
    parts.headers.remove("host");
    let path = parts.uri.path();
    let body = axum::body::to_bytes(body, 65536)
        .await
        .unwrap_or_else(|error| panic!("relay body: {error}"));
    let response = relay
        .http
        .request(parts.method, format!("{}{}", relay.upstream, parts.uri))
        .headers(parts.headers)
        .body(body)
        .send()
        .await
        .unwrap_or_else(|_| panic!("relay request failed"));
    let status = response.status();
    assert!(status.is_success(), "upstream status {status} on {path}");
    let bytes = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("relay response failed"));
    let body = if mutation && relay.pending_loss.lock().await.remove(path) {
        Body::from("{")
    } else {
        Body::from(bytes)
    };
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body)
        .unwrap_or_else(|error| panic!("relay response: {error}"))
}
