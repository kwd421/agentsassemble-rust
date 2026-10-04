use std::{sync::Arc, time::Duration};

use agentsassemble_persistence::{PersistenceError, SqliteStore};
use chrono::Utc;
use futures_util::StreamExt;
use parking_lot::RwLock;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{CentralHostIdentity, HostIdentityError, public_ingress::PublicIngress};

const HEARTBEAT: Duration = Duration::from_mins(5);
const LEASE_SECONDS: i64 = 600;
const RESPONSE_LIMIT: usize = 32 * 1024;

#[derive(Clone)]
pub(crate) struct CentralDirectory(Option<Arc<CentralDirectoryInner>>);

struct CentralDirectoryInner {
    base_url: Url,
    client: Client,
    status: RwLock<CentralDirectoryStatus>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerConnectionResponse {
    status: String,
    connection_id: String,
    server_id: String,
    person_id: String,
    device_id: String,
    browser_fingerprint: String,
    origin: String,
    generation: i64,
    expires_at: i64,
    session_expires_at: i64,
    renew_at: i64,
}

#[derive(Clone, Serialize)]
pub(crate) struct CentralDirectoryStatus {
    pub(crate) enabled: bool,
    pub(crate) registered_origin: String,
    pub(crate) last_success_at: i64,
    pub(crate) last_error: String,
}

#[derive(Debug, Error)]
pub enum CentralDirectoryError {
    #[error("central directory is unavailable")]
    Disabled,
    #[error("central directory URL is invalid")]
    InvalidUrl,
    #[error("central directory client construction failed")]
    Client(#[source] reqwest::Error),
    #[error("central directory request failed")]
    Request(#[source] reqwest::Error),
    #[error("central directory rejected the request")]
    Rejected,
    #[error("central directory is temporarily unavailable")]
    Unavailable,
    #[error("central directory response is invalid")]
    InvalidResponse,
    #[error("host request signing failed")]
    Signing(#[from] HostIdentityError),
    #[error("central endpoint generation failed")]
    Persistence(#[from] PersistenceError),
}

impl CentralDirectory {
    pub(crate) fn disabled() -> Self {
        Self(None)
    }

    pub(crate) fn configured(value: &str) -> Result<Self, CentralDirectoryError> {
        let base_url = normalize_base_url(value)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(CentralDirectoryError::Client)?;
        Ok(Self(Some(Arc::new(CentralDirectoryInner {
            base_url,
            client,
            status: RwLock::new(CentralDirectoryStatus {
                enabled: true,
                registered_origin: String::new(),
                last_success_at: 0,
                last_error: String::new(),
            }),
        }))))
    }

    pub(crate) fn status(&self) -> CentralDirectoryStatus {
        self.0.as_ref().map_or(
            CentralDirectoryStatus {
                enabled: false,
                registered_origin: String::new(),
                last_success_at: 0,
                last_error: String::new(),
            },
            |inner| inner.status.read().clone(),
        )
    }

    pub(crate) async fn run(
        self,
        store: SqliteStore,
        ingress: PublicIngress,
        identity: CentralHostIdentity,
        cancellation: CancellationToken,
    ) -> Result<(), CentralDirectoryError> {
        let Some(inner) = self.0 else { return Ok(()) };
        let mut registered = String::new();
        let mut last_success: Option<std::time::Instant> = None;
        let mut failure_count = 0_u32;
        let mut next_attempt = std::time::Instant::now();
        loop {
            let ready = ingress.ready_snapshot().map(|ready| ready.public_url);
            let renewal_due = last_success.is_none_or(|at| at.elapsed() >= HEARTBEAT);
            let attempt_due = std::time::Instant::now() >= next_attempt;
            if let Some(origin) = ready {
                if (origin != registered || renewal_due) && attempt_due {
                    let renew = origin == registered
                        && last_success.is_some_and(|at| {
                            at.elapsed() < Duration::from_secs(LEASE_SECONDS.cast_unsigned())
                        });
                    match publish_online(&inner, &store, &identity, &origin, renew).await {
                        Ok(()) => {
                            registered.clone_from(&origin);
                            last_success = Some(std::time::Instant::now());
                            failure_count = 0;
                            next_attempt = std::time::Instant::now() + HEARTBEAT;
                            let mut status = inner.status.write();
                            status.registered_origin = origin;
                            status.last_success_at = Utc::now().timestamp();
                            status.last_error.clear();
                        }
                        Err(error) => {
                            failure_count = failure_count.saturating_add(1);
                            next_attempt = std::time::Instant::now() + retry_delay(failure_count);
                            inner.status.write().last_error = error.to_string();
                        }
                    }
                }
            } else if !registered.is_empty() && attempt_due {
                match publish_offline(&inner, &store, &identity).await {
                    Ok(()) => {
                        registered.clear();
                        last_success = None;
                        failure_count = 0;
                        next_attempt = std::time::Instant::now() + HEARTBEAT;
                        let mut status = inner.status.write();
                        status.registered_origin.clear();
                        status.last_success_at = Utc::now().timestamp();
                        status.last_error.clear();
                    }
                    Err(error) => {
                        failure_count = failure_count.saturating_add(1);
                        next_attempt = std::time::Instant::now() + retry_delay(failure_count);
                        inner.status.write().last_error = error.to_string();
                    }
                }
            }
            tokio::select! {
                () = cancellation.cancelled() => break,
                () = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
        if !registered.is_empty() {
            publish_offline(&inner, &store, &identity).await?;
        }
        Ok(())
    }

    pub(crate) async fn owner_connection(
        &self,
        identity: &CentralHostIdentity,
        field: &str,
        credential: &str,
        origin: &str,
        generation: i64,
        device: &[u8; 32],
    ) -> Result<agentsassemble_persistence::OwnerConnectionLease, CentralDirectoryError> {
        let inner = self.0.as_ref().ok_or(CentralDirectoryError::Disabled)?;
        let operation = if field == "grant_token" {
            "exchange"
        } else {
            "renew"
        };
        let path = format!(
            "/v1/servers/{}/owner-connections/{operation}",
            identity.server_id()
        );
        let body = serde_json::to_vec(&json!({ field: credential, "origin": origin,
            "generation": generation, "browser_fingerprint": hex::encode(device) }))
        .map_err(|_| CentralDirectoryError::InvalidResponse)?;
        let bytes = send_signed(inner, identity, Method::POST, &path, body).await?;
        let response: OwnerConnectionResponse =
            serde_json::from_slice(&bytes).map_err(|_| CentralDirectoryError::InvalidResponse)?;
        if response.status != "authorized"
            || response.server_id != identity.server_id()
            || response.origin != origin
            || response.generation != generation
            || response.browser_fingerprint != hex::encode(device)
        {
            return Err(CentralDirectoryError::InvalidResponse);
        }
        agentsassemble_persistence::OwnerConnectionLease::verified(
            agentsassemble_persistence::OwnerConnectionBinding {
                connection_id: response.connection_id,
                server_id: response.server_id,
                person_id: response.person_id,
                device_id: response.device_id,
                browser_fingerprint: *device,
                origin: response.origin,
                generation: response.generation,
                session_expires_at: response.session_expires_at,
            },
            response.expires_at,
            response.renew_at,
        )
        .map_err(|_| CentralDirectoryError::InvalidResponse)
    }
}

fn retry_delay(failure_count: u32) -> Duration {
    Duration::from_secs(2_u64.saturating_pow(failure_count.min(5)).min(60))
}

async fn publish_online(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
    origin: &str,
    renew: bool,
) -> Result<(), CentralDirectoryError> {
    let now = Utc::now().timestamp();
    let body = serde_json::to_vec(&json!({
        "origin": origin,
        "generation": if renew { store.current_central_endpoint_generation().await? } else { store.next_central_endpoint_generation().await? },
        "issued_at": now,
        "lease_expires_at": now + LEASE_SECONDS,
    }))
    .map_err(|_| CentralDirectoryError::InvalidResponse)?;
    let suffix = if renew { "/renew" } else { "" };
    let path = format!("/v1/servers/{}/endpoint{suffix}", identity.server_id());
    send_signed(
        inner,
        identity,
        if renew { Method::POST } else { Method::PUT },
        &path,
        body,
    )
    .await?;
    Ok(())
}

async fn publish_offline(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
) -> Result<(), CentralDirectoryError> {
    let body = serde_json::to_vec(&json!({
        "generation": store.next_central_endpoint_generation().await?,
        "issued_at": Utc::now().timestamp(),
    }))
    .map_err(|_| CentralDirectoryError::InvalidResponse)?;
    let path = format!("/v1/servers/{}/endpoint", identity.server_id());
    send_signed(inner, identity, Method::DELETE, &path, body).await?;
    Ok(())
}

async fn send_signed(
    inner: &CentralDirectoryInner,
    identity: &CentralHostIdentity,
    method: Method,
    path: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, CentralDirectoryError> {
    let signature = identity.sign_host_request(method.as_str(), path, &body)?;
    let endpoint = inner
        .base_url
        .join(path)
        .map_err(|_| CentralDirectoryError::InvalidUrl)?;
    let response = inner
        .client
        .request(method, endpoint)
        .header("content-type", "application/json")
        .header("x-aa-host-timestamp", signature.timestamp)
        .header("x-aa-host-nonce", signature.nonce)
        .header("x-aa-host-signature", signature.signature)
        .header("user-agent", "AgentsAssemble/central-directory-host-v1")
        .body(body)
        .send()
        .await
        .map_err(CentralDirectoryError::Request)?;
    if response.status() != StatusCode::OK {
        return Err(
            if response.status().is_server_error()
                || response.status() == StatusCode::TOO_MANY_REQUESTS
            {
                CentralDirectoryError::Unavailable
            } else {
                CentralDirectoryError::Rejected
            },
        );
    }
    if response
        .content_length()
        .is_some_and(|length| length > RESPONSE_LIMIT as u64)
    {
        return Err(CentralDirectoryError::InvalidResponse);
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(CentralDirectoryError::Request)?;
        if bytes.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(CentralDirectoryError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn normalize_base_url(value: &str) -> Result<Url, CentralDirectoryError> {
    let mut url = Url::parse(value.trim()).map_err(|_| CentralDirectoryError::InvalidUrl)?;
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
    {
        return Err(CentralDirectoryError::InvalidUrl);
    }
    url.set_path("");
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_url, retry_delay};

    #[test]
    fn central_url_rejects_credentials_paths_and_public_http() {
        assert!(normalize_base_url("https://central.example.test").is_ok());
        assert!(normalize_base_url("http://127.0.0.1:8787").is_ok());
        for invalid in [
            "http://central.example.test",
            "https://user:secret@central.example.test",
            "https://central.example.test/path",
            "https://central.example.test/?token=secret",
        ] {
            assert!(normalize_base_url(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn publication_retry_backoff_is_bounded() {
        assert_eq!(retry_delay(1), std::time::Duration::from_secs(2));
        assert_eq!(retry_delay(6), std::time::Duration::from_secs(32));
        assert_eq!(retry_delay(u32::MAX), std::time::Duration::from_secs(32));
    }
}
