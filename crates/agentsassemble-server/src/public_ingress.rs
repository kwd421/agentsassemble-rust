use std::{
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::http::{HeaderMap, header, uri::Authority};
use parking_lot::RwLock;
use serde::Serialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    ingress_trust::{
        PeerAddr, TrustedIdentityOrigin, authority_host_ip, normalized_host, single_header,
        single_optional_header,
    },
    product_surface::RouteExposure,
    public_ingress_runtime::{GenerationOutcome, run_generation},
    stable_entry::{StableEntry, StableEntryActivationError, StableEntryConfig},
};

pub(crate) const MANUAL_PROXY_TOKEN_HEADER: &str = "x-agentsassemble-proxy-token";
const FORWARDED_HOST_HEADER: &str = "x-forwarded-host";
const FORWARDED_PROTO_HEADER: &str = "x-forwarded-proto";
const MIN_PROXY_SECRET_BYTES: usize = 32;
const MAX_PROXY_SECRET_BYTES: usize = 128;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PublicIngressStatus {
    pub(crate) mode: &'static str,
    pub(crate) public_url: String,
    pub(crate) stable_url: String,
    pub(crate) tunnel: TunnelStatus,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TunnelStatus {
    pub(crate) available: bool,
    pub(crate) running: bool,
    pub(crate) phase: &'static str,
    pub(crate) public_url: String,
    pub(crate) local_url: String,
    pub(crate) stable_phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct PublicIngress(Arc<PublicIngressKind>);

enum PublicIngressKind {
    Disabled,
    Manual(ManualPublicIngress),
    Managed(ManagedPublicIngress),
}

struct ManualPublicIngress {
    local_url: Arc<str>,
    origin: Arc<str>,
    host: Arc<str>,
    port: u16,
    proxy_secret_digest: [u8; 32],
}

struct ManagedPublicIngress {
    projection: Arc<RwLock<ManagedProjection>>,
    controller: ManagedController,
}

struct ManagedController {
    config: ManagedIngressConfig,
    lifecycle: Mutex<ManagedLifecycle>,
}

struct ManagedLifecycle {
    closed: bool,
    active: Option<ActiveGeneration>,
}

impl fmt::Debug for PublicIngress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.as_ref() {
            PublicIngressKind::Disabled => formatter.write_str("PublicIngress::Disabled"),
            PublicIngressKind::Manual(ingress) => formatter
                .debug_struct("PublicIngress::Manual")
                .field("origin", &ingress.origin)
                .field("proxy_secret", &"[REDACTED]")
                .finish_non_exhaustive(),
            PublicIngressKind::Managed(ingress) => formatter
                .debug_struct("PublicIngress::Managed")
                .field("status", &ingress.status())
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ManualPublicIngressError {
    #[error("manual public origin must be one canonical non-loopback HTTPS origin")]
    InvalidOrigin,
    #[error("manual proxy secret must contain 32-128 visible ASCII bytes")]
    InvalidSecret,
}

#[derive(Debug, Error)]
pub enum PublicIngressControlError {
    #[error("managed public ingress is not configured")]
    Unconfigured,
    #[error("managed public ingress cleanup failed")]
    CleanupFailed,
    #[error("managed public ingress is shutting down")]
    Closed,
}

pub(crate) enum PublicIngressAuthorization {
    Authorized,
    Identity(TrustedIdentityOrigin),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadyIngress {
    pub(crate) local_url: String,
    pub(crate) public_url: String,
}

impl PublicIngress {
    pub(crate) fn disabled() -> Self {
        Self(Arc::new(PublicIngressKind::Disabled))
    }

    pub(crate) fn configured_manual(
        listener: SocketAddr,
        origin: &str,
        proxy_secret: &str,
    ) -> Result<Self, ManualPublicIngressError> {
        let origin = CanonicalPublicOrigin::parse(origin)?;
        if !(MIN_PROXY_SECRET_BYTES..=MAX_PROXY_SECRET_BYTES).contains(&proxy_secret.len())
            || !proxy_secret.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(ManualPublicIngressError::InvalidSecret);
        }
        Ok(Self(Arc::new(PublicIngressKind::Manual(
            ManualPublicIngress {
                local_url: format!("http://{listener}").into(),
                origin: origin.value.into(),
                host: origin.host.into(),
                port: origin.port,
                proxy_secret_digest: Sha256::digest(proxy_secret.as_bytes()).into(),
            },
        ))))
    }

    pub(crate) async fn managed(
        listener: SocketAddr,
        stable_entry: Option<StableEntryConfig>,
        state_root: &Path,
    ) -> Result<Self, StableEntryActivationError> {
        let local_url = format!("http://{listener}");
        let cloudflared = which::which("cloudflared").ok();
        let stable_entry = StableEntry::new(stable_entry, state_root).await?;
        let projection = Arc::new(RwLock::new(ManagedProjection::new(
            &local_url,
            cloudflared.is_some(),
        )));
        Ok(Self(Arc::new(PublicIngressKind::Managed(
            ManagedPublicIngress {
                projection,
                controller: ManagedController {
                    config: ManagedIngressConfig {
                        local_url,
                        cloudflared,
                        stable_entry,
                    },
                    lifecycle: Mutex::new(ManagedLifecycle {
                        closed: false,
                        active: None,
                    }),
                },
            },
        ))))
    }

    pub(crate) fn status(&self) -> PublicIngressStatus {
        match self.0.as_ref() {
            PublicIngressKind::Disabled => static_status("unconfigured", ""),
            PublicIngressKind::Manual(ingress) => static_status("manual", &ingress.origin),
            PublicIngressKind::Managed(ingress) => ingress.status(),
        }
    }

    pub(crate) fn ready_snapshot(&self) -> Option<ReadyIngress> {
        match self.0.as_ref() {
            PublicIngressKind::Disabled => None,
            PublicIngressKind::Manual(ingress) => Some(ReadyIngress {
                local_url: ingress.local_url.to_string(),
                public_url: ingress.origin.to_string(),
            }),
            PublicIngressKind::Managed(ingress) => ingress.projection.read().ready_snapshot(),
        }
    }

    pub(crate) async fn start(&self) -> Result<PublicIngressStatus, PublicIngressControlError> {
        let ingress = self.managed_ingress()?;
        let mut lifecycle = ingress.controller.lifecycle.lock().await;
        if lifecycle.closed {
            return Err(PublicIngressControlError::Closed);
        }
        if lifecycle
            .active
            .as_ref()
            .is_some_and(|current| current.owner.is_finished())
        {
            join_active(&ingress.projection, &mut lifecycle.active).await;
        }
        if lifecycle.active.is_none() {
            ingress.projection.read().cleanup_result()?;
            lifecycle.active = Some(start_generation(
                &ingress.controller.config,
                &ingress.projection,
            ));
        }
        Ok(ingress.status())
    }

    pub(crate) async fn stop(&self) -> Result<PublicIngressStatus, PublicIngressControlError> {
        let ingress = self.managed_ingress()?;
        let mut lifecycle = ingress.controller.lifecycle.lock().await;
        if lifecycle.closed {
            return Err(PublicIngressControlError::Closed);
        }
        stop_active(
            &ingress.projection,
            &ingress.controller.config.stable_entry,
            &mut lifecycle.active,
        )
        .await;
        Ok(ingress.status())
    }

    pub(crate) async fn shutdown(&self) -> Result<(), PublicIngressControlError> {
        let PublicIngressKind::Managed(ingress) = self.0.as_ref() else {
            return Ok(());
        };
        let mut lifecycle = ingress.controller.lifecycle.lock().await;
        lifecycle.closed = true;
        stop_active(
            &ingress.projection,
            &ingress.controller.config.stable_entry,
            &mut lifecycle.active,
        )
        .await;
        ingress.projection.read().cleanup_result()
    }

    pub(crate) fn authorize(
        &self,
        peer: PeerAddr,
        headers: &HeaderMap,
        exposure: RouteExposure,
    ) -> Option<PublicIngressAuthorization> {
        match self.0.as_ref() {
            PublicIngressKind::Disabled => None,
            PublicIngressKind::Manual(ingress) => ingress
                .authorizes(peer, headers, exposure)
                .then(|| authorization(exposure, || TrustedIdentityOrigin(ingress.origin.clone()))),
            PublicIngressKind::Managed(ingress) => {
                if !peer.0.ip().is_loopback() || exposure == RouteExposure::Private {
                    return None;
                }
                ingress.projection.read().authorize(headers, exposure)
            }
        }
    }

    fn managed_ingress(&self) -> Result<&ManagedPublicIngress, PublicIngressControlError> {
        let PublicIngressKind::Managed(ingress) = self.0.as_ref() else {
            return Err(PublicIngressControlError::Unconfigured);
        };
        Ok(ingress)
    }
}

fn authorization(
    exposure: RouteExposure,
    identity_origin: impl FnOnce() -> TrustedIdentityOrigin,
) -> PublicIngressAuthorization {
    if exposure == RouteExposure::IdentityProbePublic {
        PublicIngressAuthorization::Identity(identity_origin())
    } else {
        PublicIngressAuthorization::Authorized
    }
}

fn static_status(mode: &'static str, public_url: &str) -> PublicIngressStatus {
    PublicIngressStatus {
        mode,
        public_url: public_url.to_owned(),
        stable_url: String::new(),
        tunnel: TunnelStatus {
            available: false,
            running: false,
            phase: "stopped",
            public_url: public_url.to_owned(),
            local_url: String::new(),
            stable_phase: "unconfigured",
            last_error: None,
        },
    }
}

impl ManualPublicIngress {
    fn authorizes(&self, peer: PeerAddr, headers: &HeaderMap, exposure: RouteExposure) -> bool {
        if !peer.0.ip().is_loopback() || exposure == RouteExposure::Private {
            return false;
        }
        single_header(headers, header::HOST).is_some_and(|host| self.authority_matches(host))
            && forwarded_https(headers)
            && single_header(
                headers,
                header::HeaderName::from_static(MANUAL_PROXY_TOKEN_HEADER),
            )
            .is_some_and(|secret| digest_matches(secret, &self.proxy_secret_digest))
            && single_optional_header(headers, header::ORIGIN).is_ok_and(|origin| match exposure {
                RouteExposure::Private => false,
                RouteExposure::IdentityProbePublic => true,
                RouteExposure::SameOriginPublic => {
                    origin.is_none_or(|origin| self.origin_matches(origin))
                }
            })
    }

    fn authority_matches(&self, value: &str) -> bool {
        let Ok(authority) = value.parse::<Authority>() else {
            return false;
        };
        normalized_host(authority.host()).eq_ignore_ascii_case(&self.host)
            && authority.port_u16().unwrap_or(443) == self.port
    }

    fn origin_matches(&self, value: &str) -> bool {
        CanonicalPublicOrigin::parse(value).is_ok_and(|origin| origin.value == self.origin.as_ref())
    }
}

impl ManagedPublicIngress {
    fn status(&self) -> PublicIngressStatus {
        let stable = self.controller.config.stable_entry.status();
        let mut status = self.projection.read().status();
        let stable_matches_direct = stable_target_matches_direct(stable.target.as_deref(), &status);
        status.stable_url = if stable_matches_direct {
            stable.url
        } else {
            String::new()
        };
        status.tunnel.stable_phase = if stable.phase == "ready" && !stable_matches_direct {
            "pending"
        } else {
            stable.phase
        };
        status.tunnel.last_error = status.tunnel.last_error.or(stable.last_error);
        status
    }
}

fn stable_target_matches_direct(target: Option<&str>, status: &PublicIngressStatus) -> bool {
    match target {
        Some(target) => target == status.public_url,
        None => {
            status.public_url.is_empty()
                && !matches!(status.tunnel.phase, "starting" | "running" | "stopping")
        }
    }
}

pub(crate) struct CanonicalPublicOrigin {
    pub(crate) value: String,
    host: String,
    port: u16,
}

impl CanonicalPublicOrigin {
    pub(crate) fn parse(value: &str) -> Result<Self, ManualPublicIngressError> {
        let url = Url::parse(value.trim()).map_err(|_| ManualPublicIngressError::InvalidOrigin)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ManualPublicIngressError::InvalidOrigin);
        }
        let host = url
            .host_str()
            .map(normalized_host)
            .filter(|host| !host.is_empty())
            .ok_or(ManualPublicIngressError::InvalidOrigin)?;
        let numeric_host = authority_host_ip(host);
        if host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
            || numeric_host.is_some_and(|address| address.is_loopback() || address.is_unspecified())
        {
            return Err(ManualPublicIngressError::InvalidOrigin);
        }
        Ok(Self {
            value: url.origin().ascii_serialization(),
            host: host.to_ascii_lowercase(),
            port: url
                .port_or_known_default()
                .ok_or(ManualPublicIngressError::InvalidOrigin)?,
        })
    }

    fn authority_matches(&self, value: &str) -> bool {
        let Ok(authority) = value.parse::<Authority>() else {
            return false;
        };
        normalized_host(authority.host()).eq_ignore_ascii_case(&self.host)
            && authority.port_u16().unwrap_or(443) == self.port
    }
}

#[derive(Clone)]
pub(crate) struct ManagedIngressConfig {
    pub(crate) local_url: String,
    pub(crate) cloudflared: Option<PathBuf>,
    pub(crate) stable_entry: StableEntry,
}

struct ActiveGeneration {
    number: u64,
    cancellation: CancellationToken,
    owner: JoinHandle<()>,
}

fn start_generation(
    config: &ManagedIngressConfig,
    projection: &Arc<RwLock<ManagedProjection>>,
) -> ActiveGeneration {
    if config.cloudflared.is_none() {
        let generation = projection.write().begin_unavailable();
        return stable_clear_owner(projection, &config.stable_entry, generation);
    }
    let generation = projection.write().begin_start();
    config.stable_entry.begin_generation();
    let cancellation = CancellationToken::new();
    let task_config = config.clone();
    let task_projection = projection.clone();
    let task_cancellation = cancellation.clone();
    let owner = tokio::spawn(async move {
        let outcome = run_generation(
            generation,
            task_config,
            task_projection.clone(),
            task_cancellation,
        )
        .await;
        task_projection.write().finish(generation, outcome);
    });
    ActiveGeneration {
        number: generation,
        cancellation,
        owner,
    }
}

async fn stop_active(
    projection: &Arc<RwLock<ManagedProjection>>,
    stable_entry: &StableEntry,
    active: &mut Option<ActiveGeneration>,
) {
    if active.is_none() {
        projection.write().settle_stopped();
        let generation = projection.read().generation;
        *active = Some(stable_clear_owner(projection, stable_entry, generation));
    }
    let current = active.as_mut().unwrap_or_else(|| unreachable!());
    if !current.owner.is_finished() {
        projection.write().begin_stop(current.number);
        current.cancellation.cancel();
    }
    join_active(projection, active).await;
}

fn stable_clear_owner(
    projection: &Arc<RwLock<ManagedProjection>>,
    stable_entry: &StableEntry,
    generation: u64,
) -> ActiveGeneration {
    let task_projection = projection.clone();
    let task_stable_entry = stable_entry.clone();
    let cancellation = CancellationToken::new();
    let owner = tokio::spawn(async move {
        if !task_stable_entry.clear().await {
            task_projection
                .write()
                .cleanup_failed(generation, "stable-entry cleanup failed");
        }
    });
    ActiveGeneration {
        number: generation,
        cancellation,
        owner,
    }
}

async fn join_active(
    projection: &Arc<RwLock<ManagedProjection>>,
    active: &mut Option<ActiveGeneration>,
) {
    let current = active.as_mut().unwrap_or_else(|| unreachable!());
    let number = current.number;
    if (&mut current.owner).await.is_err() {
        projection
            .write()
            .finish(number, GenerationOutcome::owner_failed());
    }
    active.take();
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IngressPhase {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

pub(crate) struct ManagedProjection {
    generation: u64,
    phase: IngressPhase,
    trust: Option<ManagedTrust>,
    available: bool,
    local_url: String,
    last_error: Option<String>,
    cleanup_failed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedReadiness {
    Rejected,
    Unchanged,
    Changed,
}

struct ManagedTrust {
    origin: CanonicalPublicOrigin,
    origin_host_digest: [u8; 32],
}

impl ManagedProjection {
    fn new(local_url: &str, available: bool) -> Self {
        Self {
            generation: 0,
            phase: IngressPhase::Stopped,
            trust: None,
            available,
            local_url: local_url.to_owned(),
            last_error: None,
            cleanup_failed: false,
        }
    }

    fn begin_start(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.phase = IngressPhase::Starting;
        self.trust = None;
        self.last_error = None;
        self.cleanup_failed = false;
        self.generation
    }

    pub(crate) fn ready_managed(
        &mut self,
        generation: u64,
        origin: CanonicalPublicOrigin,
        origin_host: &str,
    ) -> ManagedReadiness {
        if self.generation != generation {
            return ManagedReadiness::Rejected;
        }
        if !matches!(self.phase, IngressPhase::Starting | IngressPhase::Running) {
            return ManagedReadiness::Rejected;
        }
        if self
            .trust
            .as_ref()
            .is_some_and(|trust| trust.origin.value == origin.value)
        {
            return ManagedReadiness::Unchanged;
        }
        self.phase = IngressPhase::Running;
        self.trust = Some(ManagedTrust {
            origin,
            origin_host_digest: Sha256::digest(origin_host.as_bytes()).into(),
        });
        ManagedReadiness::Changed
    }

    pub(crate) fn begin_stop(&mut self, generation: u64) {
        if self.generation == generation
            && matches!(self.phase, IngressPhase::Starting | IngressPhase::Running)
        {
            self.phase = IngressPhase::Stopping;
            self.trust = None;
        }
    }

    pub(crate) fn revoke(&mut self, generation: u64) {
        if self.generation == generation {
            self.trust = None;
        }
    }

    fn finish(&mut self, generation: u64, outcome: GenerationOutcome) {
        if self.generation != generation {
            return;
        }
        self.trust = None;
        self.last_error = outcome.error;
        self.cleanup_failed = outcome.cleanup_failed;
        self.phase = if self.last_error.is_some() {
            IngressPhase::Error
        } else {
            IngressPhase::Stopped
        };
    }

    fn begin_unavailable(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.phase = IngressPhase::Stopped;
        self.trust = None;
        self.last_error = Some("cloudflared is not installed".to_owned());
        self.cleanup_failed = false;
        self.generation
    }

    fn settle_stopped(&mut self) {
        if self.phase != IngressPhase::Error {
            self.phase = IngressPhase::Stopped;
            self.trust = None;
        }
    }

    fn cleanup_result(&self) -> Result<(), PublicIngressControlError> {
        (!self.cleanup_failed)
            .then_some(())
            .ok_or(PublicIngressControlError::CleanupFailed)
    }

    fn cleanup_failed(&mut self, generation: u64, message: &str) {
        if self.generation == generation {
            self.trust = None;
            self.last_error = Some(message.to_owned());
            self.cleanup_failed = true;
            self.phase = IngressPhase::Error;
        }
    }

    fn status(&self) -> PublicIngressStatus {
        let public_url = self
            .trust
            .as_ref()
            .map_or_else(String::new, |trust| trust.origin.value.clone());
        PublicIngressStatus {
            mode: "managed",
            public_url: public_url.clone(),
            stable_url: String::new(),
            tunnel: TunnelStatus {
                available: self.available,
                running: matches!(
                    self.phase,
                    IngressPhase::Starting | IngressPhase::Running | IngressPhase::Stopping
                ),
                phase: match self.phase {
                    IngressPhase::Stopped => "stopped",
                    IngressPhase::Starting => "starting",
                    IngressPhase::Running => "running",
                    IngressPhase::Stopping => "stopping",
                    IngressPhase::Error => "error",
                },
                public_url,
                local_url: self.local_url.clone(),
                stable_phase: "unconfigured",
                last_error: self.last_error.clone(),
            },
        }
    }

    fn ready_snapshot(&self) -> Option<ReadyIngress> {
        let trust = self.trust.as_ref()?;
        (self.phase == IngressPhase::Running).then(|| ReadyIngress {
            local_url: self.local_url.clone(),
            public_url: trust.origin.value.clone(),
        })
    }

    fn authorize(
        &self,
        headers: &HeaderMap,
        exposure: RouteExposure,
    ) -> Option<PublicIngressAuthorization> {
        let trust = self.trust.as_ref()?;
        trust.authorizes(headers, exposure).then(|| {
            authorization(exposure, || {
                TrustedIdentityOrigin(trust.origin.value.as_str().into())
            })
        })
    }
}

impl ManagedTrust {
    fn authorizes(&self, headers: &HeaderMap, exposure: RouteExposure) -> bool {
        single_header(headers, header::HOST)
            .and_then(|host| host.parse::<Authority>().ok())
            .is_some_and(|host| {
                host.port_u16().is_none()
                    && digest_matches(
                        &normalized_host(host.host()).to_ascii_lowercase(),
                        &self.origin_host_digest,
                    )
            })
            && forwarded_https(headers)
            && single_header(
                headers,
                header::HeaderName::from_static(FORWARDED_HOST_HEADER),
            )
            .is_some_and(|host| self.origin.authority_matches(host))
            && single_optional_header(headers, header::ORIGIN).is_ok_and(|origin| {
                exposure == RouteExposure::IdentityProbePublic
                    || origin.is_none_or(|origin| {
                        CanonicalPublicOrigin::parse(origin)
                            .is_ok_and(|observed| observed.value == self.origin.value)
                    })
            })
    }
}

fn forwarded_https(headers: &HeaderMap) -> bool {
    single_header(
        headers,
        header::HeaderName::from_static(FORWARDED_PROTO_HEADER),
    ) == Some("https")
}

fn digest_matches(value: &str, expected: &[u8; 32]) -> bool {
    let observed: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    bool::from(expected.ct_eq(&observed))
}

pub(crate) fn generated_origin_host() -> String {
    format!("aas-{}.origin.invalid", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
#[path = "public_ingress/managed_lifecycle_tests.rs"]
pub(crate) mod managed_lifecycle_tests;
