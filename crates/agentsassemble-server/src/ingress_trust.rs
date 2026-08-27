use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use crate::http_api::TAURI_ORIGINS;
use crate::product_surface::{RouteExposure, registered_route_exposure, registered_route_path};
use crate::public_ingress::{MANUAL_PROXY_TOKEN_HEADER, PublicIngress};
use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderMap, Method, StatusCode, header, uri::Authority},
    middleware::Next,
    response::{IntoResponse, Response},
};

const EXACT_PROXY_PROVENANCE_HEADERS: [&str; 6] = [
    "forwarded",
    "via",
    "x-real-ip",
    "cdn-loop",
    MANUAL_PROXY_TOKEN_HEADER,
    "x-agentsassemble-client-ip",
];

#[derive(Clone, Copy)]
pub(crate) struct LocalIngress {
    listener: SocketAddr,
}

impl LocalIngress {
    pub(crate) fn from_listener(address: SocketAddr) -> Option<Self> {
        local_bind_is_supported(address).then_some(Self { listener: address })
    }

    fn authorizes(self, peer: PeerAddr, headers: &HeaderMap) -> bool {
        if !peer.0.ip().is_loopback() || has_proxy_provenance(headers) {
            return false;
        }
        let Some(authority) = single_header(headers, header::HOST)
            .and_then(|host| local_authority(host, self.listener))
        else {
            return false;
        };
        single_optional_header(headers, header::ORIGIN).is_ok_and(|origin| {
            origin.is_none_or(|origin| {
                TAURI_ORIGINS.contains(&origin) || local_origin(origin, &authority)
            })
        })
    }
}

pub(crate) fn normalized_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
}

#[must_use]
pub fn local_bind_is_supported(address: SocketAddr) -> bool {
    address.ip().is_loopback()
}

#[derive(Clone, Copy)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

#[derive(Clone)]
pub(crate) struct TrustedIdentityOrigin(pub(crate) Arc<str>);

impl TrustedIdentityOrigin {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) async fn require_trusted_ingress(mut request: Request, next: Next) -> Response {
    let peer = request.extensions().get::<PeerAddr>().copied();
    let local_trusted = request
        .extensions()
        .get::<LocalIngress>()
        .copied()
        .zip(peer)
        .is_some_and(|(ingress, peer)| ingress.authorizes(peer, request.headers()));
    let exact_exposure = request
        .extensions()
        .get::<RouteExposure>()
        .copied()
        .or_else(|| dynamic_exposure(&request));
    let registered = exact_exposure.is_some()
        || (!request
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
            && request
                .extensions()
                .get::<MatchedPath>()
                .is_some_and(|path| registered_route_path(path.as_str())));
    let public_ingress = request.extensions().get::<PublicIngress>().cloned();
    let public_trusted = public_ingress
        .as_ref()
        .zip(peer)
        .zip(exact_exposure)
        .is_some_and(|((ingress, peer), exposure)| {
            ingress.authorizes(peer, request.headers(), exposure)
        });
    if !(local_trusted || public_trusted) || !registered {
        return StatusCode::FORBIDDEN.into_response();
    }
    if exact_exposure == Some(RouteExposure::IdentityProbePublic) {
        let identity_origin = if local_trusted {
            single_header(request.headers(), header::HOST)
                .map(|host| TrustedIdentityOrigin(format!("http://{host}").into()))
        } else {
            public_ingress.and_then(|ingress| ingress.identity_origin())
        };
        let Some(identity_origin) = identity_origin else {
            return StatusCode::FORBIDDEN.into_response();
        };
        request.extensions_mut().insert(identity_origin);
    }
    next.run(request).await
}

fn dynamic_exposure(request: &Request) -> Option<RouteExposure> {
    let method = match *request.method() {
        Method::GET | Method::HEAD => agentsassemble_protocol::HttpMethod::Get,
        Method::POST => agentsassemble_protocol::HttpMethod::Post,
        Method::OPTIONS => {
            match single_header(request.headers(), header::ACCESS_CONTROL_REQUEST_METHOD)? {
                "GET" => agentsassemble_protocol::HttpMethod::Get,
                "POST" => agentsassemble_protocol::HttpMethod::Post,
                _ => return None,
            }
        }
        _ => return None,
    };
    let path = request.extensions().get::<MatchedPath>()?.as_str();
    registered_route_exposure(method, path)
}

pub(crate) fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    single_optional_header(headers, name).ok().flatten()
}

pub(crate) fn single_optional_header(
    headers: &HeaderMap,
    name: header::HeaderName,
) -> Result<Option<&str>, ()> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    value.to_str().map(Some).map_err(|_| ())
}

fn has_proxy_provenance(headers: &HeaderMap) -> bool {
    headers.keys().any(|name| {
        let name = name.as_str();
        EXACT_PROXY_PROVENANCE_HEADERS.contains(&name)
            || name.starts_with("x-forwarded-")
            || name.starts_with("cf-")
    })
}

fn local_origin(origin: &str, request_authority: &Authority) -> bool {
    let Some(origin_authority) = origin
        .strip_prefix("http://")
        .and_then(|authority| authority.parse::<Authority>().ok())
    else {
        return false;
    };
    origin_authority
        .host()
        .eq_ignore_ascii_case(request_authority.host())
        && origin_authority.port_u16() == request_authority.port_u16()
}

fn local_authority(value: &str, listener: SocketAddr) -> Option<Authority> {
    let Ok(authority) = value.parse::<Authority>() else {
        return None;
    };
    let hostname_matches = authority_host_ip(authority.host()) == Some(listener.ip())
        || (authority.host().eq_ignore_ascii_case("localhost")
            && listener_accepts_localhost(listener.ip()));
    (authority.port_u16() == Some(listener.port()) && hostname_matches).then_some(authority)
}

fn listener_accepts_localhost(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.octets() == [127, 0, 0, 1],
        IpAddr::V6(address) => address.is_loopback(),
    }
}

pub(crate) fn is_loopback_http_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || authority_host_ip(host).is_some_and(|address| address.is_loopback())
}

pub(crate) fn authority_host_ip(host: &str) -> Option<IpAddr> {
    normalized_host(host).parse().ok()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{LocalIngress, PeerAddr, RouteExposure, TAURI_ORIGINS};
    use crate::public_ingress::{ManualPublicIngressError, PublicIngress};

    const PORT: u16 = 41_955;

    #[test]
    fn local_authority_accepts_only_exact_loopback_aliases_and_port() {
        for host in ["localhost:41955", "127.0.0.1:41955"] {
            assert!(trusted(&[("host", host)]));
        }
        for host in [
            "localhost",
            "localhost:41956",
            "127.0.0.2:41955",
            "example.com:41955",
        ] {
            assert!(!trusted(&[("host", host)]));
        }

        let alternate = LocalIngress::from_listener(SocketAddr::from(([127, 0, 0, 2], PORT)))
            .unwrap_or_else(|| panic!("alternate loopback listener must be accepted"));
        assert!(alternate.authorizes(loopback_peer(), &headers(&[("host", "127.0.0.2:41955")])));
        assert!(!alternate.authorizes(loopback_peer(), &headers(&[("host", "127.0.0.3:41955")])));
        assert!(!alternate.authorizes(loopback_peer(), &headers(&[("host", "localhost:41955")])));

        let ipv6 = LocalIngress::from_listener(SocketAddr::from((Ipv6Addr::LOCALHOST, PORT)))
            .unwrap_or_else(|| panic!("IPv6 loopback listener must be accepted"));
        for host in ["localhost:41955", "[::1]:41955"] {
            assert!(ipv6.authorizes(loopback_peer(), &headers(&[("host", host)])));
        }
    }

    #[test]
    fn local_origin_preserves_same_server_and_exact_tauri_origins() {
        for (host, origin) in [
            ("localhost:41955", "http://localhost:41955"),
            ("127.0.0.1:41955", "http://127.0.0.1:41955"),
        ] {
            assert!(trusted(&[("host", host), ("origin", origin)]));
        }
        for origin in TAURI_ORIGINS {
            assert!(trusted(&[("host", "127.0.0.1:41955"), ("origin", origin)]));
        }
        assert!(!trusted(&[
            ("host", "127.0.0.1:41955"),
            ("origin", "http://localhost:41955"),
        ]));
        assert!(!trusted(&[
            ("host", "127.0.0.1:41955"),
            ("origin", "https://example.com"),
        ]));
    }

    #[test]
    fn proxy_provenance_and_non_loopback_peer_fail_closed() {
        for header in ["via", "x-real-ip", "x-forwarded-client-cert", "cf-ray"] {
            assert!(!trusted(&[("host", "127.0.0.1:41955"), (header, "proxy"),]));
        }
        let headers = headers(&[("host", "127.0.0.1:41955")]);
        let peer = PeerAddr(SocketAddr::from(([192, 0, 2, 1], 50_000)));
        assert!(!ingress().authorizes(peer, &headers));
    }

    #[test]
    fn manual_public_config_and_headers_are_exact() {
        let ingress = PublicIngress::configured_manual(
            "https://Public.Example.test:443/",
            "manual-proxy-secret-0000000000000001",
        )
        .unwrap_or_else(|error| panic!("build manual ingress: {error}"));
        let trusted = headers(&[
            ("host", "public.example.test"),
            ("x-forwarded-proto", "https"),
            (
                "x-agentsassemble-proxy-token",
                "manual-proxy-secret-0000000000000001",
            ),
            ("origin", "https://public.example.test:443"),
        ]);
        assert!(ingress.authorizes(loopback_peer(), &trusted, RouteExposure::SameOriginPublic));
        assert!(ingress.authorizes(
            loopback_peer(),
            &headers(&[
                ("host", "public.example.test"),
                ("x-forwarded-proto", "https"),
                (
                    "x-agentsassemble-proxy-token",
                    "manual-proxy-secret-0000000000000001",
                ),
                ("origin", "https://directory.example"),
            ]),
            RouteExposure::IdentityProbePublic
        ));
        for origin in [
            "http://public.example.test",
            "https://127.0.0.2",
            "https://localhost.",
            "https://public.example.test/path",
        ] {
            assert!(matches!(
                PublicIngress::configured_manual(origin, "manual-proxy-secret-0000000000000001",),
                Err(ManualPublicIngressError::InvalidOrigin)
            ));
        }
        assert!(matches!(
            PublicIngress::configured_manual("https://public.example.test", "short"),
            Err(ManualPublicIngressError::InvalidSecret)
        ));
    }

    fn trusted(values: &[(&'static str, &'static str)]) -> bool {
        let headers = headers(values);
        ingress().authorizes(loopback_peer(), &headers)
    }

    fn ingress() -> LocalIngress {
        LocalIngress::from_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, PORT)))
            .unwrap_or_else(|| panic!("loopback listener must be accepted"))
    }

    fn loopback_peer() -> PeerAddr {
        PeerAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50_000))
    }

    fn headers(values: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in values {
            headers.append(
                name.parse::<header::HeaderName>()
                    .unwrap_or_else(|error| panic!("parse test header name: {error}")),
                HeaderValue::from_static(value),
            );
        }
        headers
    }
}
