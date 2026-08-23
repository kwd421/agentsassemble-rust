use std::{path::Path, time::Duration};

use reqwest::{Client, Method, Response, StatusCode, Url, redirect::Policy};
use serde_json::Value;
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum LoopbackHttpError {
    #[error("the loopback provider endpoint is invalid")]
    InvalidEndpoint,
    #[error("the loopback provider credentials are invalid")]
    InvalidCredentials,
    #[error("the loopback provider request failed")]
    Request,
    #[error("the loopback provider response exceeded its bound")]
    ResponseTooLarge,
    #[error("the loopback provider returned invalid JSON")]
    InvalidJson,
}

#[derive(Clone)]
pub(crate) struct LoopbackHttp {
    client: Client,
    endpoint: Url,
    directory: String,
    username: String,
    password: String,
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
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| LoopbackHttpError::Request)?;
        Ok(Self {
            client,
            endpoint,
            directory,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    pub(crate) async fn get_json(
        &self,
        path: &str,
        timeout: Duration,
    ) -> Result<JsonResponse, LoopbackHttpError> {
        self.json_request(Method::GET, path, None, timeout).await
    }

    pub(crate) async fn post_json(
        &self,
        path: &str,
        payload: &Value,
        timeout: Duration,
    ) -> Result<JsonResponse, LoopbackHttpError> {
        self.json_request(Method::POST, path, Some(payload), timeout)
            .await
    }

    pub(crate) async fn get_stream(
        &self,
        path: &str,
        timeout: Duration,
    ) -> Result<Response, LoopbackHttpError> {
        let response = self
            .request(Method::GET, path)?
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| LoopbackHttpError::Request)?;
        if !response.status().is_success() {
            return Err(LoopbackHttpError::Request);
        }
        Ok(response)
    }

    async fn json_request(
        &self,
        method: Method,
        path: &str,
        payload: Option<&Value>,
        timeout: Duration,
    ) -> Result<JsonResponse, LoopbackHttpError> {
        let mut request = self.request(method, path)?.timeout(timeout);
        if let Some(payload) = payload {
            request = request.json(payload);
        }
        let response = request
            .send()
            .await
            .map_err(|_| LoopbackHttpError::Request)?;
        let status = response.status();
        let mut response = response;
        let mut encoded = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| LoopbackHttpError::Request)?
        {
            if encoded.len().saturating_add(chunk.len()) > MAX_JSON_BYTES {
                return Err(LoopbackHttpError::ResponseTooLarge);
            }
            encoded.extend_from_slice(&chunk);
        }
        let value = if encoded.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&encoded).map_err(|_| LoopbackHttpError::InvalidJson)?
        };
        Ok(JsonResponse { status, value })
    }

    fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, LoopbackHttpError> {
        Ok(self
            .client
            .request(method, self.url(path)?)
            .basic_auth(&self.username, Some(&self.password)))
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
                    .any(|line| line.starts_with("authorization: Basic "));
                stream
                    .write_all(response)
                    .await
                    .unwrap_or_else(|error| panic!("write fixture response: {error}"));
            }
            authenticated
        });
        let client = client(&endpoint).unwrap_or_else(|error| panic!("create client: {error}"));
        let response = client
            .get_json("/global/health", std::time::Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("request fixture: {error}"));
        assert!(response.status.is_success());
        let stream = client
            .get_stream("/event", std::time::Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("stream fixture: {error}"));
        assert!(stream.status().is_success());
        assert!(
            observed
                .await
                .unwrap_or_else(|error| panic!("join fixture: {error}"))
        );
    }
}
