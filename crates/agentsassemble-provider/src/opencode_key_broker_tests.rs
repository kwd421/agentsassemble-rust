use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpListener, sync::mpsc};

use super::{KeyBroker, Upstream};

const SECRET: &str = "real-provider-secret";
const MODEL: &str = "gemini-3.1-flash-lite";

/// A stand-in provider host that records what reached it.
async fn provider_fixture() -> (
    String,
    mpsc::Receiver<(String, Option<String>, Option<String>)>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind provider fixture: {error}"));
    let host = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|error| panic!("fixture address: {error}"))
    );
    let (sender, receiver) = mpsc::channel(4);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let sender = sender.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let sender = sender.clone();
                    async move {
                        let header = |name: &str| {
                            request
                                .headers()
                                .get(name)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_owned)
                        };
                        let record = (
                            request.uri().to_string(),
                            header("x-goog-api-key"),
                            header("authorization"),
                        );
                        let _ = sender.send(record).await;
                        Ok::<_, Infallible>(
                            Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Full::new(Bytes::from_static(b"data: ok\n\n")))
                                .unwrap_or_else(|error| panic!("fixture response: {error}")),
                        )
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (host, receiver)
}

async fn broker(upstream: Upstream, host: String) -> KeyBroker {
    KeyBroker::start_with(
        upstream,
        MODEL.to_owned(),
        SECRET.to_owned(),
        host,
        reqwest::Client::new(),
    )
    .await
    .unwrap_or_else(|error| panic!("start broker: {error}"))
}

#[tokio::test]
async fn only_the_session_token_and_model_reach_the_provider_with_the_real_key() {
    let (host, mut seen) = provider_fixture().await;
    let broker = broker(Upstream::Gemini, host).await;
    let base = format!("http://127.0.0.1:{}", broker.port);
    let client = reqwest::Client::new();
    let stream_path = format!("{base}/v1beta/models/{MODEL}:streamGenerateContent?alt=sse");

    let wrong = client
        .post(&stream_path)
        .header("x-goog-api-key", "guessed")
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("wrong token request: {error}"));
    assert_eq!(wrong.status(), 401);

    let other_model = client
        .post(format!(
            "{base}/v1beta/models/gemini-3.5-pro:streamGenerateContent?alt=sse"
        ))
        .header("x-goog-api-key", &broker.token)
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("other model request: {error}"));
    assert_eq!(other_model.status(), 404);

    let allowed = client
        .post(&stream_path)
        .header("x-goog-api-key", &broker.token)
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("allowed request: {error}"));
    assert_eq!(allowed.status(), 200);
    assert_eq!(
        allowed
            .bytes()
            .await
            .unwrap_or_else(|error| panic!("streamed body: {error}")),
        Bytes::from_static(b"data: ok\n\n")
    );
    let (uri, google_key, _) = seen
        .recv()
        .await
        .unwrap_or_else(|| panic!("provider saw the forwarded request"));
    assert_eq!(
        uri,
        format!("/v1beta/models/{MODEL}:streamGenerateContent?alt=sse")
    );
    assert_eq!(google_key.as_deref(), Some(SECRET));
    assert!(
        seen.try_recv().is_err(),
        "rejected requests never reach the provider"
    );
}

#[tokio::test]
async fn deepseek_takes_a_bearer_token_and_swaps_in_the_real_key() {
    let (host, mut seen) = provider_fixture().await;
    let broker = broker(Upstream::DeepSeek, host).await;
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/chat/completions", broker.port))
        .bearer_auth(&broker.token)
        .body("{}")
        .send()
        .await
        .unwrap_or_else(|error| panic!("deepseek request: {error}"));
    assert_eq!(response.status(), 200);
    let (_, _, authorization) = seen
        .recv()
        .await
        .unwrap_or_else(|| panic!("provider saw the forwarded request"));
    assert_eq!(authorization, Some(format!("Bearer {SECRET}")));
}

#[tokio::test]
async fn opencode_config_names_the_broker_and_never_the_secret() {
    let broker = broker(Upstream::Gemini, "http://127.0.0.1:9".to_owned()).await;
    let config: serde_json::Value = serde_json::from_str(&broker.config_content())
        .unwrap_or_else(|error| panic!("config JSON: {error}"));
    assert_eq!(
        config["provider"]["google"]["options"]["baseURL"],
        format!("http://127.0.0.1:{}/v1beta", broker.port)
    );
    assert_eq!(
        config["provider"]["google"]["options"]["apiKey"],
        broker.token
    );
    assert_eq!(config["small_model"], format!("google/{MODEL}"));
    assert!(!broker.config_content().contains(SECRET));
}
