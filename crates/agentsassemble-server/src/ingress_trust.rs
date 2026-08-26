use std::net::SocketAddr;

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode, header, uri::Authority},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::http_api::TAURI_ORIGINS;

const PROXY_PROVENANCE_HEADERS: [&str; 9] = [
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "cf-connecting-ip",
    "cf-ipcountry",
    "cf-ray",
    "cf-visitor",
    "cdn-loop",
];

#[derive(Clone, Copy)]
pub(crate) struct LocalIngress {
    port: u16,
}

impl LocalIngress {
    pub(crate) fn from_listener(address: SocketAddr) -> Option<Self> {
        address.ip().is_loopback().then_some(Self {
            port: address.port(),
        })
    }

    fn authorizes(self, peer: PeerAddr, headers: &HeaderMap) -> bool {
        peer.0.ip().is_loopback()
            && !has_proxy_provenance(headers)
            && single_header(headers, header::HOST)
                .is_some_and(|host| local_authority(host, self.port))
            && single_optional_header(headers, header::ORIGIN).is_ok_and(|origin| {
                origin.is_none_or(|origin| {
                    TAURI_ORIGINS.contains(&origin) || local_origin(origin, self.port)
                })
            })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

pub(crate) async fn require_trusted_ingress(request: Request, next: Next) -> Response {
    let trusted = request
        .extensions()
        .get::<LocalIngress>()
        .copied()
        .zip(request.extensions().get::<PeerAddr>().copied())
        .is_some_and(|(ingress, peer)| ingress.authorizes(peer, request.headers()));
    if !trusted {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    single_optional_header(headers, name).ok().flatten()
}

fn single_optional_header(
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
    PROXY_PROVENANCE_HEADERS
        .iter()
        .any(|name| headers.contains_key(*name))
}

fn local_origin(origin: &str, port: u16) -> bool {
    origin
        .strip_prefix("http://")
        .is_some_and(|authority| local_authority(authority, port))
}

fn local_authority(value: &str, port: u16) -> bool {
    let Ok(authority) = value.parse::<Authority>() else {
        return false;
    };
    authority.port_u16() == Some(port)
        && (authority.host().eq_ignore_ascii_case("localhost")
            || authority.host() == "127.0.0.1"
            || authority.host() == "[::1]")
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{LocalIngress, PeerAddr};

    const PORT: u16 = 41_955;

    #[test]
    fn local_authority_accepts_only_exact_loopback_aliases_and_port() {
        for host in ["localhost:41955", "127.0.0.1:41955", "[::1]:41955"] {
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
    }

    #[test]
    fn local_origin_preserves_same_server_and_exact_tauri_origins() {
        for origin in [
            "http://localhost:41955",
            "http://127.0.0.1:41955",
            "http://[::1]:41955",
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
        ] {
            assert!(trusted(&[("host", "127.0.0.1:41955"), ("origin", origin)]));
        }
        assert!(!trusted(&[
            ("host", "127.0.0.1:41955"),
            ("origin", "https://example.com"),
        ]));
    }

    #[test]
    fn proxy_provenance_and_non_loopback_peer_fail_closed() {
        assert!(!trusted(&[
            ("host", "127.0.0.1:41955"),
            ("x-forwarded-for", "203.0.113.10"),
        ]));
        let headers = headers(&[("host", "127.0.0.1:41955")]);
        let peer = PeerAddr(SocketAddr::from(([192, 0, 2, 1], 50_000)));
        assert!(!ingress().authorizes(peer, &headers));
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
