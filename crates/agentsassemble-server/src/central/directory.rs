use std::{sync::Arc, time::Duration};

use agentsassemble_persistence::{PersistenceError, SqliteStore};
use chrono::Utc;
use futures_util::StreamExt;
use parking_lot::RwLock;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{CentralHostIdentity, HostIdentityError, public_ingress::PublicIngress};

#[path = "admission_redeem.rs"]
mod admission_redeem;
#[path = "event_publisher.rs"]
mod event_publisher;
#[path = "member_sync.rs"]
mod member_sync;
pub(crate) use admission_redeem::MemberGrantPurpose;

#[cfg(test)]
#[path = "directory_terminal_tests.rs"]
mod terminal_tests;

const RESPONSE_LIMIT: usize = 32 * 1024;

#[derive(Clone)]
pub(crate) struct CentralDirectory(Option<Arc<CentralDirectoryInner>>);

struct CentralDirectoryInner {
    base_url: Url,
    client: Client,
    status: RwLock<CentralDirectoryStatus>,
    ingress: RwLock<PublicIngress>,
    #[cfg(test)]
    publisher_probe: tokio::sync::watch::Sender<(u64, bool)>,
}

#[derive(Clone, Serialize)]
pub(crate) struct CentralDirectoryStatus {
    pub(crate) enabled: bool,
    pub(crate) registered_origin: String,
    pub(crate) last_success_at: i64,
    pub(crate) last_error: String,
    pub(crate) name_sync_error: String,
}

impl CentralDirectoryStatus {
    fn published(&mut self, origin: &str) {
        origin.clone_into(&mut self.registered_origin);
        self.last_success_at = Utc::now().timestamp();
        self.last_error.clear();
    }
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
    #[error("server_retired")]
    ServerRetired,
    #[error("registration_absent")]
    RegistrationAbsent,
    #[error("public ingress demotion failed")]
    Demotion(#[from] crate::PublicIngressControlError),
    #[error("central directory is temporarily unavailable")]
    Unavailable,
    #[error("central directory request capacity is temporarily exhausted")]
    CapacityExhausted,
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
            #[cfg(test)]
            publisher_probe: tokio::sync::watch::channel((0, false)).0,
            base_url,
            client,
            ingress: RwLock::new(PublicIngress::disabled()),
            status: RwLock::new(CentralDirectoryStatus {
                enabled: true,
                registered_origin: String::new(),
                last_success_at: 0,
                last_error: String::new(),
                name_sync_error: String::new(),
            }),
        }))))
    }

    pub(crate) fn bind_ingress(&self, ingress: PublicIngress) {
        if let Some(inner) = &self.0 {
            *inner.ingress.write() = ingress;
        }
    }

    pub(crate) fn status(&self) -> CentralDirectoryStatus {
        self.0.as_ref().map_or(
            CentralDirectoryStatus {
                enabled: false,
                registered_origin: String::new(),
                last_success_at: 0,
                last_error: String::new(),
                name_sync_error: String::new(),
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
        self.bind_ingress(ingress.clone());
        let Some(inner) = self.0 else { return Ok(()) };
        event_publisher::run(inner, store, ingress, identity, cancellation).await
    }
}

fn retry_delay(failure_count: u32) -> Duration {
    Duration::from_secs(2_u64.saturating_pow(failure_count.min(5)).min(60))
}

// The existing directory task is the sole delivery owner. Profiles are durable;
// notifications coalesce edits, and retries/restarts read the latest revision.
async fn publish_default_name(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
    published: Option<&(String, String)>,
    parked: &mut Option<(String, i64)>,
) -> Result<Option<(String, String)>, CentralDirectoryError> {
    let Some(epoch) = store.registration_epoch().await? else {
        return Ok(None);
    };
    let profile = store.local_operator_profile().await?;
    let revision = (epoch.clone(), profile.revision);
    if parked.as_ref() == Some(&revision) {
        return Err(CentralDirectoryError::Rejected);
    }
    let name = super::host_identity::default_server_name(&profile.display_name).await;
    let version = (epoch, name.clone());
    if published != Some(&version) {
        let path = format!("/v1/servers/{}/name", identity.server_id());
        send_signed(
            inner,
            identity,
            store,
            Method::PUT,
            &path,
            json!({"name": name, "name_revision": profile.revision}),
        )
        .await
        .inspect_err(|error| {
            if matches!(error, CentralDirectoryError::Rejected) {
                *parked = Some(revision);
            }
        })?;
    }
    *parked = None;
    Ok(Some(version))
}

async fn send_signed(
    inner: &CentralDirectoryInner,
    identity: &CentralHostIdentity,
    store: &SqliteStore,
    method: Method,
    path: &str,
    mut body: serde_json::Value,
) -> Result<Vec<u8>, CentralDirectoryError> {
    let ingress = inner.ingress.read().clone();
    if reconcile_demotion(store, &ingress).await? {
        return Err(CentralDirectoryError::ServerRetired);
    }
    if body.get("registration_epoch").is_none()
        && let Some(epoch) = store.registration_epoch().await?
    {
        body["registration_epoch"] = epoch.into();
    }
    let body = serde_json::to_vec(&body).map_err(|_| CentralDirectoryError::InvalidResponse)?;
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
    let status = response.status();
    if status != StatusCode::OK && status != StatusCode::GONE {
        return Err(if response.status() == StatusCode::TOO_MANY_REQUESTS {
            CentralDirectoryError::CapacityExhausted
        } else if response.status().is_server_error()
            || response.status() == StatusCode::REQUEST_TIMEOUT
        {
            CentralDirectoryError::Unavailable
        } else {
            CentralDirectoryError::Rejected
        });
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
    if status == StatusCode::GONE {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| CentralDirectoryError::Rejected)?;
        let epoch = store.registration_epoch().await?;
        let code = value["error"]["code"].as_str();
        let terminal = match code {
            Some("server_retired") => CentralDirectoryError::ServerRetired,
            Some("registration_absent") if epoch.is_some() => {
                CentralDirectoryError::RegistrationAbsent
            }
            _ => return Err(CentralDirectoryError::Rejected),
        };
        if value["error"]["registration_epoch"]
            .as_str()
            .is_none_or(str::is_empty)
            || value["error"]["server_id"].as_str() != Some(identity.server_id())
            || epoch
                .as_deref()
                .is_some_and(|epoch| value["error"]["registration_epoch"].as_str() != Some(epoch))
        {
            return Err(CentralDirectoryError::InvalidResponse);
        }
        let ingress = inner.ingress.read().clone();
        demote_host(store, &ingress, true).await?;
        inner.status.write().registered_origin.clear();
        return Err(terminal);
    }
    Ok(bytes)
}

async fn reconcile_demotion(
    store: &SqliteStore,
    ingress: &PublicIngress,
) -> Result<bool, CentralDirectoryError> {
    if let Some(retired) = ingress.pending_demotion() {
        demote_host(store, ingress, retired).await?;
    }
    if store.hosting_restriction().await?.is_some() {
        ingress.demote().await?;
        return Ok(true);
    }
    Ok(ingress.restricted())
}

pub(crate) async fn demote_host(
    store: &SqliteStore,
    ingress: &PublicIngress,
    retired: bool,
) -> Result<(), CentralDirectoryError> {
    ingress.retain_demotion(retired);
    let reason = ingress.pending_demotion().unwrap_or(retired);
    let persisted = store.restrict_hosting(reason).await;
    if persisted.is_ok() {
        ingress.confirm_demotion(reason);
    } else {
        ingress.demotion_failure().cancel();
    }
    let stopped = ingress.demote().await;
    persisted?;
    stopped.map_err(CentralDirectoryError::Demotion)
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

    #[tokio::test]
    async fn host_routes_sign_the_stored_epoch_and_omit_it_before_upgrade() {
        use axum::{
            Router,
            body::Bytes,
            http::{HeaderMap, Method, Uri},
            routing::any,
        };
        let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|e| panic!("store: {e}"));
        let persistent = store
            .host_identity()
            .await
            .unwrap_or_else(|e| panic!("identity: {e}"));
        let identity = crate::CentralHostIdentity::from_persistent(&persistent)
            .unwrap_or_else(|e| panic!("key: {e}"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().route(
            "/{*path}",
            any(
                move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
                    let tx = tx.clone();
                    async move {
                        tx.send((method, uri, headers, body))
                            .unwrap_or_else(|_| panic!("capture"));
                        axum::Json(serde_json::json!({"status": "ok"}))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|e| panic!("bind: {e}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|e| panic!("address: {e}"));
        let cancellation = tokio_util::sync::CancellationToken::new();
        let shutdown = cancellation.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap_or_else(|e| panic!("serve: {e}"));
        });
        let directory = super::CentralDirectory::configured(&format!("http://{address}"))
            .unwrap_or_else(|e| panic!("directory: {e}"));
        let inner = directory.0.as_ref().unwrap_or_else(|| panic!("configured"));
        for epoch in [None, Some("opaque-registration-epoch")] {
            store
                .set_registration_epoch(epoch)
                .await
                .unwrap_or_else(|e| panic!("epoch: {e}"));
            for method in [reqwest::Method::PUT, reqwest::Method::DELETE] {
                super::send_signed(
                    inner,
                    &identity,
                    &store,
                    method,
                    &format!("/v1/servers/{}/endpoint", identity.server_id()),
                    serde_json::json!({"generation":1}),
                )
                .await
                .unwrap_or_else(|e| panic!("signed publication: {e}"));
            }
            assert!(
                directory
                    .owner_admission(
                        &identity,
                        &store,
                        "grant",
                        "https://host.example",
                        1,
                        &[0; 32],
                        None,
                    )
                    .await
                    .is_err()
            );
            for (method, suffix) in [
                ("PUT", "endpoint"),
                ("DELETE", "endpoint"),
                ("POST", "connect-grants/redeem"),
            ] {
                let (actual, uri, headers, body) =
                    rx.recv().await.unwrap_or_else(|| panic!("request"));
                assert_eq!(actual.as_str(), method);
                assert!(uri.path().ends_with(suffix));
                verify_epoch_request(&identity, &headers, &body, method, uri.path(), epoch);
            }
        }
        cancellation.cancel();
        task.await.unwrap_or_else(|e| panic!("join: {e}"));
    }

    #[tokio::test]
    async fn profile_changes_publish_default_names_without_public_ingress()
    -> Result<(), Box<dyn std::error::Error>> {
        use axum::{Router, body::Bytes, http::HeaderMap, routing::put};
        let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:").await?;
        store
            .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "First Profile")
            .await?;
        let persistent = store.host_identity().await?;
        let identity = crate::CentralHostIdentity::from_persistent(&persistent)?;
        let path = format!("/v1/servers/{}/name", identity.server_id());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let verify_identity = identity.clone();
        let verify_path = path.clone();
        let router = Router::new()
            .route(
                "/v1/servers/{id}/endpoint",
                axum::routing::delete(|| async { axum::Json(serde_json::json!({"status":"ok"})) }),
            )
            .route(
                &path,
                put(move |headers: HeaderMap, body: Bytes| {
                    let (tx, identity, path) =
                        (tx.clone(), verify_identity.clone(), verify_path.clone());
                    async move {
                        verify_epoch_request(
                            &identity,
                            &headers,
                            &body,
                            "PUT",
                            &path,
                            Some("name-epoch"),
                        );
                        let value: serde_json::Value = serde_json::from_slice(&body)
                            .unwrap_or_else(|error| panic!("name body: {error}"));
                        tx.send(value)
                            .unwrap_or_else(|error| panic!("capture: {error}"));
                        axum::Json(serde_json::json!({"status": "ok"}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let cancellation = tokio_util::sync::CancellationToken::new();
        let server_cancellation = tokio_util::sync::CancellationToken::new();
        let shutdown = server_cancellation.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap_or_else(|error| panic!("serve: {error}"));
        });
        let directory = super::CentralDirectory::configured(&format!("http://{address}"))?;
        let host = tokio::spawn(directory.run(
            store.clone(),
            crate::public_ingress::PublicIngress::disabled(),
            identity,
            cancellation.clone(),
        ));
        store.set_registration_epoch(Some("name-epoch")).await?;
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await?
            .ok_or("capture closed")?;
        assert_eq!(first["name_revision"], 1);
        assert!(
            first["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("First Profile의 "))
        );
        store
            .update_local_operator_profile(
                1,
                agentsassemble_domain::UserProfilePatch {
                    display_name: Some("Changed Profile".to_owned()),
                    ..Default::default()
                },
            )
            .await?;
        let changed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await?
            .ok_or("capture closed")?;
        assert_eq!(changed["name_revision"], 2);
        assert_eq!(
            changed["name"],
            first["name"].as_str().unwrap_or_default().replacen(
                "First Profile",
                "Changed Profile",
                1
            )
        );
        cancellation.cancel();
        host.await??;
        server_cancellation.cancel();
        server.await?;
        Ok(())
    }

    #[tokio::test]
    async fn name_delivery_skips_unrelated_changes_and_parks_permanent_rejections()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::publish_default_name as publish;
        use agentsassemble_domain::UserProfilePatch;
        use axum::{Router, body::Bytes, http::StatusCode, routing::put};
        use std::sync::{
            Arc,
            atomic::{AtomicU16, Ordering},
        };
        let store = agentsassemble_persistence::SqliteStore::open("sqlite::memory:").await?;
        store
            .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "Profile")
            .await?;
        store.set_registration_epoch(Some("name-epoch")).await?;
        let identity = crate::CentralHostIdentity::from_persistent(&store.host_identity().await?)?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let status = Arc::new(AtomicU16::new(200));
        let response_status = status.clone();
        let router = Router::new().route(
            "/{*path}",
            put(move |body: Bytes| {
                let (tx, status) = (tx.clone(), response_status.clone());
                async move {
                    tx.send(serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default())
                        .ok();
                    StatusCode::from_u16(status.load(Ordering::SeqCst))
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let directory =
            super::CentralDirectory::configured(&format!("http://{}", listener.local_addr()?))?;
        let cancellation = tokio_util::sync::CancellationToken::new();
        let shutdown = cancellation.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        let inner = directory.0.as_ref().ok_or("configured directory")?;
        let mut parked = None;
        let mut sent = publish(inner, &store, &identity, None, &mut parked).await?;
        assert_eq!(rx.try_recv()?["name_revision"], 1);
        store
            .update_local_operator_profile(
                1,
                UserProfilePatch {
                    avatar_label: Some("AV".into()),
                    status: Some("idle".into()),
                    mic_muted: Some(false),
                    deafened: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        sent = publish(inner, &store, &identity, sent.as_ref(), &mut parked).await?;
        assert!(rx.try_recv().is_err()); // Unrelated edits emit no PUT.
        // Each changed name sends its current revision; conflicts park, and
        // another name revision unparks exactly once.
        for (revision, name, code) in [
            (2, "Changed", 200),
            (3, "Conflict", 409),
            (4, "Corrected", 409),
        ] {
            store
                .update_local_operator_profile(
                    revision,
                    UserProfilePatch {
                        display_name: Some(name.into()),
                        ..Default::default()
                    },
                )
                .await?;
            status.store(code, Ordering::SeqCst);
            let result = publish(inner, &store, &identity, sent.as_ref(), &mut parked).await;
            assert_eq!(rx.try_recv()?["name_revision"], revision + 1);
            if code == 200 {
                sent = result?;
            } else {
                assert!(result.is_err());
                for _ in 0..3 {
                    assert!(
                        publish(inner, &store, &identity, sent.as_ref(), &mut parked)
                            .await
                            .is_err()
                    );
                    assert!(rx.try_recv().is_err()); // No additional nonce/budget use.
                }
            }
        }
        store
            .set_registration_epoch(Some("replacement-epoch"))
            .await?;
        // Rate limiting, timeout and server failure remain retryable.
        for code in [429, 408, 503, 200] {
            status.store(code, Ordering::SeqCst);
            let result = publish(inner, &store, &identity, sent.as_ref(), &mut parked).await;
            assert_eq!(result.is_ok(), code == 200);
            assert_eq!(rx.try_recv()?["registration_epoch"], "replacement-epoch");
        }
        cancellation.cancel();
        server.await??;
        Ok(())
    }

    fn verify_epoch_request(
        identity: &crate::CentralHostIdentity,
        headers: &axum::http::HeaderMap,
        body: &[u8],
        method: &str,
        path: &str,
        epoch: Option<&str>,
    ) {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use sha2::{Digest, Sha256};
        let value: serde_json::Value =
            serde_json::from_slice(body).unwrap_or_else(|e| panic!("body: {e}"));
        assert_eq!(
            value
                .get("registration_epoch")
                .and_then(serde_json::Value::as_str),
            epoch
        );
        let canonical = format!(
            "AA-HOST-1\n{method}\n{}\n{}\n{}\n{}",
            path,
            headers["x-aa-host-timestamp"].to_str().unwrap_or_default(),
            headers["x-aa-host-nonce"].to_str().unwrap_or_default(),
            URL_SAFE_NO_PAD.encode(Sha256::digest(body))
        );
        let signature = URL_SAFE_NO_PAD
            .decode(&headers["x-aa-host-signature"])
            .unwrap_or_else(|e| panic!("signature: {e}"));
        let key = URL_SAFE_NO_PAD
            .decode(identity.public_key_x())
            .unwrap_or_else(|e| panic!("key: {e}"));
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
            .verify(canonical.as_bytes(), &signature)
            .unwrap_or_else(|_| panic!("signature must bind epoch body"));
    }

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
