//! Fixed-address provision parsing and once-per-open orchestration.
//!
//! Central owns the Cloudflare API; the host only triggers one host-signed
//! `POST /v1/servers/:id/named-tunnel/provision` per external-access open
//! when no stored credential for the live epoch exists, then runs the named
//! connector. Every failure falls back to today's quick tunnel for that run
//! and never blocks external access. The credentials JSON never enters logs or
//! argv; the parsed credential redacts it in `Debug`.

use agentsassemble_persistence::{SqliteStore, StoredNamedTunnel};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    central::{directory::CentralDirectory, host_identity::CentralHostIdentity},
    public_ingress::CanonicalPublicOrigin,
};

const MAX_RESPONSE_BYTES: usize = 16 * 1024;
const MAX_CREDENTIALS_BYTES: usize = 8192;
const MAX_TUNNEL_ID_BYTES: usize = 128;

/// A successfully provisioned fixed-address credential. The credentials JSON
/// is redacted in `Debug`.
#[derive(Clone)]
pub(crate) struct ProvisionedTunnel {
    hostname: String,
    origin: String,
    tunnel_id: String,
    credentials: String,
}

impl ProvisionedTunnel {
    #[must_use]
    pub(crate) fn hostname(&self) -> &str {
        &self.hostname
    }

    #[must_use]
    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    #[must_use]
    pub(crate) fn tunnel_id(&self) -> &str {
        &self.tunnel_id
    }

    #[must_use]
    pub(crate) fn credentials(&self) -> &str {
        &self.credentials
    }
}

impl std::fmt::Debug for ProvisionedTunnel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProvisionedTunnel")
            .field("hostname", &self.hostname)
            .field("origin", &self.origin)
            .field("tunnel_id", &self.tunnel_id)
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionCredentials {
    #[serde(rename = "AccountTag")]
    account_tag: String,
    #[serde(rename = "TunnelID")]
    tunnel_id: String,
    #[serde(rename = "TunnelSecret")]
    tunnel_secret: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionResponse {
    credentials: ProvisionCredentials,
    tunnel_id: String,
    hostname: String,
    origin: String,
}

/// Parses one central provision response with exact keys and consistency:
/// `origin == "https://" + hostname`, `credentials.TunnelID == tunnel_id`,
/// and the hostname matches the derived fixed shape. Rejects anything else
/// without logging the credentials.
pub(crate) fn parse_provision_response(bytes: &[u8]) -> Result<ProvisionedTunnel, &'static str> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("provision response is too large");
    }
    let response: ProvisionResponse =
        serde_json::from_slice(bytes).map_err(|_| "provision response is invalid")?;
    if !agentsassemble_persistence::is_valid_named_tunnel_hostname(&response.hostname) {
        return Err("provision hostname is invalid");
    }
    if response.origin != format!("https://{}", response.hostname) {
        return Err("provision origin does not match hostname");
    }
    CanonicalPublicOrigin::parse(&response.origin).map_err(|_| "provision origin is invalid")?;
    validate_tunnel_id(&response.tunnel_id)?;
    validate_credential_field(&response.credentials.account_tag)?;
    validate_credential_field(&response.credentials.tunnel_id)?;
    validate_credential_field(&response.credentials.tunnel_secret)?;
    if response.credentials.tunnel_id != response.tunnel_id {
        return Err("provision tunnel id does not match credentials");
    }
    let credentials = serde_json::to_string(&serde_json::json!({
        "AccountTag": response.credentials.account_tag,
        "TunnelID": response.credentials.tunnel_id,
        "TunnelSecret": response.credentials.tunnel_secret,
    }))
    .map_err(|_| "provision response is invalid")?;
    if credentials.len() > MAX_CREDENTIALS_BYTES {
        return Err("provision credentials are invalid");
    }
    Ok(ProvisionedTunnel {
        hostname: response.hostname,
        origin: response.origin,
        tunnel_id: response.tunnel_id,
        credentials,
    })
}

fn validate_tunnel_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > MAX_TUNNEL_ID_BYTES
        || value.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err("provision tunnel id is invalid");
    }
    Ok(())
}

fn validate_credential_field(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > MAX_CREDENTIALS_BYTES
        || value.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err("provision credentials are invalid");
    }
    Ok(())
}

/// Stable idempotency key per `(server_id, registration_epoch)` so retries
/// of the same incarnation are idempotent without carrying user text.
#[must_use]
pub(crate) fn idempotency_key(server_id: &str, registration_epoch: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"named-tunnel-v1\n");
    hasher.update(server_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(registration_epoch.as_bytes());
    hex::encode(hasher.finalize())
}

/// How one external-access open should connect.
#[derive(Debug)]
pub(crate) enum TunnelPlan {
    /// A stored or freshly provisioned token for the live epoch. The plan
    /// carries no secret; the connector reads it from storage.
    Named(StoredNamedTunnel),
    /// Use today's quick tunnel for this run. The reason is a bounded,
    /// secret-free label for status/diagnostics.
    Quick { reason: &'static str },
}

/// Resolves the tunnel mode for one external-access open: a stored credential for
/// the live epoch wins with zero central calls; otherwise exactly one
/// provision attempt (bounded by the daily budget and the existing quota
/// park) decides between a fresh named credential and the quick fallback.
pub(crate) async fn ensure_named_tunnel(
    store: &SqliteStore,
    directory: &CentralDirectory,
    identity: &CentralHostIdentity,
) -> TunnelPlan {
    let now_secs = chrono::Utc::now().timestamp();
    let live_epoch = match store.registration_epoch().await {
        Ok(epoch) => epoch,
        Err(_) => {
            return TunnelPlan::Quick {
                reason: "registration_unavailable",
            };
        }
    };
    let Some(live_epoch) = live_epoch else {
        return TunnelPlan::Quick {
            reason: "unregistered",
        };
    };
    match store.named_tunnel().await {
        Ok(Some(stored)) if stored.registration_epoch() == live_epoch => {
            return TunnelPlan::Named(stored);
        }
        Ok(Some(_)) => {
            let _ = store.clear_named_tunnel().await;
        }
        Ok(None) => {}
        Err(_) => {}
    }
    if !directory.status().enabled {
        return TunnelPlan::Quick {
            reason: "central_disabled",
        };
    }
    if directory.status().quota_parked_until > now_secs {
        return TunnelPlan::Quick {
            reason: "quota_parked",
        };
    }
    let allowed = store
        .named_tunnel_provision_allowed(now_secs)
        .await
        .unwrap_or(false);
    if !allowed {
        return TunnelPlan::Quick {
            reason: "provision_budget",
        };
    }
    let key = idempotency_key(identity.server_id(), &live_epoch);
    let outcome = directory
        .provision_named_tunnel(store, identity, &live_epoch, &key)
        .await;
    // Only requests that reached the provision route spend budget. Disabled
    // never sends; everything else went through the shared signed sender
    // (which also enforces the quota gate without spending on a parked skip).
    if !matches!(
        outcome,
        Err(crate::central::directory::CentralDirectoryError::Disabled)
    ) {
        let _ = store.record_named_tunnel_provision_attempt(now_secs).await;
    }
    match outcome {
        Ok(bytes) => match parse_provision_response(&bytes) {
            Ok(provisioned) => match store
                .save_named_tunnel(
                    provisioned.hostname(),
                    provisioned.tunnel_id(),
                    provisioned.credentials(),
                    &live_epoch,
                )
                .await
            {
                Ok(stored) => TunnelPlan::Named(stored),
                Err(_) => TunnelPlan::Quick {
                    reason: "credential_store_failed",
                },
            },
            Err(_) => TunnelPlan::Quick {
                reason: "invalid_provision_response",
            },
        },
        Err(error) => {
            use crate::central::directory::CentralDirectoryError as DirectoryError;
            // A refusal means the incarnation or its stored credential is no
            // good; drop any leftover so the next open starts clean.
            if matches!(
                error,
                DirectoryError::Rejected
                    | DirectoryError::ServerRetired
                    | DirectoryError::AccountDeleted
                    | DirectoryError::RegistrationAbsent
            ) {
                let _ = store.clear_named_tunnel().await;
            }
            TunnelPlan::Quick {
                reason: match error {
                    DirectoryError::QuotaExhausted => "quota_parked",
                    DirectoryError::CapacityExhausted => "central_capacity",
                    DirectoryError::Unavailable => "central_unavailable",
                    DirectoryError::Rejected
                    | DirectoryError::ServerRetired
                    | DirectoryError::AccountDeleted
                    | DirectoryError::RegistrationAbsent => "central_refused",
                    DirectoryError::Disabled => "central_disabled",
                    _ => "provision_failed",
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(hostname: &str, origin: &str, tunnel_id: &str) -> Vec<u8> {
        provision_body(hostname, origin, tunnel_id, tunnel_id, None).to_string().into_bytes()
    }

    fn provision_body(
        hostname: &str,
        origin: &str,
        tunnel_id: &str,
        credentials_tunnel_id: &str,
        extra: Option<(&str, serde_json::Value)>,
    ) -> serde_json::Value {
        let mut value = serde_json::json!({
            "credentials": {
                "AccountTag": "account-tag",
                "TunnelID": credentials_tunnel_id,
                "TunnelSecret": "tunnel-secret",
            },
            "tunnel_id": tunnel_id,
            "hostname": hostname,
            "origin": origin,
        });
        if let Some((key, val)) = extra {
            value[key] = val;
        }
        value
    }

    #[test]
    fn provision_response_requires_exact_keys_and_consistency() {
        let ok = parse_provision_response(&response(
            "h-0123456789ab.agentsassemble.uk",
            "https://h-0123456789ab.agentsassemble.uk",
            "tunnel-id-one",
        ))
        .unwrap_or_else(|error| panic!("valid provision response: {error}"));
        assert_eq!(
            format!("https://{}", ok.hostname()),
            "https://h-0123456789ab.agentsassemble.uk"
        );
        assert_eq!(ok.tunnel_id(), "tunnel-id-one");
        assert_eq!(ok.origin(), "https://h-0123456789ab.agentsassemble.uk");
        let credentials: serde_json::Value = serde_json::from_str(ok.credentials())
            .unwrap_or_else(|error| panic!("stored credentials JSON: {error}"));
        assert_eq!(credentials["TunnelID"], "tunnel-id-one");
        assert_eq!(credentials["AccountTag"], "account-tag");
        let rendered = format!("{ok:?}");
        assert!(!rendered.contains("tunnel-secret"));
        assert!(!rendered.contains(ok.credentials()));
        assert!(rendered.contains("[REDACTED]"));
        parse_provision_response(&response(
            "h-0123456789abcdef.agentsassemble.uk",
            "https://h-0123456789abcdef.agentsassemble.uk",
            "tunnel-id-two",
        ))
        .unwrap_or_else(|error| panic!("16-hex hostname: {error}"));

        for value in [
            provision_body(
                "h-0123456789ab.agentsassemble.uk",
                "https://other.agentsassemble.uk",
                "tunnel-id-one",
                "tunnel-id-one",
                None,
            ),
            provision_body(
                "phish.agentsassemble.uk",
                "https://phish.agentsassemble.uk",
                "tunnel-id-one",
                "tunnel-id-one",
                None,
            ),
            provision_body(
                "h-0123456789AB.agentsassemble.uk",
                "https://h-0123456789AB.agentsassemble.uk",
                "tunnel-id-one",
                "tunnel-id-one",
                None,
            ),
            provision_body(
                "h-0123456789ab.agentsassemble.uk",
                "https://h-0123456789ab.agentsassemble.uk/",
                "tunnel-id-one",
                "tunnel-id-one",
                None,
            ),
            provision_body(
                "h-0123456789ab.agentsassemble.uk",
                "https://h-0123456789ab.agentsassemble.uk",
                "tunnel-id-one",
                "other-tunnel-id",
                None,
            ),
            provision_body(
                "h-0123456789ab.agentsassemble.uk",
                "https://h-0123456789ab.agentsassemble.uk",
                "tunnel-id-one",
                "tunnel-id-one",
                Some(("unexpected", serde_json::json!("1"))),
            ),
            provision_body(
                "h-0123456789ab.agentsassemble.uk",
                "https://h-0123456789ab.agentsassemble.uk",
                "",
                "",
                None,
            ),
            serde_json::json!({
                "hostname": "h-0123456789ab.agentsassemble.uk",
                "origin": "https://h-0123456789ab.agentsassemble.uk",
                "tunnel_token": "legacy-token",
            }),
        ] {
            assert!(
                parse_provision_response(value.to_string().as_bytes()).is_err(),
                "accepted {value}"
            );
        }
        assert!(
            parse_provision_response(b"{\"hostname\":\"h-0123456789ab.agentsassemble.uk\"}")
                .is_err()
        );
        assert!(parse_provision_response(&vec![b'x'; MAX_RESPONSE_BYTES + 1]).is_err());
    }

    #[test]
    fn idempotency_key_is_stable_per_server_and_epoch() {
        let first = idempotency_key("server-id", "epoch-one");
        assert_eq!(first, idempotency_key("server-id", "epoch-one"));
        assert_ne!(first, idempotency_key("server-id", "epoch-two"));
        assert_ne!(first, idempotency_key("other-server", "epoch-one"));
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    async fn test_directory(
        router: axum::Router,
    ) -> (
        CentralDirectory,
        SqliteStore,
        CentralHostIdentity,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        tokio_util::sync::CancellationToken,
        tokio::task::JoinHandle<()>,
    ) {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let hits = Arc::new(AtomicUsize::new(0));
        let counting = hits.clone();
        let router = router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let counting = counting.clone();
                async move {
                    counting.fetch_add(1, Ordering::SeqCst);
                    next.run(request).await
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("bind fake central: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("fake central address: {error}"));
        let cancellation = tokio_util::sync::CancellationToken::new();
        let shutdown = cancellation.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap_or_else(|error| panic!("serve fake central: {error}"));
        });
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("store: {error}"));
        store
            .bootstrap_local_authority("449ce88d-61cd-471f-82a1-e03242ff210f", "Owner")
            .await
            .unwrap_or_else(|error| panic!("bootstrap: {error}"));
        store
            .set_registration_epoch(Some("tunnel-epoch"))
            .await
            .unwrap_or_else(|error| panic!("epoch: {error}"));
        let identity = CentralHostIdentity::from_persistent(
            &store
                .host_identity()
                .await
                .unwrap_or_else(|error| panic!("identity: {error}")),
        )
        .unwrap_or_else(|error| panic!("host identity: {error}"));
        let directory = CentralDirectory::configured(&format!("http://{address}"))
            .unwrap_or_else(|error| panic!("directory: {error}"));
        (directory, store, identity, hits, cancellation, server)
    }

    fn provision_router(status: u16, body: serde_json::Value) -> axum::Router {
        use axum::{http::StatusCode, routing::post};
        axum::Router::new().route(
            "/v1/servers/{id}/named-tunnel/provision",
            post(move || {
                let (status, body) = (status, body.clone());
                async move {
                    (
                        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        axum::Json(body),
                    )
                }
            }),
        )
    }

    fn valid_provision_body(hostname: &str, tunnel_id: &str) -> serde_json::Value {
        serde_json::json!({
            "credentials": {
                "AccountTag": "account-tag",
                "TunnelID": tunnel_id,
                "TunnelSecret": "fresh-secret",
            },
            "tunnel_id": tunnel_id,
            "hostname": hostname,
            "origin": format!("https://{hostname}"),
        })
    }

    fn test_credentials(tunnel_id: &str) -> String {
        serde_json::json!({
            "AccountTag": "account-tag",
            "TunnelID": tunnel_id,
            "TunnelSecret": format!("secret-for-{tunnel_id}"),
        })
        .to_string()
    }

    #[tokio::test]
    async fn stored_credential_reuses_with_zero_provision_calls() {
        let (directory, store, identity, hits, cancellation, server) =
            test_directory(provision_router(200, serde_json::json!({}))).await;
        store
            .save_named_tunnel(
                "h-0123456789ab.agentsassemble.uk",
                "stored-tunnel-id",
                &test_credentials("stored-tunnel-id"),
                "tunnel-epoch",
            )
            .await
            .unwrap_or_else(|error| panic!("save stored credential: {error}"));
        let plan = ensure_named_tunnel(&store, &directory, &identity).await;
        let TunnelPlan::Named(stored) = plan else {
            panic!("stored credential must win without provision");
        };
        assert_eq!(stored.origin(), "https://h-0123456789ab.agentsassemble.uk");
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "restart must send zero provision calls"
        );
        cancellation.cancel();
        server
            .await
            .unwrap_or_else(|error| panic!("join fake central: {error}"));
    }

    #[tokio::test]
    async fn provision_success_stores_the_fixed_origin() {
        let (directory, store, identity, hits, cancellation, server) =
            test_directory(provision_router(
                200,
                valid_provision_body("h-abcdef0123456789.agentsassemble.uk", "tunnel-id-one"),
            ))
            .await;
        let plan = ensure_named_tunnel(&store, &directory, &identity).await;
        let TunnelPlan::Named(stored) = plan else {
            panic!("successful provision must run named");
        };
        assert_eq!(
            stored.origin(),
            "https://h-abcdef0123456789.agentsassemble.uk"
        );
        assert_eq!(stored.tunnel_id(), "tunnel-id-one");
        assert_eq!(
            store
                .named_tunnel()
                .await
                .unwrap_or_else(|error| panic!("read stored: {error}"))
                .map(|stored| stored.origin()),
            Some("https://h-abcdef0123456789.agentsassemble.uk".to_owned())
        );
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Second open reuses the stored credential without another call.
        let plan = ensure_named_tunnel(&store, &directory, &identity).await;
        assert!(matches!(plan, TunnelPlan::Named(_)));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        cancellation.cancel();
        server
            .await
            .unwrap_or_else(|error| panic!("join fake central: {error}"));
    }

    #[tokio::test]
    async fn provision_created_is_accepted_defensively() {
        let (directory, store, identity, hits, cancellation, server) =
            test_directory(provision_router(
                201,
                valid_provision_body("h-0123456789ab.agentsassemble.uk", "tunnel-id-201"),
            ))
            .await;
        let plan = ensure_named_tunnel(&store, &directory, &identity).await;
        let TunnelPlan::Named(stored) = plan else {
            panic!("201 provision must run named");
        };
        assert_eq!(stored.tunnel_id(), "tunnel-id-201");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        cancellation.cancel();
        server
            .await
            .unwrap_or_else(|error| panic!("join fake central: {error}"));
    }

    #[tokio::test]
    async fn every_central_failure_falls_back_to_quick() {
        for (status, body, expect_hits) in [
            (
                503,
                serde_json::json!({"error": {"code": "named_tunnel_unavailable"}}),
                1,
            ),
            (
                429,
                serde_json::json!({"error": {"code": "temporary_capacity_exhausted"}}),
                1,
            ),
            (
                429,
                serde_json::json!({"error": {"code": "actor_quota_exhausted"}}),
                1,
            ),
            (
                400,
                serde_json::json!({"error": {"code": "bad_request"}}),
                1,
            ),
            (404, serde_json::json!({"error": {"code": "not_found"}}), 1),
            (500, serde_json::json!({"error": {"code": "internal"}}), 1),
            (
                200,
                serde_json::json!({
                    "credentials": {
                        "AccountTag": "account-tag",
                        "TunnelID": "tunnel-id-one",
                        "TunnelSecret": "secret",
                    },
                    "tunnel_id": "other-tunnel-id",
                    "hostname": "h-0123456789ab.agentsassemble.uk",
                    "origin": "https://h-0123456789ab.agentsassemble.uk",
                }),
                1,
            ),
        ] {
            let (directory, store, identity, hits, cancellation, server) =
                test_directory(provision_router(status, body)).await;
            let plan = ensure_named_tunnel(&store, &directory, &identity).await;
            assert!(
                matches!(plan, TunnelPlan::Quick { .. }),
                "status {status} must fall back to quick"
            );
            assert!(
                store
                    .named_tunnel()
                    .await
                    .unwrap_or_else(|error| panic!("read stored: {error}"))
                    .is_none()
            );
            assert_eq!(
                hits.load(std::sync::atomic::Ordering::SeqCst),
                expect_hits,
                "status {status} call count"
            );
            cancellation.cancel();
            server
                .await
                .unwrap_or_else(|error| panic!("join fake central: {error}"));
        }
    }

    #[tokio::test]
    async fn quota_park_and_daily_budget_skip_the_network() {
        let (directory, store, identity, hits, cancellation, server) =
            test_directory(provision_router(
                429,
                serde_json::json!({"error": {"code": "actor_quota_exhausted"}}),
            ))
            .await;
        let plan = ensure_named_tunnel(&store, &directory, &identity).await;
        assert!(matches!(plan, TunnelPlan::Quick { .. }));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        // While the daily park holds, the next open falls back without a call.
        let plan = ensure_named_tunnel(&store, &directory, &identity).await;
        assert!(matches!(
            plan,
            TunnelPlan::Quick {
                reason: "quota_parked"
            }
        ));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        cancellation.cancel();
        server
            .await
            .unwrap_or_else(|error| panic!("join fake central: {error}"));
    }

    #[tokio::test]
    async fn central_disabled_skips_the_network() {
        let (directory, store, identity, hits, cancellation, server) =
            test_directory(provision_router(200, serde_json::json!({}))).await;
        let disabled = CentralDirectory::disabled();
        let plan = ensure_named_tunnel(&store, &disabled, &identity).await;
        assert!(matches!(
            plan,
            TunnelPlan::Quick {
                reason: "central_disabled"
            }
        ));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        let _ = (directory, cancellation, server);
    }
}
