//! Loopback key broker for API agents that run inside OpenCode.
//!
//! The saved API key never enters the OpenCode process: OpenCode's shell tool
//! inherits its whole environment, so anything placed there is readable by a
//! command the model runs. OpenCode instead talks to this broker on 127.0.0.1
//! with a per-launch token; the broker checks the token, allows only the
//! session's own model endpoint, swaps in the real key and forwards to the fixed
//! provider host. A leaked token works only through this broker, only for that
//! model and only while the agent runs.

use std::convert::Infallible;

use bytes::Bytes;
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Full, Limited, StreamBody, combinators::BoxBody};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Frame, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::driver::DriverError;

const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Upstream {
    Gemini,
    DeepSeek,
}

impl Upstream {
    pub(crate) fn for_provider(provider_kind: &str) -> Option<Self> {
        match provider_kind {
            "gemini_api" => Some(Self::Gemini),
            "deepseek_api" => Some(Self::DeepSeek),
            _ => None,
        }
    }

    const fn host(self) -> &'static str {
        match self {
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::DeepSeek => "https://api.deepseek.com",
        }
    }

    const fn opencode_provider(self) -> &'static str {
        match self {
            Self::Gemini => "google",
            Self::DeepSeek => "deepseek",
        }
    }

    /// Paths OpenCode may reach for this session's model, and nothing else.
    fn allows(self, model: &str, path: &str, query: Option<&str>) -> bool {
        match self {
            Self::Gemini => {
                let stream = format!("/v1beta/models/{model}:streamGenerateContent");
                let unary = format!("/v1beta/models/{model}:generateContent");
                (path == stream && matches!(query, None | Some("alt=sse")))
                    || (path == unary && query.is_none())
            }
            Self::DeepSeek => {
                matches!(path, "/chat/completions" | "/v1/chat/completions") && query.is_none()
            }
        }
    }

    fn presented_token<B>(self, request: &Request<B>) -> Option<&[u8]> {
        let headers = request.headers();
        match self {
            Self::Gemini => headers.get("x-goog-api-key").map(|value| value.as_bytes()),
            Self::DeepSeek => headers
                .get(hyper::header::AUTHORIZATION)
                .and_then(|value| value.as_bytes().strip_prefix(b"Bearer ")),
        }
    }

    fn authorize(self, request: reqwest::RequestBuilder, secret: &str) -> reqwest::RequestBuilder {
        match self {
            Self::Gemini => request.header("x-goog-api-key", secret),
            Self::DeepSeek => request.bearer_auth(secret),
        }
    }
}

pub(crate) struct KeyBroker {
    port: u16,
    token: String,
    upstream: Upstream,
    model: String,
    shutdown: CancellationToken,
    task: JoinHandle<()>,
}

impl KeyBroker {
    pub(crate) async fn start(
        upstream: Upstream,
        model: String,
        secret: String,
    ) -> Result<Self, DriverError> {
        let client = crate::remote_https::fixed_endpoint_client().map_err(|_| broker_error())?;
        Self::start_with(upstream, model, secret, upstream.host().to_owned(), client).await
    }

    async fn start_with(
        upstream: Upstream,
        model: String,
        secret: String,
        host: String,
        client: reqwest::Client,
    ) -> Result<Self, DriverError> {
        if secret.is_empty() || secret.contains(['\0', '\r', '\n']) {
            return Err(DriverError::new(
                "provider_credential_invalid",
                "The saved API credential cannot be used by OpenCode.",
            ));
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| broker_error())?;
        let port = listener.local_addr().map_err(|_| broker_error())?.port();
        let token = crate::opencode_startup::server_password();
        let shutdown = CancellationToken::new();
        let state = std::sync::Arc::new(BrokerState {
            upstream,
            model: model.clone(),
            token: token.clone(),
            secret,
            host,
            client,
        });
        let stop = shutdown.clone();
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    () = stop.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((stream, _)) = accepted else { continue };
                let state = state.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        let state = state.clone();
                        async move { Ok::<_, Infallible>(state.handle(request).await) }
                    });
                    let connection =
                        http1::Builder::new().serve_connection(TokioIo::new(stream), service);
                    tokio::select! {
                        () = stop.cancelled() => {}
                        _ = connection => {}
                    }
                });
            }
        });
        Ok(Self {
            port,
            token,
            upstream,
            model,
            shutdown,
            task,
        })
    }

    /// OpenCode configuration that points this provider at the broker. The main
    /// and small model are both the session's model, the only one the broker serves.
    pub(crate) fn config_content(&self) -> String {
        let provider = self.upstream.opencode_provider();
        let base_url = match self.upstream {
            Upstream::Gemini => format!("http://127.0.0.1:{}/v1beta", self.port),
            Upstream::DeepSeek => format!("http://127.0.0.1:{}", self.port),
        };
        json!({
            "plugin": [],
            "mcp": {},
            "provider": { provider: { "options": { "baseURL": base_url, "apiKey": self.token } } },
            "small_model": format!("{provider}/{}", self.model),
        })
        .to_string()
    }
}

impl Drop for KeyBroker {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.task.abort();
    }
}

struct BrokerState {
    upstream: Upstream,
    model: String,
    token: String,
    secret: String,
    host: String,
    client: reqwest::Client,
}

type BrokerBody = BoxBody<Bytes, std::io::Error>;

impl BrokerState {
    async fn handle(&self, request: Request<Incoming>) -> Response<BrokerBody> {
        let presented = self.upstream.presented_token(&request).unwrap_or_default();
        if !bool::from(presented.ct_eq(self.token.as_bytes())) {
            return status(StatusCode::UNAUTHORIZED);
        }
        if request.method() != Method::POST {
            return status(StatusCode::METHOD_NOT_ALLOWED);
        }
        let path = request.uri().path().to_owned();
        let query = request.uri().query().map(str::to_owned);
        if !self.upstream.allows(&self.model, &path, query.as_deref()) {
            return status(StatusCode::NOT_FOUND);
        }
        let Ok(body) = Limited::new(request.into_body(), MAX_REQUEST_BYTES)
            .collect()
            .await
            .map(http_body_util::Collected::to_bytes)
        else {
            return status(StatusCode::PAYLOAD_TOO_LARGE);
        };
        let target = match &query {
            Some(query) => format!("{}{path}?{query}", self.host),
            None => format!("{}{path}", self.host),
        };
        let forwarded = self
            .upstream
            .authorize(self.client.post(target), &self.secret)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await;
        let Ok(upstream) = forwarded else {
            return status(StatusCode::BAD_GATEWAY);
        };
        let mut response = Response::builder().status(upstream.status().as_u16());
        if let Some(content_type) = upstream.headers().get(reqwest::header::CONTENT_TYPE) {
            response = response.header(hyper::header::CONTENT_TYPE, content_type.as_bytes());
        }
        let stream = upstream
            .bytes_stream()
            .map_ok(Frame::data)
            .map_err(std::io::Error::other);
        response
            .body(BodyExt::boxed(StreamBody::new(stream)))
            .unwrap_or_else(|_| status(StatusCode::BAD_GATEWAY))
    }
}

fn status(code: StatusCode) -> Response<BrokerBody> {
    let mut response = Response::new(
        Full::new(Bytes::new())
            .map_err(|never: Infallible| match never {})
            .boxed(),
    );
    *response.status_mut() = code;
    response
}

const fn broker_error() -> DriverError {
    DriverError::new(
        "provider_launch_failed",
        "The OpenCode key broker could not start.",
    )
}

#[cfg(test)]
#[path = "opencode_key_broker_tests.rs"]
mod tests;
