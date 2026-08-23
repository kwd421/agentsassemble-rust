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
}

pub(crate) struct JsonResponse {
    pub(crate) status: StatusCode,
    pub(crate) value: Value,
}

impl LoopbackHttp {
    pub(crate) fn new(endpoint: &str, directory: &Path) -> Result<Self, LoopbackHttpError> {
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
            .client
            .get(self.url(path)?)
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
        let mut request = self
            .client
            .request(method, self.url(path)?)
            .timeout(timeout);
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

    use super::LoopbackHttp;

    #[test]
    fn endpoint_accepts_only_explicit_ipv4_loopback_authority() {
        assert!(LoopbackHttp::new("http://127.0.0.1:3210/", Path::new("/tmp")).is_ok());
        for endpoint in [
            "https://127.0.0.1:3210/",
            "http://localhost:3210/",
            "http://127.0.0.1/",
            "http://127.0.0.1:3210/base",
        ] {
            assert!(LoopbackHttp::new(endpoint, Path::new("/tmp")).is_err());
        }
    }

    #[test]
    fn paths_cannot_replace_the_verified_loopback_authority() {
        let client = LoopbackHttp::new("http://127.0.0.1:3210/", Path::new("/tmp"))
            .unwrap_or_else(|error| panic!("create loopback client: {error}"));
        assert!(client.url("/session").is_ok());
        assert!(client.url("//example.com/session").is_err());
    }
}
