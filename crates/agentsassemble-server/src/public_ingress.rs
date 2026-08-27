use std::{fmt, sync::Arc};

use axum::http::{HeaderMap, header, uri::Authority};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use url::Url;

use crate::{
    ingress_trust::{
        PeerAddr, TrustedIdentityOrigin, authority_host_ip, normalized_host, single_header,
        single_optional_header,
    },
    product_surface::RouteExposure,
};

pub(crate) const MANUAL_PROXY_TOKEN_HEADER: &str = "x-agentsassemble-proxy-token";
const FORWARDED_PROTO_HEADER: &str = "x-forwarded-proto";
const MIN_MANUAL_PROXY_SECRET_BYTES: usize = 32;
const MAX_MANUAL_PROXY_SECRET_BYTES: usize = 128;

#[derive(Clone)]
pub(crate) struct PublicIngress(Option<Arc<ManualPublicIngress>>);

struct ManualPublicIngress {
    origin: Arc<str>,
    host: Arc<str>,
    port: u16,
    proxy_secret_digest: [u8; 32],
}

impl fmt::Debug for PublicIngress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            None => formatter.write_str("PublicIngress::Disabled"),
            Some(ingress) => formatter
                .debug_struct("PublicIngress::Manual")
                .field("origin", &ingress.origin)
                .field("proxy_secret", &"[REDACTED]")
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

impl PublicIngress {
    pub(crate) fn disabled() -> Self {
        Self(None)
    }

    pub(crate) fn configured_manual(
        origin: &str,
        proxy_secret: &str,
    ) -> Result<Self, ManualPublicIngressError> {
        let origin = public_origin(origin)?;
        if !(MIN_MANUAL_PROXY_SECRET_BYTES..=MAX_MANUAL_PROXY_SECRET_BYTES)
            .contains(&proxy_secret.len())
            || !proxy_secret.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(ManualPublicIngressError::InvalidSecret);
        }
        Ok(Self(Some(Arc::new(ManualPublicIngress {
            origin: origin.value.into(),
            host: origin.host.into(),
            port: origin.port,
            proxy_secret_digest: Sha256::digest(proxy_secret.as_bytes()).into(),
        }))))
    }

    pub(crate) fn authorizes(
        &self,
        peer: PeerAddr,
        headers: &HeaderMap,
        exposure: RouteExposure,
    ) -> bool {
        self.0
            .as_ref()
            .is_some_and(|ingress| ingress.authorizes(peer, headers, exposure))
    }

    pub(crate) fn identity_origin(&self) -> Option<TrustedIdentityOrigin> {
        self.0
            .as_ref()
            .map(|ingress| TrustedIdentityOrigin(ingress.origin.clone()))
    }
}

impl ManualPublicIngress {
    fn authorizes(&self, peer: PeerAddr, headers: &HeaderMap, exposure: RouteExposure) -> bool {
        if !peer.0.ip().is_loopback() || exposure == RouteExposure::Private {
            return false;
        }
        single_header(headers, header::HOST).is_some_and(|host| self.authority_matches(host))
            && single_header(
                headers,
                header::HeaderName::from_static(FORWARDED_PROTO_HEADER),
            ) == Some("https")
            && single_header(
                headers,
                header::HeaderName::from_static(MANUAL_PROXY_TOKEN_HEADER),
            )
            .is_some_and(|secret| self.secret_matches(secret))
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
        public_origin(value).is_ok_and(|origin| origin.value == self.origin.as_ref())
    }

    fn secret_matches(&self, value: &str) -> bool {
        let observed: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        bool::from(self.proxy_secret_digest.ct_eq(&observed))
    }
}

struct PublicOrigin {
    value: String,
    host: String,
    port: u16,
}

fn public_origin(value: &str) -> Result<PublicOrigin, ManualPublicIngressError> {
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
    Ok(PublicOrigin {
        value: url.origin().ascii_serialization(),
        host: host.to_ascii_lowercase(),
        port: url
            .port_or_known_default()
            .ok_or(ManualPublicIngressError::InvalidOrigin)?,
    })
}
