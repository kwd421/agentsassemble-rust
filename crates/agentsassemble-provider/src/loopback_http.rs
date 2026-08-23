use std::{net::SocketAddr, path::Path, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    Method, Request, StatusCode, Uri,
    body::Incoming,
    header::{ACCEPT, AUTHORIZATION, CONNECTION, CONTENT_TYPE, HOST, HeaderValue},
};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use thiserror::Error;
use tokio::{net::TcpStream, task::JoinHandle};
use url::{Position, Url};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum LoopbackHttpError {
    #[error("the loopback provider endpoint is invalid")]
    InvalidEndpoint,
    #[error("the loopback provider credentials are invalid")]
    InvalidCredentials,
    #[error("the loopback provider peer is not the owned runtime")]
    PeerNotOwned,
    #[error("the loopback provider request failed")]
    Request,
    #[error("the loopback provider response exceeded its bound")]
    ResponseTooLarge,
    #[error("the loopback provider returned invalid JSON")]
    InvalidJson,
}

#[derive(Clone)]
pub(crate) struct LoopbackHttp {
    endpoint: Url,
    address: SocketAddr,
    authority: HeaderValue,
    directory: String,
    authorization: HeaderValue,
}

pub(crate) struct UnverifiedLoopbackConnection {
    stream: TcpStream,
}

pub(crate) struct VerifiedLoopbackConnection {
    stream: TcpStream,
    http: LoopbackHttp,
}

pub(crate) struct LoopbackStream {
    body: Incoming,
    connection_task: JoinHandle<()>,
}

pub(crate) struct JsonResponse {
    pub(crate) status: StatusCode,
    pub(crate) value: Value,
}

impl LoopbackHttp {
    pub(crate) fn new(
        endpoint: &str,
        directory: &Path,
        username: &str,
        password: &str,
    ) -> Result<Self, LoopbackHttpError> {
        let endpoint = Url::parse(endpoint).map_err(|_| LoopbackHttpError::InvalidEndpoint)?;
        if endpoint.scheme() != "http"
            || endpoint.host_str() != Some("127.0.0.1")
            || endpoint.port().is_none()
            || endpoint.path() != "/"
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(LoopbackHttpError::InvalidEndpoint);
        }
        let port = endpoint.port().ok_or(LoopbackHttpError::InvalidEndpoint)?;
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let authority = HeaderValue::from_str(&format!("127.0.0.1:{port}"))
            .map_err(|_| LoopbackHttpError::InvalidEndpoint)?;
        let directory = directory
            .to_str()
            .filter(|value| !value.is_empty() && !value.contains('\0'))
            .ok_or(LoopbackHttpError::InvalidEndpoint)?
            .to_owned();
        if username.is_empty()
            || username.len() > 128
            || username.contains([':', '\r', '\n', '\0'])
            || password.len() < 32
            || password.len() > 1024
            || password.contains(['\r', '\n', '\0'])
        {
            return Err(LoopbackHttpError::InvalidCredentials);
        }
        let encoded = STANDARD.encode(format!("{username}:{password}"));
        let authorization = HeaderValue::from_str(&format!("Basic {encoded}"))
            .map_err(|_| LoopbackHttpError::InvalidCredentials)?;
        Ok(Self {
            endpoint,
            address,
            authority,
            directory,
            authorization,
        })
    }

    pub(crate) async fn connect(&self) -> Result<UnverifiedLoopbackConnection, LoopbackHttpError> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(self.address))
            .await
            .map_err(|_| LoopbackHttpError::Request)?
            .map_err(|_| LoopbackHttpError::Request)?;
        if stream.peer_addr().map_err(|_| LoopbackHttpError::Request)? != self.address {
            return Err(LoopbackHttpError::InvalidEndpoint);
        }
        Ok(UnverifiedLoopbackConnection { stream })
    }

    pub(crate) fn verify_peer(
        &self,
        connection: UnverifiedLoopbackConnection,
        exact_child_is_alive: bool,
    ) -> Result<VerifiedLoopbackConnection, LoopbackHttpError> {
        if !exact_child_is_alive {
            return Err(LoopbackHttpError::PeerNotOwned);
        }
        Ok(VerifiedLoopbackConnection {
            stream: connection.stream,
            http: self.clone(),
        })
    }

    fn url(&self, path: &str) -> Result<Url, LoopbackHttpError> {
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['\r', '\n', '\0']) {
            return Err(LoopbackHttpError::InvalidEndpoint);
        }
        let mut url = self
            .endpoint
            .join(path)
            .map_err(|_| LoopbackHttpError::InvalidEndpoint)?;
        if url.scheme() != self.endpoint.scheme()
            || url.host_str() != self.endpoint.host_str()
            || url.port() != self.endpoint.port()
        {
            return Err(LoopbackHttpError::InvalidEndpoint);
        }
        url.query_pairs_mut()
            .append_pair("directory", &self.directory);
        Ok(url)
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        payload: Option<&Value>,
        stream: bool,
    ) -> Result<Request<Full<Bytes>>, LoopbackHttpError> {
        let url = self.url(path)?;
        let uri = url[Position::BeforePath..]
            .parse::<Uri>()
            .map_err(|_| LoopbackHttpError::InvalidEndpoint)?;
        let body = payload
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| LoopbackHttpError::InvalidJson)?
            .unwrap_or_default();
        if body.len() > MAX_JSON_BYTES {
            return Err(LoopbackHttpError::ResponseTooLarge);
        }
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, self.authority.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONNECTION, "close");
        if payload.is_some() {
            builder = builder.header(CONTENT_TYPE, "application/json");
        }
        if stream {
            builder = builder.header(ACCEPT, "text/event-stream");
        }
        builder
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| LoopbackHttpError::Request)
    }
}

impl VerifiedLoopbackConnection {
    pub(crate) async fn get_json(
        self,
        path: &str,
        timeout: Duration,
    ) -> Result<JsonResponse, LoopbackHttpError> {
        self.json_request(Method::GET, path, None, timeout).await
    }

    pub(crate) async fn post_json(
        self,
        path: &str,
        payload: &Value,
        timeout: Duration,
    ) -> Result<JsonResponse, LoopbackHttpError> {
        self.json_request(Method::POST, path, Some(payload), timeout)
            .await
    }

    pub(crate) async fn get_stream(
        self,
        path: &str,
        timeout: Duration,
    ) -> Result<LoopbackStream, LoopbackHttpError> {
        let request = self.http.request(Method::GET, path, None, true)?;
        let (response, connection_task) = tokio::time::timeout(timeout, self.send(request))
            .await
            .map_err(|_| LoopbackHttpError::Request)??;
        if !response.status().is_success() {
            connection_task.abort();
            return Err(LoopbackHttpError::Request);
        }
        Ok(LoopbackStream {
            body: response.into_body(),
            connection_task,
        })
    }

    async fn json_request(
        self,
        method: Method,
        path: &str,
        payload: Option<&Value>,
        timeout: Duration,
    ) -> Result<JsonResponse, LoopbackHttpError> {
        let request = self.http.request(method, path, payload, false)?;
        tokio::time::timeout(timeout, async move {
            let (response, connection_task) = self.send(request).await?;
            let status = response.status();
            let mut body = response.into_body();
            let mut encoded = Vec::new();
            while let Some(frame) = body.frame().await {
                let frame = frame.map_err(|_| LoopbackHttpError::Request)?;
                if let Ok(chunk) = frame.into_data() {
                    if encoded.len().saturating_add(chunk.len()) > MAX_JSON_BYTES {
                        connection_task.abort();
                        return Err(LoopbackHttpError::ResponseTooLarge);
                    }
                    encoded.extend_from_slice(&chunk);
                }
            }
            connection_task.abort();
            let value = if encoded.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&encoded).map_err(|_| LoopbackHttpError::InvalidJson)?
            };
            Ok(JsonResponse { status, value })
        })
        .await
        .map_err(|_| LoopbackHttpError::Request)?
    }

    async fn send(
        self,
        request: Request<Full<Bytes>>,
    ) -> Result<(hyper::Response<Incoming>, JoinHandle<()>), LoopbackHttpError> {
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(self.stream))
                .await
                .map_err(|_| LoopbackHttpError::Request)?;
        let connection_task = tokio::spawn(async move {
            let _ = connection.await;
        });
        if let Ok(response) = sender.send_request(request).await {
            Ok((response, connection_task))
        } else {
            connection_task.abort();
            Err(LoopbackHttpError::Request)
        }
    }
}

impl LoopbackStream {
    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>, LoopbackHttpError> {
        loop {
            let Some(frame) = self.body.frame().await else {
                return Ok(None);
            };
            let frame = frame.map_err(|_| LoopbackHttpError::Request)?;
            if let Ok(data) = frame.into_data() {
                return Ok(Some(data));
            }
        }
    }
}

impl Drop for LoopbackStream {
    fn drop(&mut self) {
        self.connection_task.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::LoopbackHttp;

    fn client(endpoint: &str) -> Result<LoopbackHttp, super::LoopbackHttpError> {
        LoopbackHttp::new(
            endpoint,
            Path::new("/tmp"),
            "agentsassemble",
            &"x".repeat(64),
        )
    }

    #[test]
    fn endpoint_accepts_only_explicit_ipv4_loopback_authority() {
        assert!(client("http://127.0.0.1:3210/").is_ok());
        for endpoint in [
            "https://127.0.0.1:3210/",
            "http://localhost:3210/",
            "http://127.0.0.1/",
            "http://127.0.0.1:3210/base",
        ] {
            assert!(client(endpoint).is_err());
        }
    }

    #[test]
    fn paths_cannot_replace_the_verified_loopback_authority() {
        let client = client("http://127.0.0.1:3210/")
            .unwrap_or_else(|error| panic!("create loopback client: {error}"));
        assert!(client.url("/session").is_ok());
        assert!(client.url("//example.com/session").is_err());
    }

    #[tokio::test]
    async fn unowned_connected_peer_receives_no_http_or_credentials() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("bind fixture: {error}"));
        let endpoint = format!(
            "http://127.0.0.1:{}/",
            listener
                .local_addr()
                .unwrap_or_else(|error| panic!("fixture address: {error}"))
                .port()
        );
        let observed = tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .unwrap_or_else(|error| panic!("accept fixture: {error}"));
            let mut encoded = Vec::new();
            stream
                .read_to_end(&mut encoded)
                .await
                .unwrap_or_else(|error| panic!("read fixture: {error}"));
            encoded
        });
        let client = client(&endpoint).unwrap_or_else(|error| panic!("create client: {error}"));
        let connected = client
            .connect()
            .await
            .unwrap_or_else(|error| panic!("connect fixture: {error}"));
        assert!(client.verify_peer(connected, false).is_err());
        assert!(
            observed
                .await
                .unwrap_or_else(|error| panic!("join fixture: {error}"))
                .is_empty()
        );
    }

    #[tokio::test]
    async fn json_and_stream_requests_carry_the_private_basic_capability() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("bind fixture: {error}"));
        let endpoint = format!(
            "http://127.0.0.1:{}/",
            listener
                .local_addr()
                .unwrap_or_else(|error| panic!("fixture address: {error}"))
                .port()
        );
        let observed = tokio::spawn(async move {
            let mut authenticated = true;
            for response in [
                &b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"healthy\":true}"[..],
                &b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
            ] {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .unwrap_or_else(|error| panic!("accept fixture request: {error}"));
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while request.len() < 16 * 1024 && !request.ends_with(b"\r\n\r\n") {
                    let count = stream
                        .read(&mut byte)
                        .await
                        .unwrap_or_else(|error| panic!("read fixture request: {error}"));
                    if count == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                authenticated &= String::from_utf8_lossy(&request)
                    .lines()
                    .any(|line| line.to_ascii_lowercase().starts_with("authorization: basic "));
                stream
                    .write_all(response)
                    .await
                    .unwrap_or_else(|error| panic!("write fixture response: {error}"));
            }
            authenticated
        });
        let client = client(&endpoint).unwrap_or_else(|error| panic!("create client: {error}"));
        let response = client
            .verify_peer(
                client
                    .connect()
                    .await
                    .unwrap_or_else(|error| panic!("connect JSON fixture: {error}")),
                true,
            )
            .unwrap_or_else(|error| panic!("verify JSON fixture: {error}"))
            .get_json("/global/health", std::time::Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("request fixture: {error}"));
        assert!(response.status.is_success());
        let stream = client
            .verify_peer(
                client
                    .connect()
                    .await
                    .unwrap_or_else(|error| panic!("connect SSE fixture: {error}")),
                true,
            )
            .unwrap_or_else(|error| panic!("verify SSE fixture: {error}"))
            .get_stream("/event", std::time::Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("stream fixture: {error}"));
        drop(stream);
        assert!(
            observed
                .await
                .unwrap_or_else(|error| panic!("join fixture: {error}"))
        );
    }
}
